# ralph Specification

CLI tool for iterative Claude Code execution. Invokes `$AGENT_CMD` directly (no Docker sandbox) with clean NDJSON stream formatting, sentinel file completion detection, and git auto-push.

## Overview

`ralph` provides:
- **Iteration loop**: Run Claude Code repeatedly against a prompt file or inline text, up to N iterations
- **Two modes**: Interactive (terminal passthrough) and AFK (formatted NDJSON stream)
- **Direct agent invocation**: Runs `$AGENT_CMD` as a child process — no Docker, no Mutagen, no sandbox lifecycle. `$AGENT_CMD` must be set; ralph fails immediately if the env var is missing or empty.
- **System prompt injection**: Read `PROMPT_FILES` env var and pass a `study @<file>` instruction via `--append-system-prompt` to Claude Code; optionally append spec files via `--spec`
- **NDJSON formatting**: Compact, readable output from Claude's stream-json format
- **Completion detection**: Exit early when Claude signals task completion by creating the `.ralph-complete` sentinel file
- **Interactive notification**: Play a sound on the host when Claude needs user input (via `.ralph-ding` sentinel file)
- **Flexible prompt input**: Accept either a file path or an inline text string as the prompt
- **Git auto-push**: Automatically push new commits after each iteration

## Design Goals

1. **Readable AFK output**: Styled, Claude Code-like terminal output — ANSI colors, tool call one-liners, truncated tool results, boxed banners
2. **Testable**: NDJSON formatting is a pure function; full binary testable via command override
3. **Minimal dependencies**: Only `clap`, `serde`, `serde_json` — no async runtime needed
4. **No implicit agent binary**: `$AGENT_CMD` is required. Never fall back to a hardcoded binary name.

## Architecture

```
ralph/
├── src/
│   ├── main.rs      # CLI, iteration loop, agent invocation, git operations, output rendering
│   ├── format.rs    # NDJSON parsing, tool call/result formatting (pure, no ANSI)
│   ├── style.rs     # ANSI escape code helpers (bold, dim, green, yellow, red), NO_COLOR support
│   └── banner.rs    # Box-drawing banner renderer (render_box)
├── tests/
│   ├── integration.rs           # Binary-level E2E tests with mock agent
│   └── fixtures/
│       ├── prompt.md            # Dummy prompt for tests
│       ├── afk-session.ndjson   # Fixture: normal AFK session with text + tool calls + tool results
│       └── complete.ndjson      # Fixture: session ending with completion promise
└── Cargo.toml
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (4, derive) | CLI argument parsing with env var support |
| `serde` (1, derive) | JSON deserialization |
| `serde_json` (1) | NDJSON line parsing |
| `signal-hook` (0.4) | SIGINT/SIGTERM handling for graceful shutdown |
| `libc` (0.2) | `setsid()` in pre_exec hook to detach child from controlling terminal |
| `tracing` (0.1) | Structured logging |
| `tracing-subscriber` (0.3, fmt + env-filter) | Log output formatting, `RUST_LOG` env filter |

Dev dependencies for integration tests:

| Crate | Purpose |
|-------|---------|
| `tempfile` (3) | Temporary directories for test isolation |

No async runtime. Process spawning and I/O use `std::process` and `std::io::BufRead`.

## CLI Interface

```
ralph [OPTIONS] [ITERATIONS] [PROMPT]
```

When invoked by `sgf`, the full command looks like:

```
SGF_SPEC=auth ralph [-a] [--loop-id ID] [--auto-push BOOL] [--max-iterations N] [--spec auth] ITERATIONS PROMPT
```

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `ITERATIONS` | u32 | `1` | Number of iterations to run |
| `PROMPT` | String | `prompt.md` | Prompt file path or inline text string |

### Prompt Resolution

The `PROMPT` argument accepts either a file path or an inline text string. Ralph uses a simple heuristic:

1. If the value is a path to an existing file → read the file and pass its contents to Claude (via `@` prefix)
2. If the value is not a path to an existing file → pass it directly as literal text to Claude

The default value `prompt.md` is treated specially: if no explicit prompt is provided and `prompt.md` does not exist, ralph exits with an error (code 1). This prevents accidentally sending the literal string "prompt.md" as a prompt.

### Flags and Options

| Flag/Option | Env Var | Default | Description |
|-------------|---------|---------|-------------|
| `-a`, `--afk` | — | `false` | Run in AFK mode (non-interactive) |
| `--loop-id` | — | — | Loop identifier (sgf-generated, included in banner output) |
| `--max-iterations` | `RALPH_MAX_ITERATIONS` | `100` | Safety limit for iterations |
| `--auto-push` | `RALPH_AUTO_PUSH` | `true` | Auto-push after new commits (requires explicit value: `true`/`false`/`yes`/`no`/`1`/`0`) |
| `--command` | `RALPH_COMMAND` | — | Override: path to executable replacing agent invocation (for testing) |
| `--spec` | `SGF_SPEC` | — | Spec stem — adds `./specs/<stem>.md` to the study instruction. Fails with error if the spec file does not exist. |
| `--prompt-file` | — | — | Additional prompt file path (repeatable). Added to the study instruction passed via `--append-system-prompt`. |
| — | `AGENT_CMD` | **(required)** | Path to the agent binary (e.g., `claude`). Ralph fails immediately with exit code 1 if this is unset or empty. |
| — | `PROMPT_FILES` | `$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md` | Colon-separated list of files to include in the study instruction |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Completion file `.ralph-complete` detected |
| `1` | Error (prompt file missing, etc.) |
| `2` | Iterations exhausted without completion |
| `130` | Interrupted by SIGINT (Ctrl+C) or SIGTERM |

### Examples

```bash
AGENT_CMD=claude ralph 10                              # Interactive, 10 iterations, prompt.md
AGENT_CMD=claude ralph -a 5                            # AFK mode, 5 iterations
AGENT_CMD=claude ralph 10 my-task.md                   # Custom prompt file
AGENT_CMD=claude ralph 5 "fix the login bug"           # Inline text prompt
AGENT_CMD=claude ralph -a 3 "refactor auth module"     # AFK mode with inline text
RALPH_AUTO_PUSH=false AGENT_CMD=claude ralph -a 10     # Disable auto-push
AGENT_CMD=claude ralph -a --loop-id build-auth-20260226T143000 10 prompt.md  # With loop ID
AGENT_CMD=claude ralph -a --spec auth 10 .sgf/prompts/build.md              # With spec
AGENT_CMD=claude ralph --prompt-file ./NOTES.md 5 prompt.md                  # Extra prompt file
```

## Agent Binary Resolution

Ralph requires the `AGENT_CMD` environment variable to be set to a non-empty value. This is the path or name of the agent binary (e.g., `claude`). Ralph **never** falls back to a hardcoded binary name — if `AGENT_CMD` is unset or empty, ralph prints an error and exits with code 1 immediately.

```
AGENT_CMD not set. Set AGENT_CMD to the path of the agent binary (e.g., AGENT_CMD=claude).
```

When `--command` is set (testing mode), `AGENT_CMD` is not required — the command override takes precedence.

## System Prompt Injection

Ralph owns system prompt injection for all automated stages. It collects prompt files from three sources, builds a semicolon-separated `study @<file>` instruction, and passes it as a single `--append-system-prompt` argument to the agent binary invocation. This ensures the agent actively reads and processes the files rather than receiving them as passive system context.

### Sources (in order)

1. **`PROMPT_FILES` env var** — Colon-separated list of files. Default: `$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md`. Path resolution: `~` and `$HOME` expand to the home directory; `./` paths resolve relative to cwd. Missing files emit a warning to stderr and are skipped. If `PROMPT_FILES` is not set, a warning is emitted and the default is used.
2. **`--spec <stem>`** — If provided, appends `./specs/<stem>.md`. Fails with exit code 1 and a clear error (e.g., `spec file not found: specs/auth.md`) if the file does not exist.
3. **`--prompt-file <path>`** (repeatable) — Additional explicit files. Missing files are a fatal error (exit code 1).

### Agent Invocation

The collected prompt files are combined into a single `--append-system-prompt` argument with `study @<file>` instructions, placed before the prompt argument in the agent invocation:

```
$AGENT_CMD \
  --verbose \
  --dangerously-skip-permissions \
  --settings '{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}' \
  --append-system-prompt 'study @$HOME/.MEMENTO.md;study @./BACKPRESSURE.md;study @./specs/README.md;study @./specs/auth.md' \
  @prompt.md
```

Since the agent runs directly on the host filesystem, all paths (including `$HOME/.MEMENTO.md`) are accessible without staging. The external file staging step is no longer needed.

When `--command` is set (testing mode), the same `--append-system-prompt` argument is passed to the mock command.

## Modes

### Interactive Mode (default)

Spawns the agent with full terminal passthrough (stdin/stdout/stderr inherited).

```
$AGENT_CMD \
  --verbose \
  --dangerously-skip-permissions \
  --settings '{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}' \
  [--append-system-prompt 'study @<FILE>;...']  # from PROMPT_FILES, --spec, --prompt-file
  @<PROMPT_FILE>       # file prompt (@ prefix)
  # or: "<inline text>"  # inline text (no @ prefix)
```

No output processing. The user interacts with the agent directly. After each iteration, ralph checks for the `.ralph-complete` sentinel file to detect task completion.

In interactive mode, ralph also runs a **notification watcher thread** that monitors for the `.ralph-ding` sentinel file. When detected, ralph plays a notification sound on the host machine to alert the user that Claude needs input. See [Interactive Notification](#interactive-notification).

### AFK Mode (`--afk`)

Spawns the agent with piped stdout, stderr inherited.

```
$AGENT_CMD \
  --verbose \
  --print \
  --output-format stream-json \
  --dangerously-skip-permissions \
  --settings '{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}' \
  [--append-system-prompt 'study @<FILE>;...']  # from PROMPT_FILES, --spec, --prompt-file
  @<PROMPT_FILE>       # file prompt (@ prefix)
  # or: "<inline text>"  # inline text (no @ prefix)
```

Stdout is read line-by-line via `BufRead`, parsed as NDJSON, and formatted with ANSI-styled output. Lines not starting with `{` are skipped silently (handles verbose debug output). Each output line is prefixed with `\r\x1b[2K` (carriage return + ANSI clear-line). This prefix is applied per line (not per block) because text content from the agent contains embedded newlines.

The `TeeWriter` writes styled output (with ANSI codes) to stdout and stripped output (ANSI codes removed) to the log file. ANSI stripping uses a simple regex: `\x1b\[[0-9;]*m`.

After the process exits, ralph checks for the `.ralph-complete` sentinel file to determine if the task is complete.

## Signal Handling

Ralph uses the shared `shutdown` crate's `ShutdownController` (see [shutdown spec](shutdown.md)) for graceful shutdown detection. The controller handles both Ctrl+C (SIGINT) and Ctrl+D (stdin EOF) as shutdown triggers, requiring a double-press of the same key within 2 seconds. SIGTERM always triggers immediate shutdown.

### Standalone vs Managed

When ralph runs standalone (no parent sgf), it creates a `ShutdownController` with `monitor_stdin: true` — both Ctrl+C and Ctrl+D work for shutdown.

When ralph runs under sgf, sgf sets `SGF_MANAGED=1` in ralph's environment. Ralph creates the controller with `monitor_stdin: false` — it handles SIGINT directly but relies on sgf for Ctrl+D detection. sgf sends SIGTERM to ralph when the user confirms shutdown, and SIGTERM triggers immediate exit.

```rust
let config = ShutdownConfig {
    monitor_stdin: std::env::var("SGF_MANAGED").is_err(),
    ..Default::default()
};
```

### Double-Press Behavior

All modes (AFK and interactive) use double-press semantics:

1. First Ctrl+C (or Ctrl+D): the controller prints "Press Ctrl-C again to exit" (or "Press Ctrl-D again to exit") to stderr. Starts a 2-second confirmation window.
2. Second press of the **same key** within 2 seconds: kills the agent child process, exits with code 130.
3. **Timeout**: If no second press of the same key arrives within 2 seconds, the counter resets and the loop continues.
4. **SIGTERM**: Immediate shutdown — kills child, exits 130.

Ctrl+C and Ctrl+D are independent channels. Pressing Ctrl+C then Ctrl+D (or vice versa) does NOT trigger shutdown.

The between-iteration gap (2-second sleep) and the post-agent check both poll the controller.

### Session Isolation (AFK mode)

In AFK mode, `setsid()` in `pre_exec` creates a new session, detaching the agent child process from ralph's session. This prevents the child from becoming the foreground process group and stealing SIGINT delivery. With `setsid`, the agent is in a different session and `tcsetpgrp()` on ralph's terminal fails.

No PTY pair is needed since the agent runs directly (no Docker wrapper that forces raw mode on stdin). AFK mode uses `Stdio::piped()` for stdout, `Stdio::null()` for stdin, and `Stdio::inherit()` for stderr.

**Stdin isolation in AFK mode**: The agent child receives `/dev/null` as stdin. This serves two purposes: (1) the agent has no terminal fd to call `tcsetattr` on, so it cannot disable ISIG or put the terminal in raw mode — Ctrl+C continues to generate SIGINT and Ctrl+D continues to trigger EOF for the parent process; (2) the agent has no stdin to read from, which is correct for AFK mode where no user interaction occurs. When ralph runs under sgf, sgf also passes `Stdio::null()` to ralph for the same reason.

### Terminal Settings Preservation

The agent (Claude Code) may modify terminal settings via inherited file descriptors (e.g., calling `tcsetattr()` on the stderr fd to enable raw mode). This can disable `ISIG`, causing Ctrl+C to send byte `0x03` instead of generating SIGINT, and Ctrl+D to send byte `0x04` instead of triggering EOF. Both AFK and interactive modes are affected.

Ralph saves terminal settings (`tcgetattr` on stdin fd) before spawning the agent and restores them (`tcsetattr`) after the agent exits. This ensures the terminal is always in a known-good state for signal handling between iterations and after the agent run.

```rust
fn save_terminal_settings() -> Option<libc::termios> {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
            Some(termios)
        } else {
            None
        }
    }
}

fn restore_terminal_settings(termios: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, termios);
    }
}
```

The save happens once before the main loop. The restore happens after each agent invocation (both `run_afk` and `run_interactive`). If stdin is not a terminal (e.g., piped input), `tcgetattr` fails and no save/restore occurs — this is expected and harmless.

### Stdout Reading and Interrupt Polling

Stdout is read on a dedicated thread that sends lines through an `mpsc` channel. The main thread uses `recv_timeout` (100ms) to poll the channel, checking both the `interrupted` flag and `sigint_count` between receives. When the abort condition is met (double Ctrl+C in AFK, or single SIGTERM):

1. The child process is killed via `child.kill()`
2. `child.wait()` reaps the process
3. Control returns to the main loop, which detects the flag and exits with code 130

The 2-second sleep between iterations is also interruptible (polled in 100ms increments), using single-press semantics.

In interactive mode, SIGINT is delivered to the entire foreground process group. The agent child receives it directly (stdin is inherited), handles it, and eventually exits. Ralph's `.status()` call returns, the flag is checked, and ralph exits with code 130.

## Interactive Notification

In interactive mode, ralph plays a notification sound when Claude finishes its turn and needs user input.

### Mechanism

1. **Claude Code hooks** (configured in project-level `.claude/settings.local.json`) run `touch .ralph-ding` on `Notification` and `Stop` events
2. **Ralph's watcher thread** polls for `.ralph-ding` every ~100ms
3. On detection: delete the file, spawn `afplay /System/Library/Sounds/Blow.aiff` in the background (non-blocking)

Since the agent runs directly on the host, the hooks' `afplay` command also works directly — the `touch .ralph-ding` is a belt-and-suspenders mechanism for environments where `afplay` is not available.

### Hooks Configuration

The following hooks must be present in `.claude/settings.local.json` (project-level, one-time manual setup):

```json
{
  "permissions": { ... },
  "hooks": {
    "Notification": [
      {
        "hooks": [
          { "type": "command", "command": "afplay /System/Library/Sounds/Blow.aiff" },
          { "type": "command", "command": "touch .ralph-ding" }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "afplay /System/Library/Sounds/Blow.aiff" },
          { "type": "command", "command": "touch .ralph-ding" }
        ]
      }
    ]
  }
}
```

### Watcher Thread

The watcher runs only in interactive mode (AFK mode has no user interaction). It is a background thread that:

1. Polls `Path::new(".ralph-ding").exists()` every ~100ms
2. On detection: `fs::remove_file(".ralph-ding")`, then `Command::new("afplay").arg("/System/Library/Sounds/Blow.aiff").spawn()` (fire-and-forget)
3. Continues polling until signaled to stop (via `AtomicBool`)

The watcher thread is started before spawning the agent process and stopped after it exits. Stale `.ralph-ding` files are cleaned up at ralph startup alongside `.ralph-complete`.

### Gitignore

`.ralph-ding` must be listed in `.gitignore` to prevent accidental commits of the sentinel file.

## NDJSON Stream Formatting

### Stream Event Types

Claude's `--output-format stream-json` emits newline-delimited JSON. Five top-level event types exist:

| Type | Handling |
|------|----------|
| `assistant` | Parsed and formatted — contains model text and tool calls |
| `result` | Parsed and formatted — final result text, may contain `session_id` and `usage` fields |
| `user` | Parsed — may contain tool results (as `tool_result` content blocks). If present, displayed as truncated output beneath the corresponding tool call |
| `system` | Logged at debug level, otherwise ignored |
| Unknown | Logged at debug level via `#[serde(other)]`, otherwise ignored |

Previously, only `assistant` and `result` were handled; all others were silently dropped. Now `user` events are parsed for tool results and unknown events are logged for discoverability.

### Serde Types

```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Assistant { message: AssistantMessage },
    Result {
        result: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
    },
    User { message: UserMessage },
    System {},
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct UserMessage {
    content: Vec<UserContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text { text: String },
    ToolUse { name: String, input: serde_json::Value },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserContentBlock {
    ToolResult {
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}
```

### ANSI Styling

All AFK output uses ANSI escape codes for terminal styling with color differentiation. The `style` module provides helpers: `bold(s)`, `dim(s)`, `green(s)`, `yellow(s)`, `red(s)`, `blue(s)`, `magenta(s)`, `cyan(s)`. A `NO_COLOR` environment variable check disables all styling (per the [NO_COLOR convention](https://no-color.org/)), falling back to unstyled output. The `TeeWriter` strips ANSI codes before writing to the log file.

#### Color Scheme

Designed for GitHub Dark terminal theme. Uses standard ANSI color codes for broad terminal compatibility.

#### Tool Name Colors (by category)

Tool names are bold + colored based on operation type:

| Category | Tools | Color | Rationale |
|----------|-------|-------|-----------|
| Read ops | `Read`, `Glob`, `Grep` | Bold blue | Non-mutating, information gathering |
| Mutations | `Edit`, `Write` | Bold magenta | File modifications stand out |
| Shell | `Bash` | Bold yellow | Commands warrant attention |
| Other | Everything else | Bold cyan | Default for unknown/misc tools |

#### Full Element Styling

| Element | Style |
|---------|-------|
| Agent text (model reasoning) | Bold |
| Tool separator `─` | Dim |
| Tool name | Bold + category color (see above) |
| Tool detail (path/command/pattern) | Cyan |
| Tool result lines (normal) | Dim |
| Tool result lines (error) | Red (not dim) |
| Tool result lines matching `\.\.\. ok$` | Dim green |
| Tool result lines matching `\.\.\. FAILED$` | Red |
| Truncation indicator `... (N more lines)` | Dim |
| Box borders (`╭╮│╰╯─`) | Dim |
| Box title text | Bold |
| Completion banner title | Bold green |
| Max-iterations banner title | Bold yellow |
| Usage stats | Dim |
| "Iteration N complete, continuing..." | Dim |
| "New commits detected, pushing..." | Dim |

#### Test Result Highlighting

Tool result lines from test runners get special treatment. Lines ending in ` ... ok` are styled dim green. Lines ending in ` ... FAILED` are styled red. This applies only to tool result lines (not agent text). The matching is simple suffix-based — no regex required.

### Box Banner Formatting

A new `banner` module renders boxed banners with a title embedded in the top border:

```
╭─ Title Text Here ───────────────────╮
│  Key:    Value                      │
│  Key2:   Value2                     │
╰─────────────────────────────────────╯
```

The box width is computed from the longest content line (minimum 40 characters). Content lines are right-padded with spaces to align the right `│` border. The top border embeds the title after `╭─ ` and fills the remainder with `─` before `╮`. Border characters are dim; title text is bold.

```rust
pub fn render_box(title: &str, lines: &[String]) -> String;
```

### Startup Banner

The Ralph ASCII art is printed as-is (unstyled). The config block beneath it uses `render_box`:

```
[Ralph ASCII art]

╭─ Ralph Loop Starting ──────────────────────────╮
│  Mode:        AFK                              │
│  Prompt:      prompt.md (file)                 │
│  Iterations:  10                               │
│  Agent:       claude                           │
│  Loop ID:     build-auth-20260311T133300       │
│  Prompt files:                                 │
│    - $HOME/.MEMENTO.md                         │
│    - ./BACKPRESSURE.md                         │
│    - ./specs/README.md                         │
╰────────────────────────────────────────────────╯
```

### Iteration Banner

Each iteration uses `render_box` with the iteration/loop-id as the title:

```
╭─ Iteration 1 of 10 [build-auth-20260311T133300] ─╮
╰───────────────────────────────────────────────────╯
```

When there is no loop ID: `╭─ Iteration 1 of 10 ─╮`. The iteration banner has no body lines — title only.

### Text Block Formatting

Text blocks are printed bold, preserving Claude's reasoning output with original newlines. This visually distinguishes agent reasoning from tool output.

### Tool Call Formatting

Tool calls are formatted as styled one-liners:

```
  ─ Read  specs/README.md
  ─ Bash  git status
  ─ Edit  src/main.rs
```

The format is: 2-space indent, dim `─`, space, bold+colored tool name (color by category), 2 spaces, cyan detail.

| Tool | Detail shown |
|------|-------------|
| `Read` | `file_path` (+ `offset:limit` if present) |
| `Edit` | `file_path` |
| `Write` | `file_path` |
| `Bash` | `command` truncated to 100 chars |
| `Glob` | `pattern` |
| `Grep` | `pattern` |
| `TodoWrite` | item count from `todos` array |
| Other | first string value, truncated to 80 chars |

Truncated values end with `...`. Truncation respects UTF-8 character boundaries.

### Tool Result Formatting

When a `user` event contains `tool_result` content blocks, each result is displayed beneath the preceding tool call line(s), indented and dimmed:

```
  ─ Read  specs/README.md
     1│ # Springfield Specifications
     2│
     3│ | Spec | Code | Purpose |
     ...

  ─ Bash  cargo test -p ralph
     running 12 tests
     test format::tests::text_block_passthrough ... ok
     test format::tests::read_tool_basic ... ok
     ...
```

Tool results are truncated to a maximum of **15 lines**. If the result exceeds 15 lines, the output is truncated and a dim `... (N more lines)` indicator is appended.

For error results (`is_error: true`), the result text is styled in dim red instead of dim.

If `user` events turn out not to contain tool results in practice (the stream-json format is under-documented), this feature degrades gracefully — tool calls are shown without results, identical to the old behavior but with new styling.

### Result Event Formatting

The `result` event's text is printed with default styling. If the `result` event contains `usage` fields (`input_tokens`, `output_tokens`), a usage summary line is printed after the result text:

```
  Input: 12,450 tokens · Output: 1,230 tokens
```

Styled dim. If no usage data is present, this line is omitted.

### Completion and Max-Iterations Banners

```
╭─ Ralph COMPLETE after 3 iterations! ──────╮
╰───────────────────────────────────────────╯
```

Title styled bold green. No body lines.

```
╭─ Ralph reached max iterations (10) ──────╮
╰───────────────────────────────────────────╯
```

Title styled bold yellow. No body lines.

### Public Formatting API

The `format_line` function signature changes to return structured output:

```rust
pub enum FormattedOutput {
    Text(String),
    ToolCalls(Vec<String>),
    ToolResults(Vec<FormattedToolResult>),
    Usage { input_tokens: u64, output_tokens: u64 },
    Result(String),
    Skip,
}

pub struct FormattedToolResult {
    pub lines: Vec<String>,
    pub is_error: bool,
    pub truncated_count: usize,
}

pub fn format_line(line: &str) -> FormattedOutput;
```

The caller (`run_afk`) applies ANSI styling and writes to `TeeWriter`. This keeps formatting logic pure and testable — styling is applied at the output boundary. The `TeeWriter` strips ANSI codes before writing to the log file.

Completion detection is handled separately via the `.ralph-complete` sentinel file, not by inspecting stream output.

## Main Loop

Before the loop:
- Resolve `AGENT_CMD` from environment. If unset or empty and `--command` is not set, exit 1 with error.
- Search for and delete any stale `.ralph-complete` sentinel file (from a previous crashed/killed run), searching recursively up to depth 2
- Delete stale `.ralph-ding` sentinel file if present

Prompt resolution (before the loop):
- If no explicit prompt provided and `prompt.md` does not exist → exit 1 with error
- If explicit prompt provided and it is a path to an existing file → use as file prompt (`@` prefix)
- If explicit prompt provided and it is not an existing file → use as inline text (no `@` prefix)

The startup banner includes mode, prompt source, iteration count, agent binary path, loop ID (if provided via `--loop-id`), and a list of all collected prompt files (each on its own line, prefixed with `  - `).

For each iteration `i` in `1..=iterations`:

1. Remove any stale `.ralph-complete` sentinel
2. Print iteration banner (includes loop ID if provided)
3. Record `git rev-parse HEAD` as `head_before`
4. Execute agent via `$AGENT_CMD` (or `--command` override):
   - Interactive: start notification watcher thread, `.status()` with inherited stdio, stop watcher thread
   - AFK: `.spawn()` with piped stdout, read lines via reader thread + channel through `format_line()`
5. If interrupted: log warning, exit 130
6. Search for `.ralph-complete` recursively (depth <= 2): if found, delete it, print completion banner, auto-push, exit 0
7. Print "Iteration N complete, continuing..."
8. Sleep 2 seconds (interruptible, polled in 100ms increments)
9. If interrupted: log warning, exit 130
10. If `--auto-push` and HEAD changed: run `git push`

After loop: search for and delete sentinel files (depth <= 2), print max iterations banner, exit 2.

## Git Auto-Push

After each iteration (when `--auto-push` is true):

1. Compare `git rev-parse HEAD` before and after the iteration
2. If HEAD changed, run `git push`
3. If push fails, print warning to stderr and continue

`git_head()` returns `Option<String>` — failures (not a git repo, etc.) silently return `None` and skip the push check.

## Completion Detection

After each iteration (in both interactive and AFK modes), ralph searches for a `.ralph-complete` sentinel file starting from the working directory and recursively scanning subdirectories up to a maximum depth of 2. The first match found is used. If a sentinel file is found, ralph prints a completion banner, deletes the found file, and exits with code 0.

The sentinel file is created by Claude (via `Bash(touch .ralph-complete)` or `Write(.ralph-complete)`) when the task is complete. This out-of-band mechanism avoids false positives from the model reproducing a completion string during reasoning. The recursive search ensures detection works regardless of whether Claude creates the file in the project root or in a subdirectory (e.g., within a crate or nested project directory).

Ralph deletes any stale `.ralph-complete` sentinel file (found via the same recursive search) at the start of the run and at all exit paths. The file is gitignored.

If all iterations complete without the sentinel file appearing, ralph exits with code 2.

### Sentinel Search Details

- **Search direction:** Downward from the working directory into subdirectories
- **Maximum depth:** 2 (working directory = depth 0, immediate children = depth 1, grandchildren = depth 2)
- **Match behavior:** Returns the first `.ralph-complete` file found; does not search exhaustively for all matches
- **Cleanup:** The found file path is deleted (not a hardcoded path relative to cwd)

## Command Override (`--command`)

When `--command` (or `RALPH_COMMAND`) is set, ralph runs the specified executable instead of `$AGENT_CMD`. The override executable is invoked with the same arguments and must write NDJSON (AFK mode) or interactive output to stdout.

This enables integration testing without a real agent binary. Tests create a mock script that emits fixture NDJSON. For completion detection tests, the mock script also creates the `.ralph-complete` sentinel file. When `--command` is set, `AGENT_CMD` is not required.

## Error Handling

No custom error types. Fail loudly, continue when sensible:

| Scenario | Behavior |
|----------|----------|
| `AGENT_CMD` not set (no `--command`) | `tracing::error!` + exit 1 (before loop starts) |
| Default prompt file missing | `tracing::error!` + exit 1 (before loop starts, only when no explicit prompt provided) |
| Spec file missing (`--spec`) | `tracing::error!` + exit 1 (e.g., `spec file not found: specs/auth.md`) |
| Prompt file missing (`--prompt-file`) | `tracing::error!` + exit 1 |
| `PROMPT_FILES` entry missing | `tracing::warn!` to stderr, skip the file (non-fatal) |
| Agent/command spawn failure | `tracing::warn!`, continue to next iteration |
| NDJSON parse error (line starts with `{`) | Skip line, log at debug level |
| Non-JSON line (no `{` prefix) | Skip line silently (expected verbose debug output) |
| stdout read error | `tracing::warn!`, continue reading |
| Git `rev-parse` failure | Return `None`, skip push check |
| Git push failure | `tracing::warn!`, continue |
| SIGINT/Ctrl+D received (all modes) | First press: print "Press Ctrl-C again to exit" (or "Press Ctrl-D again to exit") to stderr, start 2s timeout. Second press of same key: kill child, `tracing::warn!`, exit 130. Timeout: reset counter, continue. |
| SIGTERM received | Kill child process, `tracing::warn!`, exit 130 (immediate, single signal) |

## Testing

### Unit Tests

#### `format.rs`

The `format_line()` function is a pure function returning `FormattedOutput`. Unit tests cover:

- Text block passthrough → `FormattedOutput::Text`
- Each tool type formatting (Read, Edit, Write, Bash, Glob, Grep, TodoWrite, fallback) → `FormattedOutput::ToolCalls`
- Read with offset/limit variants
- Bash command truncation
- UTF-8 safe truncation
- Result event → `FormattedOutput::Result`
- Result event with usage → `FormattedOutput::Usage`
- User event with tool results → `FormattedOutput::ToolResults`
- User event with error tool result → `FormattedToolResult { is_error: true }`
- Tool result truncation at 15 lines → `truncated_count > 0`
- System event → `FormattedOutput::Skip`
- Non-JSON lines → `FormattedOutput::Skip`
- Unknown event types → `FormattedOutput::Skip`
- Malformed JSON → `FormattedOutput::Skip`

#### `style.rs`

- `bold()`, `dim()`, `green()`, `yellow()`, `red()`, `blue()`, `magenta()`, `cyan()` produce correct ANSI sequences
- `tool_name_style(name)` returns the correct bold+color combo for each tool category
- `NO_COLOR=1` disables all styling (returns input unchanged)

#### `banner.rs`

- `render_box()` with title and body lines produces correct box-drawing output
- Right borders align (all lines have same width)
- Title-only box (no body lines) renders correctly
- Box width respects minimum of 40 characters
- Long title wraps the box to fit

### Integration Tests (`tests/integration.rs`)

Binary-level E2E tests using `cargo test -p ralph`. Each test:

1. Creates a `tempfile::TempDir` with a dummy `prompt.md`
2. Initializes a git repo in the temp directory
3. Creates a mock script that emits fixture NDJSON
4. Runs the `ralph` binary via `std::process::Command` with:
   - `RALPH_COMMAND` set to the mock script path (bypasses `AGENT_CMD` requirement)
   - `RALPH_AUTO_PUSH=false` (no remote to push to)
   - Working directory set to the temp directory
   - `NO_COLOR=1` for tests that assert on text content (avoids ANSI in assertions)
5. Asserts on exit code and stdout content

#### Test Cases

| Test | Fixture | Asserts |
|------|---------|---------|
| AFK formats text blocks | `afk-session.ndjson` | stdout contains Claude's text verbatim |
| AFK formats tool calls as styled one-liners | `afk-session.ndjson` | stdout contains `─ Read` format, no raw JSON args |
| AFK shows tool results | `afk-session.ndjson` | stdout contains truncated tool result output (if `user` events present in fixture) |
| AFK detects completion file | `complete.ndjson` + sentinel file | exit code 0, sentinel cleaned up |
| AFK exhausts iterations without completion | `afk-session.ndjson` | exit code 2 |
| AFK startup banner uses box format | `afk-session.ndjson` | stdout contains `╭─ Ralph Loop Starting` |
| AFK iteration banner uses box format | `afk-session.ndjson` | stdout contains `╭─ Iteration 1 of` |
| AFK completion banner uses box format | `complete.ndjson` + sentinel | stdout contains `╭─ Ralph COMPLETE` |
| AFK max-iterations banner uses box format | `afk-session.ndjson` | stdout contains `╭─ Ralph reached max iterations` |
| Missing prompt file | — | exit code 1, stderr contains error message |
| Iterations clamped to max | `afk-session.ndjson` | stdout contains "Warning: Reducing iterations" |
| Help flag | — | exit code 0, stdout contains usage info |
| Bash command truncation | `afk-session.ndjson` | long commands end with `...` |
| AFK double Ctrl+C aborts | `afk-session.ndjson` + two SIGINTs | exit code 130, stdout contains "Press Ctrl+C again to stop" |
| AFK single Ctrl+C resets after timeout | `afk-session.ndjson` + one SIGINT | exit code 2 (iterations exhaust), loop continues after timeout |
| ANSI output disabled with NO_COLOR | `afk-session.ndjson` + `NO_COLOR=1` | stdout contains no ANSI escape sequences |

### NDJSON Fixtures

Fixtures are derived from real AFK output captured in [`ralph/tests/fixtures/ralph-sample-output.txt`](../ralph/tests/fixtures/ralph-sample-output.txt) (9 iterations of `scripts/ralph.sh --afk 10`).

`ralph/tests/fixtures/afk-session.ndjson` — updated to include `user` events with tool results. Covers:
- Text blocks (Claude's reasoning)
- Parallel tool calls (multiple content blocks per event)
- User events with `tool_result` content blocks (truncatable output)
- User events with error tool results (`is_error: true`)
- Read with and without `offset`/`limit`
- Edit with `old_string`/`new_string` content (must not appear in formatted output)
- Bash with short and long commands
- TodoWrite with `todos` array
- Grep and Glob tool calls
- Result event (with optional `usage` fields if stream provides them)

`ralph/tests/fixtures/complete.ndjson` — modeled on iteration 9 of sample output. Covers:
- Short session ending with a result event (sentinel file creation is handled by the mock script, not the NDJSON fixture)

### Expected Formatted Output

For `afk-session.ndjson`, the formatter should produce output like (ANSI styling indicated in brackets, not literal):

```
[bold]I'll start by studying the required files to understand the context and plan.[/bold]

  [dim]─[/dim] [bold][blue]Read[/blue][/bold]  [cyan]specs/README.md[/cyan]
     [dim]1│ # Springfield Specifications[/dim]
     [dim]2│ [/dim]
     [dim]3│ | Spec | Code | Purpose |[/dim]
     [dim]...[/dim]

  [dim]─[/dim] [bold][blue]Read[/blue][/bold]  [cyan]plans/cleanup/buddy-llm.md[/cyan]
     [dim]1│ # Cleanup Plan[/dim]
     [dim]...[/dim]

[bold]Now I can see the cleanup plan. Many items are checked off...[/bold]

  [dim]─[/dim] [bold][cyan]TodoWrite[/cyan][/bold]  [cyan]3 items[/cyan]

[bold]Let me read the relevant files in parallel...[/bold]

  [dim]─[/dim] [bold][blue]Read[/blue][/bold]  [cyan]specs/tokenizer-embedding.md[/cyan]
  [dim]─[/dim] [bold][blue]Read[/blue][/bold]  [cyan]crates/buddy-llm/src/inference.rs 1:80[/cyan]
  [dim]─[/dim] [bold][blue]Read[/blue][/bold]  [cyan]specs/buddy-llm.md[/cyan]

[bold]Now I have full context...[/bold]

  [dim]─[/dim] [bold][magenta]Edit[/magenta][/bold]  [cyan]specs/tokenizer-embedding.md[/cyan]
     [dim]✓ Applied edit[/dim]

  [dim]─[/dim] [bold][yellow]Bash[/yellow][/bold]  [cyan]git diff specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md[/cyan]
     [dim]diff --git a/specs/tokenizer-embedding.md b/specs/tokenizer-embedding.md[/dim]
     [dim]...[/dim]

  [dim]─[/dim] [bold][yellow]Bash[/yellow][/bold]  [cyan]cargo test -p ralph[/cyan]
     [dim]running 12 tests[/dim]
     [dim][green]test format::tests::text_block_passthrough ... ok[/green][/dim]
     [dim][green]test format::tests::read_tool_basic ... ok[/green][/dim]
     [dim]...[/dim]

  [dim]─[/dim] [bold][yellow]Bash[/yellow][/bold]  [cyan]git add ... && git commit ...[/cyan]
     [dim][master 170c9b2] Replace mistral.rs code snippet[/dim]
     [dim] 1 file changed, 4 insertions(+), 8 deletions(-)[/dim]

[bold]Done. Updated `specs/tokenizer-embedding.md`.[/bold]

  [dim]Input: 12,450 tokens · Output: 1,230 tokens[/dim]
```

## Related Specifications

- [springfield](springfield.md) — CLI entry point that orchestrates ralph
- [pensa](pensa.md) — Agent persistent memory, used by the agent inside ralph iterations
