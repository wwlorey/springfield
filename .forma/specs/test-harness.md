# test-harness Specification

Cross-crate integration test harness — concurrency control, process lifecycle guards, mock infrastructure, and environment isolation

| Field | Value |
|-------|-------|
| Src | `crates/springfield/tests/` |
| Status | proven |

## Overview

Springfield integration tests span multiple crates — springfield, pensa, forma, and claude-wrapper all spawn subprocesses that need concurrency control, environment isolation, and mock infrastructure. When the full test suite runs in parallel (~100+ tests), each test independently spawns subprocesses, causing process table exhaustion and OS-level resource failures (`WouldBlock` / "Resource temporarily unavailable" on `fork()`).

This spec defines a shared test harness that:
1. **Eliminates redundant mock setup** — a single `LazyLock` creates the shared mock `pn`/`fm` scripts once, reused by all springfield tests.
2. **Caps concurrent subprocess invocations** — `shutdown::ProcessSemaphore` limits how many tests can spawn binaries simultaneously (default: 8, overridable via `SGF_TEST_MAX_CONCURRENT`). Each crate maintains its own static semaphore instance.
3. **Enforces `SGF_SKIP_PREFLIGHT=1` on all sgf tests** — prevents `sgf` from spawning real daemon processes during tests.
4. **Uses `shutdown::ChildGuard`** — wraps all spawned test processes to prevent leaked children.
5. **Environment isolation** — `.env_remove()` strips environment variables that could leak between tests or affect daemon discovery.
6. **Drop-based daemon cleanup** — test fixtures that start real daemons implement `Drop` to POST to the daemon's `/shutdown` endpoint.

The result is a test suite that runs reliably on any machine regardless of thread count, without changing what the tests are actually testing.

## Architecture

## Components

### 1. Shared Mock Directory (`MOCK_BINS`)

```rust
static MOCK_BINS: LazyLock<(TempDir, String)> = LazyLock::new(|| {
    let mock_dir = TempDir::new().unwrap();
    create_mock_script(mock_dir.path(), "pn", "#\!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#\!/bin/sh\nexit 0\n");
    // Prepend mock_dir to PATH so mocks shadow real binaries
    let path = format\!("{}:{}", mock_dir.path().display(), env::var("PATH").unwrap());
    (mock_dir, path)
});

fn mock_bin_path() -> &'static str {
    &MOCK_BINS.1
}
```

A single `LazyLock` creates the shared mock `pn`/`fm` scripts once, reused by all springfield tests. The `TempDir` lives for the process lifetime. All tests reference the same mock scripts via `mock_bin_path()`.

### 2. Fake Home (`FAKE_HOME`)

```rust
static FAKE_HOME: LazyLock<TempDir> = LazyLock::new(|| {
    let home = TempDir::new().unwrap();
    let prompts = home.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts).unwrap();
    // Writes prompt template files into .sgf/prompts/
    home
});
```

A static `LazyLock` creates a shared fake HOME directory containing `.sgf/prompts/` with prompt templates. Used by `sgf_cmd()` to isolate tests from the real user home.

### 3. Per-Crate Concurrency Semaphores

Each crate that spawns subprocesses in tests maintains its own static semaphore, all reading the same env var:

| Crate | Static Name | Env Var | Default |
|-------|-------------|---------|---------|
| springfield | `SGF_PERMITS` | `SGF_TEST_MAX_CONCURRENT` | 8 |
| pensa | `PN_PERMITS` | `SGF_TEST_MAX_CONCURRENT` | 8 |
| claude-wrapper | `CL_PERMITS` | `SGF_TEST_MAX_CONCURRENT` | 8 |

All use `shutdown::ProcessSemaphore::from_env()`. Acquire timeout is 60 seconds.

### 4. ChildGuard Usage

All test subprocess spawns use `shutdown::ChildGuard` (see [shutdown spec](shutdown.md) Process Lifecycle):

```rust
cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
ChildGuard::spawn(cmd)
    .expect("spawn sgf")
    .wait_with_output_timeout(Duration::from_secs(30))
    .expect("failed to run sgf")
```

This ensures test children are killed on drop (no leaked processes) and provides timeout support (30-second default for test child waits).

### 5. Guarded Runners

Each crate has a guarded runner that enforces concurrency control, piped I/O, process cleanup, and timeouts:

**springfield** — `run_sgf` and `run_sgf_timeout`:
```rust
fn run_sgf(cmd: &mut Command) -> std::process::Output {
    run_sgf_timeout(cmd, Duration::from_secs(30))
}

fn run_sgf_timeout(cmd: &mut Command, timeout: Duration) -> std::process::Output {
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn sgf")
        .wait_with_output_timeout(timeout)
        .expect("failed to run sgf")
}
```

**pensa** — `run_pn`:
```rust
fn run_pn(cmd: &mut Command) -> std::process::Output {
    let _permit = PN_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn pn")
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to run pn")
}
```

**claude-wrapper** — `run_cl`:
```rust
fn run_cl(cmd: &mut Command) -> std::process::Output {
    let _permit = CL_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn cl")
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to run cl")
}
```

### 6. Enhanced `sgf_cmd` Helpers

```rust
fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", mock_bin_path());
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd.env("SGF_TEST_NO_SETSID", "1");
    cmd.env("SGF_TEST_ITER_DELAY_MS", "0");
    cmd.env_remove("PN_DAEMON");
    cmd.env_remove("PN_DAEMON_HOST");
    cmd.env_remove("FM_DAEMON");
    cmd.env_remove("FM_DAEMON_HOST");
    cmd
}
```

The `sgf_cmd` helper automatically injects `mock_bin_path()`, `SGF_SKIP_PREFLIGHT=1`, `SGF_TEST_NO_SETSID=1`, `SGF_TEST_ITER_DELAY_MS=0`, and strips daemon-related env vars. Tests that need custom behavior override explicitly.

A variant `sgf_cmd_with_path(dir, path)` accepts a custom PATH string instead of using `mock_bin_path()`, used for tests that need real or custom binaries on the PATH.

### 7. `SGF_TEST_NO_SETSID` Convention

When `SGF_TEST_NO_SETSID=1` is set, springfield's iteration runner and cursus runner skip the `setsid()` call in their `pre_exec` callbacks. `ChildGuard::spawn()` still calls `setpgid(0, 0)` unconditionally — the env var only controls the additional `setsid()` that springfield adds. Without `setsid()`, test children remain in the test runner's process group, making them killable when the test runner exits or times out.

### 8. `SGF_AGENT_COMMAND` for Mock Agents

Tests that exercise the iteration runner use `SGF_AGENT_COMMAND` to point to a mock script that emits fixture NDJSON:

```rust
cmd.env("SGF_AGENT_COMMAND", mock_agent_path);
```

### 9. `open_pty()` Helper

```rust
fn open_pty() -> Option<(OwnedFd, OwnedFd)> {
    // Uses libc::posix_openpt, grantpt, unlockpt, ptsname
    // Returns None when PTY allocation fails (e.g. headless CI)
}
```

Returns `Option` to gracefully degrade on headless CI environments. Used for tests that need terminal behavior (Ctrl+D, raw mode).

### 10. `Drop`-Based Daemon Cleanup

Test fixtures that start daemons implement `Drop` to POST to the daemon's `/shutdown` endpoint, ensuring graceful cleanup even when tests panic:

```rust
// forma: TestDaemon (integration.rs)
impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.client.post(self.url("/shutdown")).send();
    }
}

// forma: TestEnv (cli_client.rs)
impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = self.http.post(format\!("http://localhost:{}/shutdown", self.port)).send();
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
- `crates/springfield/tests/integration.rs` — sgf integration tests (MOCK_BINS, FAKE_HOME, SGF_PERMITS, sgf_cmd, run_sgf, open_pty)
- `crates/pensa/tests/integration.rs` — pensa integration tests (PN_PERMITS, run_pn, DualDaemon, PensaOnlyDaemon)
- `crates/claude-wrapper/tests/integration.rs` — claude-wrapper integration tests (CL_PERMITS, run_cl)
- `crates/forma/tests/integration.rs` — forma daemon integration tests (TestDaemon)
- `crates/forma/tests/cli_client.rs` — forma CLI client tests (TestEnv)

Common patterns (ProcessSemaphore, ChildGuard, env isolation) are shared via the `shutdown` crate. Mock setup is per-crate (each crate has its own mock needs).

## E2E Verification

The harness itself is verified by running the full integration test suite with default parallelism (`cargo test --workspace`). Success criteria: all tests pass with no `WouldBlock` or resource exhaustion errors, regardless of the `--test-threads` value.

A dedicated integration test (`harness_semaphore_limits_concurrency`) verifies that the semaphore correctly limits concurrent invocations by spawning multiple threads against a `ProcessSemaphore::new(3)` and asserting that a shared `AtomicUsize` counter never exceeds the semaphore max.

## Dependencies

No new crate dependencies beyond what is already in the workspace. All components use `shutdown` crate utilities and `std` primitives:

| Component | Source |
|-----------|--------|
| `ProcessSemaphore` | `shutdown` crate (workspace) |
| `ChildGuard` | `shutdown` crate (workspace) |
| `LazyLock` | `std::sync::LazyLock` |
| `TempDir` | `tempfile::TempDir` (dev-dependency) |
| `reqwest::blocking::Client` | `reqwest` (dev-dependency, used in daemon test fixtures) |

The `SGF_TEST_MAX_CONCURRENT` env var override uses `ProcessSemaphore::from_env()`.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Semaphore permit timeout | `acquire_timeout(Duration::from_secs(60))` panics with "semaphore timed out". 60-second deadline prevents indefinite hangs in CI. |
| `SGF_TEST_MAX_CONCURRENT` set to invalid value | Falls back to default (8). Parsed via `.ok().and_then(|v| v.parse().ok()).unwrap_or(default)`. |
| `SGF_TEST_MAX_CONCURRENT=0` | `ProcessSemaphore::from_env()` panics with a clear message — max must be >= 1 (see [shutdown spec](shutdown.md) Process Lifecycle). |
| Mock script creation fails (disk full) | `LazyLock` panics on first access, poisoning the `Once` — all tests that reference `MOCK_BINS` will panic with a clear error. |
| Child process spawn fails | Panics via `.expect("spawn sgf")` / `.expect("spawn pn")` / `.expect("spawn cl")` depending on the crate. |
| Child process timeout | `wait_with_output_timeout` panics via `.expect("failed to run sgf")` (or equivalent). Default timeout is 30 seconds; `run_sgf_timeout` accepts a custom duration. |
| Daemon startup timeout | Daemon fixtures poll readiness in a loop (50 iterations x 100ms = 5 seconds max). Panics if the daemon never becomes ready. |

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

A test `harness_semaphore_limits_concurrency` creates a `ProcessSemaphore::new(3)`, spawns multiple threads that each acquire a permit, increment a shared `AtomicUsize` counter, sleep briefly, then release. It asserts the counter never exceeds the semaphore max.

### Correctness checks

All springfield tests use `mock_bin_path()` for mock binaries and `run_sgf()` / `run_sgf_timeout()` for spawning `sgf` — no direct `.output().unwrap()` calls on sgf commands. Pensa tests use `run_pn()` and claude-wrapper tests use `run_cl()`. `SGF_AGENT_COMMAND` points tests to mock agent scripts instead of real agent binaries.

### Timeout conventions

| Timeout | Value | Usage |
|---------|-------|-------|
| Semaphore acquire | 60 seconds | Max wait for a permit before panicking |
| Child process wait | 30 seconds | Default max wait for a spawned process to exit |
| Daemon startup poll | 50 x 100ms (5 seconds) | Max wait for daemon to become ready |
| Readiness file poll | 25ms interval, 5s max | `wait_for_ready()` polling for file existence |
| Inter-iteration delay (in tests) | 0ms | `SGF_TEST_ITER_DELAY_MS=0` eliminates delay between iterations |

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [forma](forma.md) — Specification management — forma daemon and fm CLI
- [pensa](pensa.md) — Agent persistent memory — SQLite-backed issue/task tracker with pn CLI
- [shutdown](shutdown.md) — Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle
