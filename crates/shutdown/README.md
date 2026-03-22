# shutdown

Shared graceful shutdown utilities — double-press Ctrl+C/Ctrl+D detection with confirmation prompts, RAII child process guards, and subprocess concurrency control.

## Public API

### `ShutdownController`

Registers SIGINT/SIGTERM handlers and optionally monitors stdin for Ctrl+D (EOF). Poll-based — call `poll()` periodically in your main loop.

- **Double-press shutdown**: Two Ctrl+C or two Ctrl+D presses within a 2-second window trigger shutdown. The two channels are independent.
- **SIGTERM**: Immediate shutdown (single signal, no confirmation).
- **First press** prints "Press Ctrl-C again to exit" (or Ctrl-D) to stderr.
- Signal handlers are unregistered on `Drop`.

```rust
let controller = ShutdownController::new(ShutdownConfig::default())?;

loop {
    match controller.poll() {
        ShutdownStatus::Running => { /* continue */ }
        ShutdownStatus::Pending => { /* waiting for confirmation */ }
        ShutdownStatus::Shutdown => { break; }
    }
    std::thread::sleep(Duration::from_millis(50));
}
```

`ShutdownConfig` fields:
- `timeout: Duration` — confirmation window (default: 2 seconds)
- `monitor_stdin: bool` — watch stdin for EOF (default: `true`; disable when child owns stdin)

### `ChildGuard`

RAII wrapper around `std::process::Child`. Rust's `Child` has no `Drop` — dropped handles leak processes. `ChildGuard` kills the process group on drop.

```rust
let guard = ChildGuard::spawn(Command::new("my-program").arg("--flag"))?;
let output = guard.wait_with_output()?;
```

**Convention**: Every `.spawn()` call in the workspace must use `ChildGuard::spawn()` or be immediately wrapped in `ChildGuard::new()`. Fire-and-forget spawns are prohibited.

Key methods:
- `spawn(cmd)` — spawn in a new process group (via `setpgid`)
- `new(child)` — wrap an existing `Child`
- `wait_with_output(self)` — consume guard, wait for output
- `wait_with_output_timeout(self, timeout)` — wait with timeout, kills on expiry
- `try_wait(&mut self)` — non-blocking poll
- **Drop**: calls `kill_process_group(pid, 200ms)`, then fallback `child.kill()`

### `kill_process_group(pid, timeout) -> bool`

Sends SIGTERM to the entire process group (`kill(-pid, ...)`), polls for exit, escalates to SIGKILL after timeout. Returns `true` if terminated, `false` if already dead.

### `ProcessSemaphore`

Counting semaphore for throttling concurrent subprocess spawns. Prevents fork exhaustion in test suites.

```rust
static SEM: LazyLock<ProcessSemaphore> = LazyLock::new(|| {
    ProcessSemaphore::from_env("SGF_TEST_MAX_CONCURRENT", 8)
});

let _permit = SEM.acquire();
let output = ChildGuard::spawn(&mut cmd)?.wait_with_output()?;
// permit released on drop
```

Key methods:
- `new(max)` — create with max permits (panics if 0)
- `from_env(var, default)` — read max from env var
- `acquire()` — blocking acquire, returns `ProcessSemaphoreGuard`
- `acquire_timeout(timeout)` — returns `None` on timeout

## Test Conventions

- Set `SGF_TEST_NO_SETSID=1` on spawned commands so children stay killable by the test runner.
- Use `ProcessSemaphore` to throttle `.output()` calls in tests.
