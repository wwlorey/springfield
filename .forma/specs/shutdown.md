# shutdown Specification

Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts

| Field | Value |
|-------|-------|
| Src | `crates/shutdown/` |
| Status | stable |

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
    pub fn new(max: usize) -> Self;

    /// Create from an environment variable, falling back to `default`.
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

## Dependencies

| Crate | Purpose |
|-------|---------|
| `signal-hook` (0.4) | SIGINT/SIGTERM handler registration |
| `libc` (0.2) | Low-level signal constants, process group kill |

No async runtime. Uses `std::thread` for the stdin monitor and `std::sync::atomic` for cross-thread state.

Dev dependencies:

| Crate | Purpose |
|-------|---------|
| `nix` (0.29, signal + process) | Signal delivery and process management in tests |
| `serial_test` (3) | Serialize signal-based tests to avoid interference |

## Error Handling

Errors are handled via `io::Result` on `ShutdownController::new()`. The library is infallible once created — `poll()` never fails.

## Testing

### Unit Tests (`lib.rs`)

All signal-based tests use `#[serial]` to avoid interference between concurrent test runs.

| Test | Description |
|------|-------------|
| `sigterm_immediate_shutdown` | Register handler, raise SIGTERM, verify `poll()` returns `Shutdown` |
| `single_sigint_returns_pending` | Raise one SIGINT, verify `poll()` returns `Pending` |
| `double_sigint_returns_shutdown` | Raise two SIGINTs, verify `poll()` returns `Shutdown` |
| `sigint_resets_after_timeout` | Raise one SIGINT, sleep past timeout, verify `poll()` returns `Running` |
| `default_config` | Verify `ShutdownConfig::default()` has 2-second timeout and `monitor_stdin: true` |
| `poll_returns_running_initially` | Create controller, verify `poll()` returns `Running` |
| `kill_pg_sends_sigterm_to_group` | Spawn a child with `ChildGuard::spawn()`, call `kill_process_group`, verify child exits (not SIGKILL — check exit signal) |
| `kill_pg_escalates_to_sigkill` | Spawn a child that traps SIGTERM (ignores it), call `kill_process_group` with short timeout, verify child is killed |
| `kill_pg_already_dead` | Spawn a child, wait for it to exit, call `kill_process_group`, verify returns `false` |
| `kill_pg_kills_descendants` | Spawn a child with `ChildGuard::spawn()` that itself spawns a grandchild, call `kill_process_group`, verify both are dead |
| `child_guard_kills_on_drop` | Spawn a long-running child via `ChildGuard::spawn()`, drop the guard, verify child is dead |
| `process_semaphore_limits_concurrency` | Create semaphore with max=2, acquire 3 permits on threads, verify third blocks until first releases |
| `sigid_cleanup_on_drop` | Create and drop a controller, verify signal handlers are unregistered |

Stdin EOF tests require a PTY or pipe to simulate Ctrl+D, which is complex for unit tests. Stdin EOF behavior is covered by integration tests at the sgf level.

### Integration Tests (at sgf level)

Signal-based integration tests in `crates/springfield/tests/`. Tests set `SGF_TEST_NO_SETSID=1` on spawned commands so children stay in the test's process group (killable by the test runner).

| Test | Description |
|------|-------------|
| `double_ctrl_c_exits_130` | Send two SIGINTs to sgf, verify exit code 130 |
| `single_ctrl_c_continues_after_timeout` | Send one SIGINT, wait 3 seconds, verify process continues |
| `sigterm_exits_immediately` | Send SIGTERM, verify immediate exit with code 130 |
| `confirmation_message_on_first_ctrl_c` | Send one SIGINT, verify stderr contains "Press Ctrl-C again to exit" |

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

A background thread reads stdin using blocking `read()`. In a Unix terminal, Ctrl+D causes `read()` to return 0 bytes (EOF) when the input buffer is empty. Each EOF event increments the `eof_count` atomic counter.

After an EOF, the thread continues reading — Ctrl+D in a terminal does not permanently close stdin. The next `read()` call blocks until the user provides more input or another EOF.

### Stdin Monitor Lifecycle

- Started by `ShutdownController::new()` when `config.monitor_stdin` is true
- The thread runs for the lifetime of the controller
- On `Drop`, the controller signals the thread to stop (via `AtomicBool`). The thread may remain blocked on `read()` — this is acceptable since the process is exiting.

### When to Enable/Disable Stdin Monitoring

The `monitor_stdin` decision is based on mode and `is_terminal()`:

| Context | `monitor_stdin` | Rationale |
|---------|-----------------|-----------|
| AFK mode (stdin is `/dev/null`) | `true` | Stdin is free — sgf owns it for shutdown detection. `is_terminal()` returns false but monitor is enabled because sgf explicitly controls stdin. |
| Interactive/non-AFK mode | `false` | Stdin belongs to the child process (user interacts with Claude). Only Ctrl+C works for shutdown. |
| Stdin is piped (not a terminal) | `false` | No user to press Ctrl+D. |

The caller determines the value based on execution mode. There is no environment variable — the process itself knows whether it owns stdin.

## Integration with sgf

sgf creates a `ShutdownController` before invoking `cl` (the agent). The controller configuration varies by mode — AFK mode owns stdin and terminal signals; non-AFK and interactive modes let the child own them.

### AFK Loops

`ChildGuard::spawn()` isolates the agent child in its own process group (via `setpgid(0, 0)` in `pre_exec`). sgf creates the controller with `monitor_stdin: true` (stdin is free — no user interaction). The 50ms polling loop calls `controller.poll()`. Both double Ctrl+C and double Ctrl+D trigger shutdown. On `Shutdown`, the `ChildGuard` is dropped, which calls `kill_process_group(pid, 200ms)` — SIGTERM to the group, escalating to SIGKILL after timeout (see [Process Group Kill with Escalation](#process-group-kill-with-escalation)).

**Stdin isolation**: sgf passes `Stdio::null()` for stdin when spawning the agent in AFK mode. This prevents the agent from inheriting the terminal fd and modifying terminal settings (e.g., disabling ISIG via `tcsetattr`). Without this, the agent can put the terminal in raw mode, causing Ctrl+C to emit byte `0x03` instead of generating SIGINT and Ctrl+D to emit byte `0x04` instead of triggering EOF. With `Stdio::null()`, the terminal fd stays under sgf's exclusive control and ISIG remains enabled.

### Non-AFK Loops (Interactive Iterations)

No `setsid()` — the agent stays in sgf's process group so it (and the agent) receive terminal signals naturally. sgf creates the controller with `monitor_stdin: false` — stdin belongs to the child for user interaction with Claude. Only double Ctrl+C works for shutdown (Ctrl+D goes to Claude as normal input). Both sgf and the child receive SIGINT; sgf's handler prints "Press Ctrl-C again to exit" while Claude handles the signal with its own logic.

### Interactive Stages (`spec`, `issues log`)

`monitor_stdin: false`, no `setsid()`. Same rationale as non-AFK loops — the user types directly into Claude.

### Terminal Settings Preservation

The agent (Claude Code) may modify terminal settings via inherited file descriptors (e.g., calling `tcsetattr()` on the stderr fd to enable raw mode). This can disable `ISIG`, causing Ctrl+C to send byte `0x03` instead of generating SIGINT.

sgf saves terminal settings (`tcgetattr` on stdin fd) before spawning the agent and restores them (`tcsetattr`) after the agent exits. This ensures the terminal is always in a known-good state for signal handling between iterations and after the agent run. If stdin is not a terminal (e.g., piped input), `tcgetattr` fails and no save/restore occurs — this is expected and harmless.

## Process Group Kill with Escalation

The `kill_process_group` function provides graceful-then-forceful termination of a process group. sgf spawns children with `setsid()`, making each child a session leader where PID=PGID. Killing a single PID leaves descendants (agent tool subprocesses, build commands, etc.) orphaned and running. This function kills the entire process group.

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
| 2 | Poll loop | Every 100ms, check `kill(pid, 0)` (signal 0 = liveness test) |
| 3a | Process exits within timeout | Return `true` |
| 3b | Timeout expires (process still alive) | `kill(-pid, SIGKILL)`, return `true` |
| 4 | Process already dead at step 1 | `kill(-pid, SIGTERM)` returns `ESRCH` → return `false` |

The `pid` parameter is the group leader's PID (which equals the PGID due to `setsid()`). The negative PID in `kill()` targets all processes in that process group.

### Default Timeout

sgf uses a 200ms timeout. After a double-press confirmation, the user has already signaled clear intent to exit — there is no meaningful cleanup for an AI agent subprocess, so a short grace window (enough for buffer flushes) is sufficient before escalating to SIGKILL.

### Usage by sgf

sgf calls `kill_process_group` when its `ShutdownController` returns `Shutdown`:

```rust
fn kill_child(child: &std::process::Child) {
    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
}
```

This replaces the previous single-PID SIGTERM: `kill(pid, SIGTERM)`.
