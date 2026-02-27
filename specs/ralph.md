# ralph Specification

CLI tool for iterative Claude Code execution via Docker sandbox. Replaces `scripts/ralph.sh` with a Rust binary that provides clean NDJSON stream formatting, sentinel file completion detection, and git auto-push.

## Overview

`ralph` provides:
- **Iteration loop**: Run Claude Code repeatedly against a prompt file or inline text, up to N iterations
- **Two modes**: Interactive (terminal passthrough) and AFK (formatted NDJSON stream)
- **NDJSON formatting**: Compact, readable output from Claude's stream-json format
- **Completion detection**: Exit early when Claude signals task completion by creating the `.ralph-complete` sentinel file
- **Interactive notification**: Play a sound on the host when Claude needs user input (via `.ralph-ding` sentinel file)
- **Flexible prompt input**: Accept either a file path or an inline text string as the prompt
- **Git auto-push**: Automatically push new commits after each iteration

## Design Goals

1. **Readable AFK output**: Tool calls shown as compact one-liners, not raw JSON argument dumps
2. **Drop-in replacement**: Same CLI interface and environment variables as `scripts/ralph.sh`
3. **Testable**: NDJSON formatting is a pure function testable without Docker; full binary testable via command override
4. **Minimal dependencies**: Only `clap`, `serde`, `serde_json` — no async runtime needed

## Architecture

```
ralph/
├── src/
│   ├── main.rs      # CLI, startup banner (mode, prompt, iterations, sandbox, loop-id), iteration loop, docker invocation, git operations
│   └── format.rs    # NDJSON parsing, tool call formatting, completion detection
├── tests/
│   ├── integration.rs           # Binary-level E2E tests with mocked docker
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
ralph [-a] [--loop-id ID] [--template T] [--auto-push BOOL] [--max-iterations N] ITERATIONS PROMPT
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
| `--template` | `RALPH_TEMPLATE` | `ralph-sandbox:latest` | Docker sandbox template image |
| `--max-iterations` | `RALPH_MAX_ITERATIONS` | `100` | Safety limit for iterations |
| `--auto-push` | `RALPH_AUTO_PUSH` | `true` | Auto-push after new commits |
| `--command` | `RALPH_COMMAND` | — | Override: path to executable replacing docker invocation (for testing) |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Completion file `.ralph-complete` detected |
| `1` | Error (prompt file missing, etc.) |
| `2` | Iterations exhausted without completion |
| `130` | Interrupted by SIGINT (Ctrl+C) or SIGTERM |

### Examples

```bash
ralph 10                              # Interactive, 10 iterations, prompt.md
ralph -a 5                            # AFK mode, 5 iterations
ralph 10 my-task.md                   # Custom prompt file
ralph 5 "fix the login bug"           # Inline text prompt
ralph -a 3 "refactor auth module"     # AFK mode with inline text
RALPH_TEMPLATE=custom:v2 ralph -a 3   # Custom docker template
RALPH_AUTO_PUSH=false ralph -a 10     # Disable auto-push
ralph -a --loop-id build-auth-20260226T143000 10 prompt.md  # With loop ID (sgf passes this)
```

## Modes

### Interactive Mode (default)

Spawns Docker with full terminal passthrough (stdin/stdout/stderr inherited):

```
docker sandbox run \
  --credentials host \
  --template <TEMPLATE> \
  claude \
  --verbose \
  --dangerously-skip-permissions \
  @<PROMPT_FILE>       # file prompt (@ prefix)
  # or: "<inline text>"  # inline text (no @ prefix)
```

No output processing. The user interacts with Claude directly. After each iteration, ralph checks for the `.ralph-complete` sentinel file to detect task completion.

In interactive mode, ralph also runs a **notification watcher thread** that monitors for the `.ralph-ding` sentinel file. When detected, ralph plays a notification sound on the host machine to alert the user that Claude needs input. See [Interactive Notification](#interactive-notification).

### AFK Mode (`--afk`)

Spawns Docker with piped stdout, stderr inherited:

```
docker sandbox run \
  --credentials host \
  --template <TEMPLATE> \
  claude \
  --verbose \
  --print \
  --output-format stream-json \
  @<PROMPT_FILE>       # file prompt (@ prefix)
  # or: "<inline text>"  # inline text (no @ prefix)
```

Stdout is read line-by-line via `BufRead`, parsed as NDJSON, and formatted for human readability. Lines not starting with `{` are skipped silently (handles Docker/verbose debug output). Each output line is prefixed with `\r\x1b[2K` (carriage return + ANSI clear-line) to counteract Docker sandbox spinner/progress writes to `/dev/tty`, which move the cursor to unpredictable columns. This prefix is applied per line (not per block) because text content from Claude contains embedded newlines. After the process exits, ralph checks for the `.ralph-complete` sentinel file to determine if the task is complete.

## Signal Handling

Ralph registers handlers for SIGINT (Ctrl+C) and SIGTERM at startup using `signal-hook`. These set an `AtomicBool` flag that is polled throughout the iteration loop and inside `run_afk()`.

In AFK mode, two defenses keep Ctrl+C working:

1. **PTY for docker's stdin**: Docker puts its stdin terminal into raw mode via `tcsetattr()`, which disables Ctrl+C signal generation on that terminal. By creating a PTY pair (`openpty()`) and giving docker the slave end as stdin, raw mode only affects the PTY — ralph's terminal stays in cooked mode and Ctrl+C generates SIGINT normally. Docker requires `isatty(0) == true`, so `Stdio::null()` cannot be used. The master end of the PTY is held alive until the child exits.

2. **`setsid()` in `pre_exec`**: Creates a new session, detaching docker from ralph's session. Without this, docker could call `tcsetpgrp()` on the inherited stderr fd (which points to ralph's terminal) to become the foreground process group, stealing SIGINT delivery. With `setsid`, docker is in a different session and `tcsetpgrp()` on ralph's terminal fails.

Stdout is read on a dedicated thread that sends lines through an `mpsc` channel. The main thread uses `recv_timeout` (100ms) to poll the channel, checking the interrupt flag between receives. When interrupted:

1. The child process is killed via `child.kill()`
2. `child.wait()` reaps the process
3. Control returns to the main loop, which detects the flag and exits with code 130

The 2-second sleep between iterations is also interruptible (polled in 100ms increments).

In interactive mode, SIGINT is delivered to the entire foreground process group. The docker child receives it directly (stdin is inherited), handles it, and eventually exits. Ralph's `.status()` call returns, the flag is checked, and ralph exits with code 130.

## Interactive Notification

In interactive mode, ralph plays a host-side notification sound when Claude finishes its turn and needs user input. This uses a sentinel file mechanism that bridges the Docker sandbox and the host.

### Mechanism

1. **Claude Code hooks** (configured in project-level `.claude/settings.local.json`) run `touch .ralph-ding` on `Notification` and `Stop` events inside the sandbox
2. The Docker sandbox syncs files bidirectionally, so `.ralph-ding` appears on the host filesystem
3. **Ralph's watcher thread** polls for `.ralph-ding` every ~100ms
4. On detection: delete the file, spawn `afplay /System/Library/Sounds/Blow.aiff` in the background (non-blocking)

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

Both `afplay` and `touch .ralph-ding` are included so the hooks work in both contexts:
- **On the host** (normal Claude usage): `afplay` plays the sound directly, `.ralph-ding` is ignored (nobody watching)
- **In the sandbox** (ralph usage): `afplay` fails silently (not available in container), `.ralph-ding` is synced to host where ralph's watcher picks it up

Hook event keys in `settings.local.json` **replace** (not merge with) the same keys from user-level `~/.claude/settings.json`. This is why `afplay` must be duplicated here if it also exists in user-level settings.

### Watcher Thread

The watcher runs only in interactive mode (AFK mode has no user interaction). It is a background thread that:

1. Polls `Path::new(".ralph-ding").exists()` every ~100ms
2. On detection: `fs::remove_file(".ralph-ding")`, then `Command::new("afplay").arg("/System/Library/Sounds/Blow.aiff").spawn()` (fire-and-forget)
3. Continues polling until signaled to stop (via `AtomicBool`)

The watcher thread is started before spawning the docker process and stopped after it exits. Stale `.ralph-ding` files are cleaned up at ralph startup alongside `.ralph-complete`.

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
- Search for and delete any stale `.ralph-complete` sentinel file (from a previous crashed/killed run), searching recursively up to depth 2
- Delete stale `.ralph-ding` sentinel file if present

Prompt resolution (before the loop):
- If no explicit prompt provided and `prompt.md` does not exist → exit 1 with error
- If explicit prompt provided and it is a path to an existing file → use as file prompt (`@` prefix)
- If explicit prompt provided and it is not an existing file → use as inline text (no `@` prefix)

The startup banner includes mode, prompt source, iteration count, sandbox template, and loop ID (if provided via `--loop-id`).

For each iteration `i` in `1..=iterations`:

1. Print iteration banner (includes loop ID if provided)
2. Record `git rev-parse HEAD` as `head_before`
3. Execute Claude via Docker (or `--command` override):
   - Interactive: start notification watcher thread, `.status()` with inherited stdio, stop watcher thread
   - AFK: `.spawn()` with piped stdout, read lines via reader thread + channel through `format_line()`
4. If interrupted: print "Interrupted.", exit 130
5. Search for `.ralph-complete` recursively (depth <= 2): if found, delete it, print completion banner, auto-push, exit 0
6. Print "Iteration N complete, continuing..."
7. Sleep 2 seconds (interruptible, polled in 100ms increments)
8. If interrupted: print "Interrupted.", exit 130
9. If `--auto-push` and HEAD changed: run `git push`

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

When `--command` (or `RALPH_COMMAND`) is set, ralph runs the specified executable instead of `docker`. The override executable is invoked with no arguments and must write NDJSON (AFK mode) or interactive output to stdout.

This enables integration testing without Docker. Tests create a mock script that emits fixture NDJSON. For completion detection tests, the mock script also creates the `.ralph-complete` sentinel file.

## Error Handling

No custom error types. Fail loudly, continue when sensible:

| Scenario | Behavior |
|----------|----------|
| Default prompt file missing | `eprintln!` + exit 1 (before loop starts, only when no explicit prompt provided) |
| Docker/command spawn failure | `eprintln!` warning, continue to next iteration |
| NDJSON parse error | Skip line silently (non-JSON output from Docker/verbose) |
| stdout read error | `eprintln!` warning, continue reading |
| Git `rev-parse` failure | Return `None`, skip push check |
| Git push failure | `eprintln!` warning, continue |
| SIGINT/SIGTERM received | Kill child process (AFK), print "Interrupted.", exit 130 |

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
   - `RALPH_COMMAND` set to the mock script path
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

- [scripts/ralph.sh](../scripts/ralph.sh) — Bash predecessor (to be deleted after implementation)
- [scripts/README.md](../scripts/README.md) — Docker sandbox setup documentation
