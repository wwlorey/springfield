# shutdown Specification

Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts

| Field | Value |
|-------|-------|
| Src | `crates/shutdown/` |
| Status | proven |

## Overview

`shutdown` provides:
- **Double-press shutdown**: Two Ctrl+C presses or two Ctrl+D presses within a timeout window trigger shutdown
- **Separate channels**: Ctrl+C and Ctrl+D are independent — pressing Ctrl+C then Ctrl+D does NOT trigger shutdown; the user must double-press the same key
- **Confirmation prompt**: First press prints "Press Ctrl-C again to exit" or "Press Ctrl-D again to exit", matching whichever key was pressed
- **Timeout reset**: If no second press of the same key arrives within 2 seconds, the counter resets and the process continues
- **SIGTERM**: Always triggers immediate shutdown (single signal, no confirmation)
- **Stdin EOF detection**: Background thread monitors stdin for Ctrl+D (EOF) events

## Architecture

```
crates/shutdown/
├── src/
│   └── lib.rs    # ShutdownController, signal registration, stdin monitor
├── Cargo.toml
```

## Process Lifecycle

`shutdown` exports two shared utilities used across all crates that spawn child processes:

### ChildGuard

RAII wrapper around `std::process::Child` that enforces "no leaked processes." Rust's `Child` has no `Drop` implementation — a dropped `Child` leaves the process running. `ChildGuard` fills this gap.

```rust
pub struct ChildGuard {
    child: Option<Child>,
    pid: u32,
}

impl ChildGuard {
    /// Spawn a command in its own process group (via `setpgid(0, 0)` in `pre_exec`).
    /// The process group is used by `kill_process_group` for clean teardown.
    pub fn spawn(cmd: &mut Command) -> io::Result<Self>;

    /// Wrap an already-spawned child.
    pub fn new(child: Child) -> Self;

    pub fn id(&self) -> u32;
    pub fn child_mut(&mut self) -> &mut Child;
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;

    /// Consume the guard, wait for the child, and return its output.
    pub fn wait_with_output(self) -> io::Result<Output>;

    /// Like `wait_with_output` but with a timeout. Returns `TimedOut` error
    /// if the child doesn't exit within the deadline.
    pub fn wait_with_output_timeout(self, timeout: Duration) -> io::Result<Output>;
}

impl Drop for ChildGuard {
    /// On drop: kill_process_group(pid, 200ms), then child.kill() as fallback,
    /// then child.wait() to reap.
    fn drop(&mut self);
}
```

**Convention**: Every `.spawn()` call in the workspace must go through `ChildGuard::spawn()` or be immediately wrapped in `ChildGuard::new()`. Fire-and-forget spawns are prohibited.

### ProcessSemaphore

Concurrency limiter for subprocess invocations. Prevents fork exhaustion when many tests or tasks spawn child processes simultaneously.

```rust
pub struct ProcessSemaphore {
    mutex: Mutex<usize>,
    condvar: Condvar,
    max: usize,
}

impl ProcessSemaphore {
    /// Panics if `max` is 0 — at least one permit must be available.
    pub fn new(max: usize) -> Self;

    /// Create from an environment variable, falling back to `default`.
    /// Panics if the resolved value is 0.
    pub fn from_env(var: &str, default: usize) -> Self;

    pub fn max(&self) -> usize;

    /// Block until a permit is available. Returns a guard that releases
    /// the permit on drop.
    pub fn acquire(&self) -> ProcessSemaphoreGuard<'_>;

    /// Like `acquire` but with a timeout. Returns `None` if the deadline
    /// expires before a permit becomes available.
    pub fn acquire_timeout(&self, timeout: Duration) -> Option<ProcessSemaphoreGuard<'_>>;
}
```

**Test usage**: Tests that spawn child processes (`.output()`, `.spawn()`) create a `LazyLock<ProcessSemaphore>` and call `.acquire()` before each spawn. The guard is held until the child exits. The default max is 8 (overridable via `SGF_TEST_MAX_CONCURRENT`). Timeout for acquire is 60 seconds.

**Constraint**: `max` must be >= 1. `ProcessSemaphore::new()` and `from_env()` panic if `max` is 0, since no permits could ever be issued and all callers would block indefinitely.

## Dependencies

| Crate | Purpose |
|-------|---------|\n| `signal-hook` (0.4) | SIGINT/SIGTERM handler registration |
| `libc` (0.2) | Low-level signal constants, process group kill |

No async runtime. Uses `std::thread` for the stdin monitor and `std::sync::atomic` for cross-thread state.

Dev dependencies:

| Crate | Purpose |
|-------|---------|\n| `nix` (0.29, signal + process) | Signal delivery and process management in tests |
| `serial_test` (3) | Serialize signal-based tests to avoid interference |
| `proptest` (1) | Property-based testing for ChildGuard and ProcessSemaphore invariants |

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `ShutdownController::new()` fails (thread spawn or signal registration) | Returns `io::Error`. Caller should exit gracefully. |
| Stdin monitor thread encounters read error | Breaks out of read loop. `poll()` continues to work (signals still detected). |
| `signal_hook::register()` fails | Propagated as `io::Error` from `ShutdownController::new()`. |
| `kill_process_group` receives PID of already-dead process | `kill(-pid, SIGTERM)` returns `ESRCH` → returns `false`. No panic. |
| `wait_with_output_timeout` exceeds deadline | Returns `TimedOut` variant. Caller decides whether to escalate. |
| `poll()` after shutdown already detected | Returns `Shutdown` immediately (idempotent). |

## Testing

### Unit Tests (`lib.rs`)

All signal-based tests use `#[serial]` to avoid interference between concurrent test runs.

#### ShutdownController Tests

| Test | Description |
|------|-------------|
| `default_config` | Verify `ShutdownConfig::default()` has 2-second timeout and `monitor_stdin: true` |
| `poll_returns_running_initially` | Create controller, verify `poll()` returns `Running` |
| `sigterm_immediate_shutdown` | Register handler, raise SIGTERM, verify `poll()` returns `Shutdown` |
| `single_sigint_returns_pending` | Raise one SIGINT, verify `poll()` returns `Pending` |
| `double_sigint_returns_shutdown` | Raise two SIGINTs, verify `poll()` returns `Shutdown` |
| `sigint_resets_after_timeout` | Raise one SIGINT, sleep past timeout, verify `poll()` returns `Running` |

#### kill_process_group Tests

| Test | Description |
|------|-------------|
| `kill_pg_sends_sigterm_to_group` | Spawn child in new session, call `kill_process_group`, verify child exits with SIGTERM signal |
| `kill_pg_escalates_to_sigkill` | Spawn child that traps SIGTERM, call `kill_process_group` with short timeout, verify child is killed |
| `kill_pg_already_dead` | Spawn child, wait for exit, call `kill_process_group`, verify returns `false` |
| `kill_pg_kills_descendants` | Spawn child that spawns grandchildren, call `kill_process_group`, verify all are dead |

#### ChildGuard Tests

| Test | Description |
|------|-------------|
| `drop_kills_running_process` | Spawn via `ChildGuard::spawn()`, drop guard, verify child is dead |
| `drop_kills_descendants` | Spawn shell with grandchildren via `ChildGuard::spawn()`, drop guard, verify all dead |
| `drop_during_panic_cleans_up` | Panic with `catch_unwind`, verify guard cleans up during unwind |
| `wait_with_output_consumes_child` | Fast process, call `wait_with_output`, verify success and no kill |
| `wait_with_output_timeout_succeeds` | Fast process with piped stdout, `wait_with_output_timeout`, verify output captured |
| `wait_with_output_timeout_kills_on_expire` | Long process, short timeout, verify `TimedOut` error returned |
| `already_exited_no_error` | Spawn fast process, sleep, drop guard — no error |
| `no_zombie_after_drop` | Fast process exits, drop guard, verify no zombie via `waitpid(WNOHANG)` |
| `fallback_kills_non_group_leader` | `ChildGuard::new()` without process group, verify individual kill works as fallback |
| `concurrent_guards_all_cleanup` | Spawn 10 guards, drop all, verify all PIDs are dead |

#### ChildGuard Property Tests (`#[ignore]`)

| Test | Description |
|------|-------------|
| `no_process_leak` | Spawn N guards (1..12), drop all, verify all PIDs are dead (16 cases) |

#### ProcessSemaphore Tests

| Test | Description |
|------|-------------|
| `acquire_up_to_max` | Acquire N=max permits, all succeed without blocking |
| `acquire_blocks_at_max` | At max, new `acquire()` blocks until a guard is released |
| `guard_releases_on_drop` | Acquire, drop, re-acquire succeeds |
| `acquire_timeout_returns_none_on_expire` | At max, `acquire_timeout()` returns `None` after deadline |
| `acquire_timeout_succeeds_when_available` | Under max, `acquire_timeout()` returns `Some(guard)` |
| `from_env_reads_variable` | Set env var, verify `from_env()` reads the value |
| `from_env_uses_default` | No env var set, verify `from_env()` falls back to default |
| `new_zero_panics` | `ProcessSemaphore::new(0)` panics with descriptive message |
| `from_env_zero_panics` | `from_env()` with env var = 0 panics |
| `from_env_default_zero_panics` | `from_env()` with default = 0 and no env var panics |

#### ProcessSemaphore Property Tests

| Test | Description |
|------|-------------|
| `active_permits_never_exceed_max` | Spawn concurrent threads, assert active permit count never exceeds max (32 cases, max in 2..8, threads in 2..16) |

Stdin EOF tests require a PTY or pipe to simulate Ctrl+D, which is complex for unit tests. Stdin EOF behavior is covered by integration tests at the sgf level.

### Integration Tests (at sgf level)

Signal-based integration tests in `crates/springfield/tests/`. Tests set `SGF_TEST_NO_SETSID=1` on spawned commands so children stay in the test's process group (killable by the test runner).

| Test | Description |
|------|-------------|
| `double_ctrl_c_exits_130` | Send two SIGINTs to sgf, verify exit code 130 |
| `single_ctrl_c_continues_after_timeout` | Send one SIGINT, wait 3 seconds, verify process continues |
| `sigterm_exits_immediately` | Send SIGTERM, verify immediate exit with code 130 |
| `confirmation_message_on_first_ctrl_c` | Send one SIGINT, verify stderr contains "Press Ctrl-C again to exit" |
| `double_ctrl_c_kills_entire_process_tree` | Send two SIGINTs, verify exit code 130 and entire process tree (including grandchildren) is dead |
| `iter_sigterm_exits_immediately` | Send SIGTERM to iter runner, verify immediate exit |
| `iter_interactive_restores_terminal_settings_after_agent_corrupts` | Spawn sgf with PTY, let child corrupt terminal settings, verify settings are restored after child exits. Uses `open_pty()` helper; skipped when PTY allocation fails (headless CI) |

## API

```rust
pub struct ShutdownController { /* ... */ }

pub struct ShutdownConfig {
    /// Timeout window for second press (default: 2 seconds)
    pub timeout: Duration,
    /// Whether to monitor stdin for Ctrl+D (EOF) events.
    /// Enable when `is_terminal()` returns true and the process owns stdin.
    /// Disable when stdin is not a terminal or when a child process owns stdin.
    pub monitor_stdin: bool,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        ShutdownConfig {
            timeout: Duration::from_secs(2),
            monitor_stdin: true,
        }
    }
}

impl ShutdownController {
    /// Create and activate the shutdown controller.
    /// Registers SIGINT and SIGTERM handlers immediately.
    /// If `config.monitor_stdin` is true, spawns a background thread
    /// to read stdin for EOF events.
    ///
    /// Signal handler SigIds are stored and unregistered on Drop,
    /// preventing handler accumulation across controller lifetimes.
    pub fn new(config: ShutdownConfig) -> io::Result<Self>;

    /// Poll the controller's state. Returns the current shutdown status.
    /// Callers should invoke this in their polling loops (e.g., every 50-100ms).
    ///
    /// Side effects on first press:
    /// - Prints "Press Ctrl-C again to exit" or "Press Ctrl-D again to exit"
    ///   to stderr
    /// - Starts the timeout window
    ///
    /// Side effects on timeout expiry:
    /// - Resets the press state, allowing a fresh double-press sequence
    pub fn poll(&self) -> ShutdownStatus;
}

/// SigIds from signal_hook are unregistered on Drop, cleaning up
/// the global signal handler table.
impl Drop for ShutdownController {
    fn drop(&mut self);
}

pub enum ShutdownStatus {
    /// No shutdown requested. Continue normally.
    Running,
    /// First press detected, waiting for confirmation.
    /// The controller has already printed the confirmation message.
    Pending,
    /// Double-press confirmed or SIGTERM received. Shut down now.
    Shutdown,
}
```

### Design Decisions

**Separate counters for Ctrl+C and Ctrl+D**: The controller maintains two independent press counters (`sigint_count` and `eof_count`). A first Ctrl+C starts a Ctrl+C confirmation window; a subsequent Ctrl+D does NOT satisfy it (and vice versa). Each channel has its own 2-second timeout. If both channels have a pending first press, either channel's second press triggers shutdown independently.

**First press resets the other channel**: When the user presses Ctrl+C, any pending Ctrl+D confirmation is reset (and vice versa). This prevents confusion — only one confirmation prompt is active at a time.

**Confirmation message on stderr**: The "Press Ctrl-C again to exit" message is printed to stderr, not stdout. This prevents it from appearing in piped output or NDJSON streams while remaining visible to the user in the terminal.

**SIGTERM bypasses double-press**: SIGTERM sets an `AtomicBool` flag. `poll()` checks this flag first and returns `ShutdownStatus::Shutdown` immediately. SIGTERM is the "hard stop" signal used by process managers and parent processes.

**SigId cleanup on Drop**: The controller stores the `SigId` handles returned by `signal_hook::register()`. On `Drop`, it calls `signal_hook::unregister()` for each, preventing handler accumulation when controllers are created and destroyed across test runs or within long-lived processes.

## Stdin EOF Detection

A background thread monitors stdin using `libc::poll()` with a 200ms timeout followed by `read()`. The thread calls `poll()` to wait for input readiness (or timeout), then calls `read()` only when data is available. In a Unix terminal, Ctrl+D causes `read()` to return 0 bytes (EOF) when the input buffer is empty. Each EOF event increments the `eof_count` atomic counter.

After an EOF, the thread continues reading — Ctrl+D in a terminal does not permanently close stdin. The next `read()` call blocks until the user provides more input or another EOF.

### Stdin Monitor Lifecycle

- Started by `ShutdownController::new()` when `config.monitor_stdin` is true
- The thread is named `shutdown-stdin`
- The thread runs for the lifetime of the controller
- On `Drop`, the controller signals the thread to stop (via `AtomicBool`), then joins the thread. The thread may remain blocked on `read()` briefly — the 200ms poll timeout ensures it checks the stop flag regularly.

### When to Enable/Disable Stdin Monitoring

The `monitor_stdin` decision is based on execution mode and terminal state:

| Context | `monitor_stdin` | Rationale |
|---------|-----------------|-----------|
| AFK mode, stdin is a terminal | `true` | The child's stdin is `/dev/null` (see [Integration with sgf](#integration-with-sgf)). sgf's own stdin remains the terminal — the user can press Ctrl+D to signal shutdown. |
| AFK mode, stdin is piped | `false` | No user to press Ctrl+D. |
| Interactive mode | `false` | The user interacts with the agent through a PTY. Only Ctrl+C works for shutdown. |
| Resume session | `false` | Stdin belongs to the child process. |

The caller determines the value based on execution mode and `stdin.is_terminal()`. There is no environment variable — the process itself knows whether it owns stdin.

## Integration with sgf

sgf creates a `ShutdownController` before spawning the agent. The controller configuration varies by mode — AFK mode owns stdin; interactive modes let the agent own the PTY.

### AFK Iterations (iter_runner)

The iter runner spawns the agent with `setsid()` in `pre_exec`, making the child a session leader (new session + new process group). sgf creates the controller with `monitor_stdin: true` when stdin is a terminal (stdin is free — no user interaction). Stdin is set to `Stdio::null()` to prevent the agent from inheriting the terminal fd and modifying terminal settings (e.g., disabling ISIG via `tcsetattr`). Without this, the agent can put the terminal in raw mode, causing Ctrl+C to emit byte `0x03` instead of generating SIGINT and Ctrl+D to emit byte `0x04` instead of triggering EOF. With `Stdio::null()`, the terminal fd stays under sgf's exclusive control and ISIG remains enabled.

The polling loop calls `controller.poll()` every ~100ms. On `Shutdown`, sgf calls `kill_process_group(child_pid, 200ms)` directly on the child's PID (which equals the session/process group ID due to `setsid()`), then `child.wait()` to reap.

Both double Ctrl+C and double Ctrl+D trigger shutdown in AFK mode.

### Interactive Iterations (pty_tee)

The interactive path spawns the agent via `pty_tee::run_interactive_with_pty()`, which allocates a PTY pair. The child is spawned with `setsid()` and `TIOCSCTTY` to become the session leader and controlling process of the PTY. The agent's stdin/stdout/stderr are connected to the PTY slave. The master side is used by sgf for output multiplexing (terminal + log file).

sgf creates the controller with `monitor_stdin: false` — the user interacts with the agent through the PTY, so sgf does not monitor its own stdin for EOF. Only double Ctrl+C works for shutdown (SIGINT is delivered to sgf's process group by the terminal, not through the PTY). The agent receives its own copy of SIGINT if the terminal's foreground process group includes it.

### Cursus Runner

The cursus runner determines `monitor_stdin` from the effective mode and terminal state: `is_afk && stdin.is_terminal()`. It delegates agent spawning to the iter runner, which applies `setsid()` unconditionally (unless `SGF_TEST_NO_SETSID` is set).

### Terminal Settings Preservation

The agent (Claude Code) may modify terminal settings via inherited file descriptors (e.g., calling `tcsetattr()` on the stderr fd to enable raw mode). This can disable `ISIG`, causing Ctrl+C to send byte `0x03` instead of generating SIGINT.

sgf saves terminal settings (`tcgetattr` on stdin fd) before spawning the agent and restores them (`tcsetattr`) after the agent exits. This ensures the terminal is always in a known-good state for signal handling between iterations and after the agent run. If stdin is not a terminal (e.g., piped input), `tcgetattr` fails and no save/restore occurs — this is expected and harmless.

## Process Group Kill with Escalation

The `kill_process_group` function provides graceful-then-forceful termination of a process group. Killing a single PID leaves descendants (agent tool subprocesses, build commands, etc.) orphaned and running. This function kills the entire process group.

### Process isolation in sgf

sgf spawns agent children with `setsid()` in `pre_exec`, making each child a session leader. This creates a new session and a new process group (PID = PGID = SID). `kill_process_group` uses the negative PID to target all processes in that group.

`ChildGuard::spawn()` uses `setpgid(0, 0)` instead of `setsid()`, placing the child in its own process group without creating a new session. This is used for standalone process management outside of sgf's iteration loop (e.g., in tests). Either approach gives the child its own PGID equal to its PID, which is what `kill_process_group` requires.

The `SGF_TEST_NO_SETSID` env var (see test-harness spec) disables `setsid()` in the iter runner so children stay in the test's process group and are killable by the test runner.

### API

```rust
/// Kill a process group gracefully, escalating to SIGKILL after a timeout.
///
/// 1. Send SIGTERM to the process group (-pid)
/// 2. Poll every 100ms for up to `timeout` checking if the group leader is still alive
/// 3. If still alive after timeout, send SIGKILL to the process group (-pid)
///
/// Returns `true` if the process group was successfully terminated (SIGTERM or SIGKILL),
/// `false` if the process was already dead.
pub fn kill_process_group(pid: u32, timeout: Duration) -> bool;
```

### Behavior

| Step | Action | Detail |
|------|--------|--------|
| 1 | `kill(-pid, SIGTERM)` | Signal the entire process group to terminate gracefully |
| 2 | Poll loop | Every 100ms, check `kill(-pid, 0)` (signal 0 = liveness test) |
| 3a | Process exits within timeout | Return `true` |
| 3b | Timeout expires (process still alive) | `kill(-pid, SIGKILL)`, return `true` |
| 4 | Process already dead at step 1 | `kill(-pid, SIGTERM)` returns `ESRCH` → return `false` |

The `pid` parameter is the group leader's PID (which equals the PGID). The negative PID in `kill()` targets all processes in that process group.

### Default Timeout

sgf uses a 200ms timeout when calling `kill_process_group` on shutdown. After a double-press confirmation, the user has already signaled clear intent to exit — there is no meaningful cleanup for an AI agent subprocess, so a short grace window (enough for buffer flushes) is sufficient before escalating to SIGKILL.

`ChildGuard` also uses a 200ms timeout (`CHILD_GUARD_KILL_TIMEOUT`) in its `Drop` implementation.
