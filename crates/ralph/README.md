# ralph

Iterative Claude Code runner. Invokes `cl` (claude-wrapper) directly with NDJSON stream formatting, sentinel file completion detection, and git auto-push.

## Usage

```
ralph [OPTIONS] [ITERATIONS] [PROMPT]
```

When invoked by `sgf`, the full command looks like:

```
ralph [-a] [--loop-id ID] [--auto-push BOOL] [--banner] [--session-id UUID] ITERATIONS PROMPT
```

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `ITERATIONS` | u32 | `1` | Number of iterations to run (clamped to 1000 max) |
| `PROMPT` | String | `prompt.md` | Prompt file path or inline text string |

### Prompt Resolution

The `PROMPT` argument accepts either a file path or an inline text string. Ralph uses a simple heuristic:

1. If the value is a path to an existing file, read the file and pass its contents to Claude (via `@` prefix)
2. If the value is not a path to an existing file, pass it directly as literal text to Claude

The default value `prompt.md` is treated specially: if no explicit prompt is provided and `prompt.md` does not exist, ralph exits with an error (code 1).

### Flags and Options

| Flag/Option | Env Var | Default | Description |
|-------------|---------|---------|-------------|
| `-a`, `--afk` | — | `false` | Run in AFK mode (non-interactive) |
| `--loop-id` | — | — | Loop identifier (sgf-generated, included in banner output) |
| `--auto-push` | `RALPH_AUTO_PUSH` | `true` | Auto-push after new commits (requires explicit value: `true`/`false`/`yes`/`no`/`1`/`0`) |
| `--banner` | — | `false` | Display ASCII art startup banner. Controlled by cursus TOML iter `banner` field |
| `--command` | `RALPH_COMMAND` | — | Override: path to executable replacing agent invocation (for testing) |
| `--prompt-file` | — | — | Additional prompt file path (repeatable). File content is read and inlined into `--append-system-prompt`. Missing files are a fatal error (exit code 1). |
| `--log-file` | — | — | Path to log file — ralph tees its output here |
| `--session-id` | — | — | Pre-assigned Claude session ID (UUID). Passed through to `cl` as `--session-id <uuid>` on iteration 1. On iterations 2+, ralph generates a fresh UUID for each iteration. |
| `--resume` | — | — | Resume a previous Claude session. Passed through to `cl` as `--resume <session_id>`. Mutually exclusive with `--session-id`. Only applies to iteration 1. |

### Examples

```bash
ralph 10                                               # Interactive, 10 iterations, prompt.md
ralph -a 5                                             # AFK mode, 5 iterations
ralph 10 my-task.md                                    # Custom prompt file
ralph 5 "fix the login bug"                            # Inline text prompt
ralph -a 3 "refactor auth module"                      # AFK mode with inline text
RALPH_AUTO_PUSH=false ralph -a 10                      # Disable auto-push
ralph -a --loop-id build-20260226T143000 10 prompt.md  # With loop ID
ralph --banner -a 10 .sgf/prompts/build.md             # With banner
ralph --prompt-file ./NOTES.md 5 prompt.md             # Extra prompt file
ralph --session-id a1b2c3d4-e5f6-... 10 prompt.md     # With pre-assigned session ID
ralph --resume a1b2c3d4-e5f6-... 1 prompt.md           # Resume a previous session
```

## Modes

### Interactive (default)

Spawns the agent with full terminal passthrough (stdin/stdout/stderr inherited). The user interacts with Claude directly. A background watcher thread monitors for the `.iter-ding` sentinel file and plays a notification sound when Claude needs input.

### AFK (`--afk`)

Spawns the agent with piped stdout. Output is read line-by-line as NDJSON and formatted for human readability. Tool calls are shown as compact one-liners instead of raw JSON.

## Sandbox

Ralph operates within Claude Code's native OS-level sandbox (Seatbelt on macOS, bubblewrap on Linux/WSL2). Project-level sandbox settings are scaffolded by `sgf init` in `.claude/settings.json`.

In both interactive and AFK modes, ralph passes `--settings '{"sandbox": {"allowUnsandboxedCommands": false}}'` to prevent the agent from escaping the sandbox. Combined with `--dangerously-skip-permissions`, this means automated agents operate freely within sandbox bounds but cannot break out.

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Completion file `.iter-complete` detected |
| `1` | Error (prompt file missing, `cl` not found in PATH, etc.) |
| `2` | Iterations exhausted without completion |
| `130` | Interrupted by SIGINT (Ctrl+C) or SIGTERM |

## NDJSON Formatting

In AFK mode, Claude's `stream-json` output is parsed and formatted into compact, readable output:

| Tool | Format | Example |
|------|--------|---------|
| `Read` | `file_path` (+ `offset:limit` if present) | `-> Read(src/main.rs)` |
| `Edit` | `file_path` | `-> Edit(src/main.rs)` |
| `Write` | `file_path` | `-> Write(src/new.rs)` |
| `Bash` | `command` truncated to 100 chars | `-> Bash(git status)` |
| `Glob` | `pattern` | `-> Glob(**/*.rs)` |
| `Grep` | `pattern` | `-> Grep(TODO)` |
| `TodoWrite` | item count from `todos` array | `-> TodoWrite(3 items)` |
| Other | first string value, truncated to 80 chars | `-> WebSearch(rust serde json...)` |

Text blocks are printed verbatim. Non-JSON lines are silently skipped.

## Testing

```bash
cargo test -p ralph           # Run all tests
cargo clippy -p ralph -- -D warnings  # Lint
cargo fmt -p ralph -- --check # Format check
```

Integration tests use `--command` to substitute a mock script for the agent binary, enabling E2E testing without running a real agent.

## Relationship to sgf

`ralph` is invoked by `sgf` commands (`sgf build`, `sgf test`, etc.). `sgf` handles prompt templating, recovery, and lifecycle management, then delegates iteration execution to `ralph` with the appropriate CLI flags. Ralph does not read config files — all configuration arrives via flags and environment variables.
