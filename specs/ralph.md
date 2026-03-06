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

1. **Readable AFK output**: Tool calls shown as compact one-liners, not raw JSON argument dumps
2. **Testable**: NDJSON formatting is a pure function; full binary testable via command override
3. **Minimal dependencies**: Only `clap`, `serde`, `serde_json` — no async runtime needed
4. **No implicit agent binary**: `$AGENT_CMD` is required. Never fall back to a hardcoded binary name.

## Architecture

```
ralph/
├── src/
│   ├── main.rs      # CLI, startup banner (mode, prompt, iterations, loop-id), iteration loop, agent invocation, git operations
│   └── format.rs    # NDJSON parsing, tool call formatting, completion detection
├── tests/
│   ├── integration.rs           # Binary-level E2E tests with mock agent
│   └── fixtures/
│       ├── prompt.md            # Dummy prompt for tests
│       ├── afk-session.ndjson   # Fixture: normal AFK session with text + tool calls
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

Stdout is read line-by-line via `BufRead`, parsed as NDJSON, and formatted for human readability. Lines not starting with `{` are skipped silently (handles verbose debug output). Each output line is prefixed with `\r\x1b[2K` (carriage return + ANSI clear-line). This prefix is applied per line (not per block) because text content from the agent contains embedded newlines. After the process exits, ralph checks for the `.ralph-complete` sentinel file to determine if the task is complete.

## Signal Handling

Ralph registers signal handlers at startup using `signal-hook`:

- **SIGTERM**: Sets an `AtomicBool` (`interrupted`) flag. A single SIGTERM always triggers immediate shutdown.
- **SIGINT (Ctrl+C)**: Increments an `AtomicUsize` counter (`sigint_count`) via `signal_hook::flag::register_usize`. Behavior depends on mode:
  - **Interactive mode**: A single SIGINT triggers immediate shutdown (same as SIGTERM). The counter is checked against `>= 1`.
  - **AFK mode**: Requires **two presses** within a timeout window (see [Double Ctrl+C in AFK Mode](#double-ctrlc-in-afk-mode)).

The between-iteration gap (2-second sleep) and the post-`run_afk` check both use single-press semantics: `sigint_count >= 1 || interrupted`.

### Double Ctrl+C in AFK Mode

In AFK mode, ralph requires two Ctrl+C presses to abort, similar to Claude Code's behavior. This prevents accidental termination of long-running unattended loops.

**Mechanism:**

1. First Ctrl+C increments `sigint_count` to 1. The `run_afk` polling loop detects `sigint_count == 1` and:
   - Prints `"\nPress Ctrl+C again to stop\n"` to stdout (not to the log file)
   - Records the current time as the start of the confirmation window
2. Second Ctrl+C increments `sigint_count` to 2. The polling loop detects `sigint_count >= 2` and:
   - Kills the child process via `child.kill()`
   - Calls `child.wait()` to reap the process
   - Returns to the main loop, which detects the signal and exits with code 130
3. **Timeout**: If no second press arrives within **2 seconds**, the counter is reset to 0 (via `store(0, Ordering::Relaxed)`) and the confirmation window message is cleared. The loop continues running normally.

**Why stdout-only for the message:** The "press again" prompt is an interactive terminal cue, not an application event. It should not appear in `--log-file` output or structured logs.

### Session Isolation (AFK mode)

In AFK mode, `setsid()` in `pre_exec` creates a new session, detaching the agent child process from ralph's session. This prevents the child from becoming the foreground process group and stealing SIGINT delivery. With `setsid`, the agent is in a different session and `tcsetpgrp()` on ralph's terminal fails.

No PTY pair is needed since the agent runs directly (no Docker wrapper that forces raw mode on stdin). AFK mode uses `Stdio::piped()` for stdout and `Stdio::inherit()` for stderr.

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

Claude's `--output-format stream-json` emits newline-delimited JSON. Two event types are handled:

```json
{"type": "assistant", "message": {"content": [...]}}
{"type": "result", "result": "final output text"}
```

Content blocks within `assistant` messages:

```json
{"type": "text", "text": "Claude's reasoning..."}
{"type": "tool_use", "name": "Read", "input": {"file_path": "/foo/bar.rs"}}
```

All other event types are silently ignored via `#[serde(other)]`.

### Serde Types

```rust
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Assistant { message: AssistantMessage },
    Result { result: String },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text { text: String },
    ToolUse { name: String, input: serde_json::Value },
    #[serde(other)]
    Unknown,
}
```

### Text Block Formatting

Text blocks are printed verbatim, preserving Claude's reasoning output with original newlines.

### Tool Call Formatting

Tool calls are formatted as compact one-liners showing only the most relevant argument:

| Tool | Shows | Example Output |
|------|-------|----------------|
| `Read` | `file_path` (+ `offset:limit` if present) | `-> Read(src/main.rs)` or `-> Read(src/main.rs 430:80)` |
| `Edit` | `file_path` | `-> Edit(src/main.rs)` |
| `Write` | `file_path` | `-> Write(src/new.rs)` |
| `Bash` | `command` truncated to 100 chars | `-> Bash(git status)` |
| `Glob` | `pattern` | `-> Glob(**/*.rs)` |
| `Grep` | `pattern` | `-> Grep(TODO)` |
| `TodoWrite` | item count from `todos` array | `-> TodoWrite(3 items)` |
| Other | first string value, truncated to 80 chars | `-> WebSearch(rust serde json...)` |

Truncated values end with `...`. Truncation respects UTF-8 character boundaries.

### Public Formatting API

```rust
pub fn format_line(line: &str) -> Option<String>;
```

Returns formatted text to print, or `None` if the line should be skipped. Completion detection is handled separately via the `.ralph-complete` sentinel file, not by inspecting stream output.

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
| SIGINT received (AFK mode) | First press: print "Press Ctrl+C again to stop" to stdout, start 2s timeout. Second press: kill child, `tracing::warn!`, exit 130. Timeout: reset counter, continue. |
| SIGINT received (interactive / between iterations) | Kill child process, `tracing::warn!`, exit 130 |
| SIGTERM received | Kill child process (AFK), `tracing::warn!`, exit 130 (immediate, single signal) |

## Testing

### Unit Tests (`format.rs`)

The `format_line()` function is a pure function. Unit tests cover:

- Text block passthrough
- Each tool type formatting (Read, Edit, Write, Bash, Glob, Grep, TodoWrite, fallback)
- Read with offset/limit variants
- Bash command truncation
- UTF-8 safe truncation
- Result event output
- Non-JSON lines are skipped
- Unknown event types are skipped
- Malformed JSON is skipped

### Integration Tests (`tests/integration.rs`)

Binary-level E2E tests using `cargo test -p ralph`. Each test:

1. Creates a `tempfile::TempDir` with a dummy `prompt.md`
2. Initializes a git repo in the temp directory
3. Creates a mock script that emits fixture NDJSON
4. Runs the `ralph` binary via `std::process::Command` with:
   - `RALPH_COMMAND` set to the mock script path (bypasses `AGENT_CMD` requirement)
   - `RALPH_AUTO_PUSH=false` (no remote to push to)
   - Working directory set to the temp directory
5. Asserts on exit code and stdout content

#### Test Cases

| Test | Fixture | Asserts |
|------|---------|---------|
| AFK formats text blocks | `afk-session.ndjson` | stdout contains Claude's text verbatim |
| AFK formats tool calls as one-liners | `afk-session.ndjson` | stdout contains `-> Read(...)` format, no raw JSON args |
| AFK detects completion file | `complete.ndjson` + sentinel file | exit code 0, sentinel cleaned up |
| AFK exhausts iterations without completion | `afk-session.ndjson` | exit code 2 |
| Missing prompt file | — | exit code 1, stderr contains error message |
| Iterations clamped to max | `afk-session.ndjson` | stdout contains "Warning: Reducing iterations" |
| Help flag | — | exit code 0, stdout contains usage info |
| Bash command truncation | `afk-session.ndjson` | long commands end with `...` |
| AFK double Ctrl+C aborts | `afk-session.ndjson` + two SIGINTs | exit code 130, stdout contains "Press Ctrl+C again to stop" |
| AFK single Ctrl+C resets after timeout | `afk-session.ndjson` + one SIGINT | exit code 2 (iterations exhaust), loop continues after timeout |

### NDJSON Fixtures

Fixtures are derived from real AFK output captured in [`ralph/tests/fixtures/ralph-sample-output.txt`](../ralph/tests/fixtures/ralph-sample-output.txt) (9 iterations of `scripts/ralph.sh --afk 10`).

`ralph/tests/fixtures/afk-session.ndjson` — modeled on iteration 1 of sample output. Covers:
- Text blocks (Claude's reasoning)
- Parallel tool calls (multiple content blocks per event)
- Read with and without `offset`/`limit`
- Edit with `old_string`/`new_string` content (must not appear in formatted output)
- Bash with short and long commands
- TodoWrite with `todos` array
- Grep and Glob tool calls
- Result event without completion promise

`ralph/tests/fixtures/complete.ndjson` — modeled on iteration 9 of sample output. Covers:
- Short session ending with a result event (sentinel file creation is handled by the mock script, not the NDJSON fixture)

### Expected Formatted Output

For `afk-session.ndjson`, the formatter should produce output like:

```
I'll start by studying the required files to understand the context and plan.
-> Read(/Users/william/Repos/buddy-ralph/specs/README.md)
-> Read(/Users/william/Repos/buddy-ralph/plans/cleanup/buddy-llm.md)
Now I can see the cleanup plan. Many items are checked off...
-> TodoWrite(3 items)
Let me read the relevant files in parallel...
-> Read(/Users/william/Repos/buddy-ralph/specs/tokenizer-embedding.md)
-> Read(/Users/william/Repos/buddy-ralph/crates/buddy-llm/src/inference.rs 1:80)
-> Read(/Users/william/Repos/buddy-ralph/specs/buddy-llm.md)
Now I have full context...
-> Edit(/Users/william/Repos/buddy-ralph/specs/tokenizer-embedding.md)
Now let me update the cleanup plan and commit.
-> Edit(/Users/william/Repos/buddy-ralph/plans/cleanup/buddy-llm.md)
-> Bash(git diff specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md)
-> Bash(git log --oneline -5)
-> Bash(git add specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md && git commit -m "$(cat <<'EOF'...)
-> Grep(GgufModelBuilder)
-> Glob(specs/**/*.md)
Done. Updated `specs/tokenizer-embedding.md`...
```

Key differences from old `scripts/ralph.sh` output:
- Edit calls show only file path, not `old_string`/`new_string` content dumps
- TodoWrite shows `3 items`, not full JSON array
- Read with offset/limit shows `1:80` in compact form
- No `\r\n` artifacts or `file_path:` prefixes
- Long Bash commands are truncated at 100 chars with `...`

## Related Specifications

- [springfield](springfield.md) — CLI entry point that orchestrates ralph
- [pensa](pensa.md) — Agent persistent memory, used by the agent inside ralph iterations
