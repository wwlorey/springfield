# test-harness Specification

Cross-crate integration test harness — concurrency control, process lifecycle guards, mock infrastructure, and environment isolation

| Field | Value |
|-------|-------|
| Src | `crates/springfield/tests/` |
| Status | proven |

## Overview

Springfield integration tests span multiple crates — springfield, pensa, and claude-wrapper all spawn subprocesses that need concurrency control, environment isolation, and mock infrastructure. When the full test suite runs in parallel (~100+ tests), each test independently spawns subprocesses, causing process table exhaustion and OS-level resource failures (`WouldBlock` / "Resource temporarily unavailable" on `fork()`).

This spec defines a shared test harness that:
1. **Eliminates redundant mock setup** — a single `LazyLock` creates the shared mock `pn`/`fm` scripts once, reused by all tests.
2. **Caps concurrent subprocess invocations** — `shutdown::ProcessSemaphore` limits how many tests can spawn `sgf` simultaneously (default: 8, overridable via `SGF_TEST_MAX_CONCURRENT`).
3. **Enforces `SGF_SKIP_PREFLIGHT=1` on all tests** — prevents `sgf` from spawning real daemon processes during tests.
4. **Uses `shutdown::ChildGuard`** — wraps all spawned test processes to prevent leaked children.
5. **Environment isolation** — `.env_remove()` strips environment variables that could leak between tests or affect daemon discovery.

The result is a test suite that runs reliably on any machine regardless of thread count, without changing what the tests are actually testing.

## Architecture

## Components

### 1. Shared Mock Directory (`MOCK_BINS`)

```rust
static MOCK_BINS: LazyLock<(TempDir, String)> = LazyLock::new(|| {
    // Create mock pn and fm scripts once
    // Return (dir_handle, PATH_string_with_mock_dir_prepended)
});

fn mock_bin_path() -> &'static str {
    &MOCK_BINS.1
}
```

A single `LazyLock` creates the shared mock `pn`/`fm` scripts once, reused by all tests. The `TempDir` lives for the process lifetime. All tests reference the same mock scripts via `mock_bin_path()`.

### 2. Concurrency Semaphore (`SGF_PERMITS`)

```rust
static SGF_PERMITS: LazyLock<shutdown::ProcessSemaphore> = LazyLock::new(|| {
    shutdown::ProcessSemaphore::from_env("SGF_TEST_MAX_CONCURRENT", 8)
});
```

Uses `shutdown::ProcessSemaphore` (see [shutdown spec](shutdown.md) Process Lifecycle). Each test acquires a permit before spawning `sgf` and releases it when the child process exits. The max concurrency defaults to 8 and is overridable via `SGF_TEST_MAX_CONCURRENT` env var. Acquire timeout is 60 seconds.

### 3. ChildGuard Usage

All test subprocess spawns use `shutdown::ChildGuard` (see [shutdown spec](shutdown.md) Process Lifecycle):

```rust
let guard = shutdown::ChildGuard::spawn(&mut cmd)?;
let output = guard.wait_with_output_timeout(Duration::from_secs(30))?;
```

This ensures test children are killed on drop (no leaked processes) and provides timeout support (30-second default for test child waits).

### 4. Guarded `sgf` Runner (`run_sgf`)

```rust
fn run_sgf(cmd: &mut Command) -> std::process::Output {
    let _permit = SGF_PERMITS.acquire_timeout(Duration::from_secs(60))
        .expect("timed out waiting for sgf permit");
    let guard = shutdown::ChildGuard::spawn(cmd).expect("failed to spawn sgf");
    guard.wait_with_output_timeout(Duration::from_secs(30))
        .expect("sgf did not exit within 30s")
}
```

All tests call `run_sgf(&mut cmd)` instead of `cmd.output().unwrap()`. This is the single enforcement point for concurrency control, process cleanup, and timeouts.

### 5. Enhanced `sgf_cmd` Helper

```rust
fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", mock_bin_path());
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd.env("SGF_TEST_NO_SETSID", "1");
    cmd.env_remove("PN_DAEMON");
    cmd.env_remove("PN_DAEMON_HOST");
    cmd.env_remove("FM_DAEMON");
    cmd.env_remove("FM_DAEMON_HOST");
    cmd
}
```

The `sgf_cmd` helper automatically injects `mock_bin_path()`, `SGF_SKIP_PREFLIGHT=1`, `SGF_TEST_NO_SETSID=1`, and strips daemon-related env vars. Tests that need custom behavior override explicitly.

### 6. `SGF_TEST_NO_SETSID` Convention

When `SGF_TEST_NO_SETSID=1` is set, springfield's iteration runner and cursus runner skip the `setsid()` call in their `pre_exec` callbacks. `ChildGuard::spawn()` still calls `setpgid(0, 0)` unconditionally — the env var only controls the additional `setsid()` that springfield adds. Without `setsid()`, test children remain in the test runner's process group, making them killable when the test runner exits or times out.

### 7. `SGF_AGENT_COMMAND` for Mock Agents

Tests that exercise the iteration runner use `SGF_AGENT_COMMAND` to point to a mock script that emits fixture NDJSON:

```rust
cmd.env("SGF_AGENT_COMMAND", mock_agent_path);
```

### 8. `open_pty()` Helper

```rust
fn open_pty() -> Option<(OwnedFd, OwnedFd)> {
    // Returns None on CI or when PTY allocation fails
    // Used for tests that need terminal behavior (Ctrl+D, raw mode)
}
```

Returns `Option` to gracefully degrade on headless CI environments.

### 9. `Drop`-Based Daemon Cleanup

Test fixtures that start daemons implement `Drop` to POST to the daemon's `/shutdown` endpoint, ensuring graceful cleanup even when tests panic:

```rust
// forma: TestDaemon (integration.rs), TestEnv (cli_client.rs)
impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.client.post(self.url("/shutdown")).send();
    }
}

// pensa: DualDaemon (starts both pensa + forma daemons)
impl Drop for DualDaemon {
    fn drop(&mut self) {
        let _ = self.client.post(self.pensa_url("/shutdown")).send();
        let _ = self.client.post(self.forma_url("/shutdown")).send();
    }
}

// pensa: PensaOnlyDaemon (pensa daemon only)
impl Drop for PensaOnlyDaemon {
    fn drop(&mut self) {
        let _ = self.client.post(self.url("/shutdown")).send();
    }
}
```

All fixtures use `reqwest::blocking::Client` (not async) because `Drop` is synchronous. The `let _ =` pattern ignores send errors — if the daemon is already dead, the cleanup is a no-op.

This pattern depends on the `/shutdown` endpoint documented in the [forma](forma.md) and [pensa](pensa.md) daemon lifecycle sections.

| Fixture | Crate | Daemons Started |
|---------|-------|-----------------|
| `TestDaemon` | `forma/tests/integration.rs` | forma only |
| `TestEnv` | `forma/tests/cli_client.rs` | forma only |
| `DualDaemon` | `pensa/tests/integration.rs` | pensa + forma |
| `PensaOnlyDaemon` | `pensa/tests/integration.rs` | pensa only |

## File Layout

The harness code lives in test files across crates:
- `crates/springfield/tests/integration.rs` — sgf integration tests
- `crates/pensa/tests/integration.rs` — pensa integration tests
- `crates/claude-wrapper/tests/integration.rs` — claude-wrapper integration tests

Common patterns (ProcessSemaphore, ChildGuard, env isolation) are shared via the `shutdown` crate. Mock setup is per-crate (each crate has its own mock needs).

## E2E Verification

The harness itself is verified by running the full integration test suite with default parallelism (`cargo test --workspace`). Success criteria: all tests pass with no `WouldBlock` or resource exhaustion errors, regardless of the `--test-threads` value.

A dedicated integration test (`harness_semaphore_limits_concurrency`) verifies that the semaphore correctly limits concurrent `sgf` invocations by spawning N+1 tests against a semaphore of size N and asserting that at most N run simultaneously.


## Dependencies

No new crate dependencies beyond what is already in the workspace. All components use `shutdown` crate utilities and `std` primitives:

| Component | Source |
|-----------|--------|
| `ProcessSemaphore` | `shutdown` crate (workspace) |
| `ChildGuard` | `shutdown` crate (workspace) |
| `LazyLock` | `std::sync::LazyLock` (already used for `FAKE_HOME`) |
| `TempDir` | `tempfile::TempDir` (already a dev-dependency) |

The `SGF_TEST_MAX_CONCURRENT` env var override uses `ProcessSemaphore::from_env()`.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Semaphore permit timeout | `acquire_timeout(Duration::from_secs(60))` panics with "timed out waiting for sgf permit". 60-second deadline prevents indefinite hangs in CI. |
| `SGF_TEST_MAX_CONCURRENT` set to invalid value | Falls back to default (8). Parsed via `.ok().and_then(|v| v.parse().ok()).unwrap_or(8)`. |
| `SGF_TEST_MAX_CONCURRENT=0` | `ProcessSemaphore::from_env()` panics with a clear message — max must be >= 1 (see [shutdown spec](shutdown.md) Process Lifecycle). |
| Mock script creation fails (disk full) | `LazyLock` panics on first access, poisoning the `Once` — all tests that reference `MOCK_BINS` will panic with a clear error. |
| `run_sgf` child process spawn fails | Returns `Output` via `.expect()` — test panics with "failed to run sgf". Same behavior as today. |

## Testing

### How to verify the harness works

The harness is verified by running the full integration test suite at default parallelism:

```bash
cargo test --workspace
```

Success criteria:
- All tests pass (exit 0)
- No `WouldBlock`, "Resource temporarily unavailable", or "Too many open files" in stderr
- Works at any `--test-threads` value (1, 4, 8, default)

### Stress test

```bash
SGF_TEST_MAX_CONCURRENT=2 cargo test --workspace --test-threads=32
```

This forces high parallelism with low concurrency — the harshest test of the semaphore. All tests should still pass, just slower.

### Dedicated harness test

A test `harness_semaphore_limits_concurrency` spawns multiple threads that each acquire a permit, increment a shared atomic counter, sleep briefly, then release. It asserts the counter never exceeds the semaphore max.

### Correctness checks

All tests use `mock_bin_path()` for mock binaries and `run_sgf()` for spawning `sgf` — no direct `.output().unwrap()` calls on sgf commands. `SGF_AGENT_COMMAND` points tests to mock agent scripts instead of real agent binaries.

### Timeout conventions

| Timeout | Value | Usage |
|---------|-------|-------|
| Semaphore acquire | 60 seconds | Max wait for a permit before panicking |
| Child process wait | 30 seconds | Max wait for a spawned process to exit |
| Inter-iteration sleep (in tests) | Shortened via env var | Tests should not wait 2 seconds between iterations |

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [forma](forma.md) — Specification management — forma daemon and fm CLI
- [pensa](pensa.md) — Agent persistent memory — SQLite-backed issue/task tracker with pn CLI
- [shutdown](shutdown.md) — Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle
