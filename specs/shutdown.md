# shutdown Specification

Shared graceful shutdown library for Springfield CLI tools. Provides unified double-press Ctrl+C and Ctrl+D detection with confirmation prompts, used by both `sgf` and `ralph`.

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
| `libc` (0.2) | Low-level signal constants |

No async runtime. Uses `std::thread` for the stdin monitor and `std::sync::atomic` for cross-thread state.

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

## Stdin EOF Detection (Ctrl+D)

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

sgf creates a `ShutdownController` with `monitor_stdin: true` before spawning ralph. The polling loop in `run_ralph()` calls `controller.poll()` instead of manually checking atomic counters.

### All Modes

All modes use double-press semantics — AFK, non-AFK, and interactive:

- **AFK loops**: `poll()` is called in the existing 50ms polling loop. On `Shutdown`, sgf sends SIGTERM to the ralph child process.
- **Non-AFK loops**: Same as AFK. Replaces the current single-press immediate shutdown.
- **Interactive stages** (`spec`, `issues log`): sgf no longer ignores SIGINT entirely. Instead, it creates a `ShutdownController` and polls it while waiting for the child. First press is absorbed by the controller (child does NOT receive it since sgf registered the SIGINT handler). Double-press sends SIGTERM to the child.

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

The `run_afk()` polling loop calls `controller.poll()` instead of manually checking `sigint_count`. On `Shutdown`, ralph kills the agent child process and exits 130.

### Interactive Mode

Same as AFK — ralph polls the controller between iterations and during the agent run. On `Shutdown`, ralph exits 130.

### SIGTERM from sgf

When sgf sends SIGTERM, the controller's SIGTERM handler sets the flag, and `poll()` returns `Shutdown` immediately. Ralph kills the agent child and exits 130.

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

Stdin EOF tests require a PTY or pipe to simulate Ctrl+D, which is complex for unit tests. Stdin EOF behavior is covered by integration tests at the sgf/ralph level.

### Integration Tests (at sgf and ralph level)

Signal-based integration tests in `crates/springfield/tests/` and `crates/ralph/tests/`:

| Test | Description |
|------|-------------|
| `double_ctrl_c_exits_130` | Send two SIGINTs to sgf/ralph, verify exit code 130 |
| `single_ctrl_c_continues_after_timeout` | Send one SIGINT, wait 3 seconds, verify process continues |
| `sigterm_exits_immediately` | Send SIGTERM, verify immediate exit with code 130 |
| `confirmation_message_on_first_ctrl_c` | Send one SIGINT, verify stderr contains "Press Ctrl-C again to exit" |
