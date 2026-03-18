# shutdown Specification

Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts

| Field | Value |
|-------|-------|
| Src | `crates/shutdown/` |
| Status | draft |

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

| Test | Description |
|------|-------------|
| `sigterm_immediate_shutdown` | Register handler, raise SIGTERM, verify `poll()` returns `Shutdown` |
| `single_sigint_returns_pending` | Raise one SIGINT, verify `poll()` returns `Pending` |
| `double_sigint_returns_shutdown` | Raise two SIGINTs, verify `poll()` returns `Shutdown` |
| `sigint_resets_after_timeout` | Raise one SIGINT, sleep past timeout, verify `poll()` returns `Running` |
| `default_config` | Verify `ShutdownConfig::default()` has 2-second timeout and `monitor_stdin: true` |
| `poll_returns_running_initially` | Create controller, verify `poll()` returns `Running` |
| `kill_pg_sends_sigterm_to_group` | Spawn a child with `setsid()`, call `kill_process_group`, verify child exits (not SIGKILL — check exit signal) |
| `kill_pg_escalates_to_sigkill` | Spawn a child that traps SIGTERM (ignores it), call `kill_process_group` with short timeout, verify child is killed |
| `kill_pg_already_dead` | Spawn a child, wait for it to exit, call `kill_process_group`, verify returns `false` |
| `kill_pg_kills_descendants` | Spawn a child with `setsid()` that itself spawns a grandchild, call `kill_process_group`, verify both are dead |

Stdin EOF tests require a PTY or pipe to simulate Ctrl+D, which is complex for unit tests. Stdin EOF behavior is covered by integration tests at the sgf/ralph level.

### Integration Tests (at sgf and ralph level)

Signal-based integration tests in `crates/springfield/tests/` and `crates/ralph/tests/`:

| Test | Description |
|------|-------------|
| `double_ctrl_c_exits_130` | Send two SIGINTs to sgf/ralph, verify exit code 130 |
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
    /// Enable for the top-level process only. Disable when running
    /// under a parent that already monitors stdin.
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

## Stdin EOF Detection

A background thread reads stdin using blocking `read()`. In a Unix terminal, Ctrl+D causes `read()` to return 0 bytes (EOF) when the input buffer is empty. Each EOF event increments the `eof_count` atomic counter.

After an EOF, the thread continues reading — Ctrl+D in a terminal does not permanently close stdin. The next `read()` call blocks until the user provides more input or another EOF.

### Stdin Monitor Lifecycle

- Started by `ShutdownController::new()` when `config.monitor_stdin` is true
- The thread runs for the lifetime of the controller
- On `Drop`, the controller signals the thread to stop (via `AtomicBool`). The thread may remain blocked on `read()` — this is acceptable since the process is exiting.

### When to Disable Stdin Monitoring

Set `monitor_stdin: false` when:
- The process is managed by a parent that already monitors stdin (e.g., ralph under sgf)
- Stdin is not a terminal (piped input)

When ralph is launched by sgf, sgf sets `SGF_MANAGED=1` in ralph's environment. Ralph checks this to decide whether to enable stdin monitoring.

## Integration with sgf

sgf creates a `ShutdownController` before spawning ralph or `cl`. The controller configuration varies by mode — AFK mode owns stdin and terminal signals; non-AFK and interactive modes let the child own them.

### AFK Loops

`setsid()` isolates ralph in its own session. sgf creates the controller with `monitor_stdin: true` (stdin is free — no user interaction). The 50ms polling loop calls `controller.poll()`. Both double Ctrl+C and double Ctrl+D trigger shutdown. On `Shutdown`, sgf kills ralph's process group via `kill_process_group(pid, 200ms)` — SIGTERM to the group, escalating to SIGKILL after timeout (see [Process Group Kill with Escalation](#process-group-kill-with-escalation)).

**Stdin isolation**: sgf passes `Stdio::null()` for stdin when spawning ralph in AFK mode. This prevents the agent from inheriting the terminal fd and modifying terminal settings (e.g., disabling ISIG via `tcsetattr`). Without this, the agent can put the terminal in raw mode, causing Ctrl+C to emit byte `0x03` instead of generating SIGINT and Ctrl+D to emit byte `0x04` instead of triggering EOF. With `Stdio::null()`, the terminal fd stays under sgf's exclusive control and ISIG remains enabled.

### Non-AFK Loops (Interactive Ralph)

No `setsid()` — ralph stays in sgf's process group so it (and the agent) receive terminal signals naturally. sgf creates the controller with `monitor_stdin: false` — stdin belongs to the child for user interaction with Claude. Only double Ctrl+C works for shutdown (Ctrl+D goes to Claude as normal input). Both sgf and the child receive SIGINT; sgf's handler prints "Press Ctrl-C again to exit" while Claude handles the signal with its own logic.

### Interactive Stages (`spec`, `issues log`)

`monitor_stdin: false`, no `setsid()`. Same rationale as non-AFK loops — the user types directly into Claude.

### Environment Variable

sgf sets `SGF_MANAGED=1` in ralph's environment when spawning it. This tells ralph to disable its own stdin monitoring and rely on sgf for Ctrl+D detection. Ralph still registers its own SIGINT/SIGTERM handlers for graceful cleanup.

## Integration with ralph

ralph creates a `ShutdownController` whose `monitor_stdin` depends on whether it's running under sgf:

```rust
let config = ShutdownConfig {
    monitor_stdin: std::env::var("SGF_MANAGED").is_err(),
    ..Default::default()
};
let controller = ShutdownController::new(config)?;
```

### AFK Mode

The `run_afk()` polling loop calls `controller.poll()` instead of manually checking `sigint_count`. On `Shutdown`, ralph kills the agent's process group via `kill_process_group` and exits 130.

### Interactive Mode

Same as AFK — ralph polls the controller between iterations and during the agent run. On `Shutdown`, ralph exits 130.

### SIGTERM from sgf

When sgf sends SIGTERM, the controller's SIGTERM handler sets the flag, and `poll()` returns `Shutdown` immediately. Ralph kills the agent's process group via `kill_process_group` and exits 130.

## Process Group Kill with Escalation

The `kill_process_group` function provides graceful-then-forceful termination of a process group. Both `sgf` and `ralph` spawn children with `setsid()`, making each child a session leader where PID=PGID. Killing a single PID leaves descendants (agent tool subprocesses, build commands, etc.) orphaned and running. This function kills the entire process group.

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

`sgf` and `ralph` both use a 200ms timeout. After a double-press confirmation, the user has already signaled clear intent to exit — there is no meaningful cleanup for an AI agent subprocess, so a short grace window (enough for buffer flushes) is sufficient before escalating to SIGKILL.

### Usage by sgf

sgf calls `kill_process_group` when its `ShutdownController` returns `Shutdown`:

```rust
fn kill_child(child: &std::process::Child) {
    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
}
```

This replaces the previous single-PID SIGTERM: `kill(pid, SIGTERM)`.

### Usage by ralph

ralph calls `kill_process_group` in both `run_afk` and `run_interactive` when the shutdown controller triggers:

```rust
if controller.poll() == ShutdownStatus::Shutdown {
    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
    let _ = child.wait();
    return;
}
```

This replaces the previous `child.kill()` (which sent SIGKILL to a single PID).

## Related Specifications

- [ralph](ralph.md) — Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push
