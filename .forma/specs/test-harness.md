# test-harness Specification

Integration test harness — shared fixtures, concurrency control, and mock infrastructure for springfield CLI tests

| Field | Value |
|-------|-------|
| Src | `crates/springfield/tests/` |
| Status | stable |

## Overview

Springfield integration tests spawn `sgf` as a subprocess, which in turn forks mock `ralph`/`cl`, `pn`, and `fm` processes. When the full test suite runs in parallel (~100+ tests), each test independently spawns these subprocesses, causing process table exhaustion and OS-level resource failures (`WouldBlock` / "Resource temporarily unavailable" on `fork()`).

This spec defines a shared test harness that:
1. **Eliminates redundant mock setup** — a single `LazyLock` creates the shared mock `pn`/`fm` scripts once, reused by all tests.
2. **Caps concurrent `sgf` invocations** — a global concurrency semaphore limits how many tests can spawn `sgf` simultaneously (default: 8).
3. **Enforces `SGF_SKIP_PREFLIGHT=1` on all tests** — prevents `sgf` from spawning real daemon processes during tests.

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

Replaces per-test `setup_mock_pn()` calls. The `TempDir` lives for the process lifetime. All tests reference the same mock scripts via `mock_bin_path()`.

### 2. Concurrency Semaphore (`SGF_PERMITS`)

```rust
static SGF_PERMITS: LazyLock<SgfSemaphore> = LazyLock::new(|| {
    let max = std::env::var("SGF_TEST_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    SgfSemaphore::new(max)
});
```

`SgfSemaphore` wraps a `std::sync::Mutex<usize>` + `Condvar`. Each test acquires a permit before spawning `sgf` and releases it when the child process exits. The max concurrency defaults to 8 and is overridable via `SGF_TEST_MAX_CONCURRENT` env var.

```rust
struct SgfSemaphore {
    mutex: Mutex<usize>,
    condvar: Condvar,
    max: usize,
}

impl SgfSemaphore {
    fn new(max: usize) -> Self { ... }

    fn acquire(&self) -> SemaphoreGuard<'_> {
        let mut count = self.mutex.lock().unwrap();
        while *count >= self.max {
            count = self.condvar.wait(count).unwrap();
        }
        *count += 1;
        SemaphoreGuard { sem: self }
    }
}

struct SemaphoreGuard<'a> { sem: &'a SgfSemaphore }

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.sem.mutex.lock().unwrap();
        *count -= 1;
        self.sem.condvar.notify_one();
    }
}
```

### 3. Guarded `sgf` Runner (`run_sgf`)

```rust
fn run_sgf(cmd: &mut Command) -> std::process::Output {
    let _permit = SGF_PERMITS.acquire();
    cmd.output().expect("failed to run sgf")
}
```

All tests call `run_sgf(&mut cmd)` instead of `cmd.output().unwrap()`. This is the single enforcement point for concurrency control.

### 4. Enhanced `sgf_cmd` Helper

```rust
fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", mock_bin_path());
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd
}
```

The updated `sgf_cmd` automatically injects `mock_bin_path()` and `SGF_SKIP_PREFLIGHT=1`. Tests that need custom PATH or preflight behavior override explicitly.

## File Layout

All harness code lives in `crates/springfield/tests/integration.rs` (the existing file). No new files are created — the helpers section at the top of the file is extended.

## E2E Verification

The harness itself is verified by running the full integration test suite with default parallelism (`cargo test -p springfield --test integration`). Success criteria: all tests pass with no `WouldBlock` or resource exhaustion errors, regardless of the `--test-threads` value.

A dedicated integration test (`harness_semaphore_limits_concurrency`) verifies that the semaphore correctly limits concurrent `sgf` invocations by spawning N+1 tests against a semaphore of size N and asserting that at most N run simultaneously.

## Dependencies

No new crate dependencies. All components use `std` primitives:

| Component | stdlib module |
|-----------|--------------|
| `SgfSemaphore` | `std::sync::{Mutex, Condvar}` |
| `LazyLock` | `std::sync::LazyLock` (already used for `FAKE_HOME`) |
| `TempDir` | `tempfile::TempDir` (already a dev-dependency) |

The `SGF_TEST_MAX_CONCURRENT` env var override uses `std::env::var`.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Semaphore permit timeout | None — `Condvar::wait` blocks indefinitely. Tests have Rust's built-in `#[should_panic]` timeout if needed. In practice, permits are held for <5s per test. |
| `SGF_TEST_MAX_CONCURRENT` set to invalid value | Falls back to default (8). Parsed via `.ok().and_then(\|v\| v.parse().ok()).unwrap_or(8)`. |
| `SGF_TEST_MAX_CONCURRENT=0` | Deadlock — all tests block forever. This is a misconfiguration, not a runtime error. Documented as "must be >= 1". |
| Mock script creation fails (disk full) | `LazyLock` panics on first access, poisoning the `Once` — all tests that reference `MOCK_BINS` will panic with a clear error. |
| `run_sgf` child process spawn fails | Returns `Output` via `.expect()` — test panics with "failed to run sgf". Same behavior as today. |

## Testing

### How to verify the harness works

The harness is verified by running the full integration test suite at default parallelism:

```bash
cargo test -p springfield --test integration
```

Success criteria:
- All tests pass (exit 0)
- No `WouldBlock`, "Resource temporarily unavailable", or "Too many open files" in stderr
- Works at any `--test-threads` value (1, 4, 8, default)

### Stress test

```bash
SGF_TEST_MAX_CONCURRENT=2 cargo test -p springfield --test integration --test-threads=32
```

This forces high parallelism with low concurrency — the harshest test of the semaphore. All tests should still pass, just slower.

### Dedicated harness test

A test `harness_semaphore_limits_concurrency` spawns multiple threads that each acquire a permit, increment a shared atomic counter, sleep briefly, then release. It asserts the counter never exceeds the semaphore max.

### Migration completeness check

After migration, `grep -c 'setup_mock_pn()' crates/springfield/tests/integration.rs` should return `1` (only the definition, or `0` if removed entirely). All tests should use `mock_bin_path()` instead.

Similarly, `grep -c '\.output()\.unwrap()' crates/springfield/tests/integration.rs` should match only non-sgf commands (git, etc.) — all `sgf` invocations should go through `run_sgf()`.

## Related Specifications

- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle
