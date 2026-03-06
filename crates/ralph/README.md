# ralph

Iterative Claude Code runner via direct `$AGENT_CMD` invocation. Runs Claude Code repeatedly against a prompt, up to N iterations, with automatic completion detection and git auto-push.

## Usage

```
ralph [OPTIONS] [ITERATIONS] [PROMPT]
```

When invoked by `sgf`, the full command looks like:

```
ralph [-a] [--loop-id ID] [--auto-push BOOL] [--max-iterations N] ITERATIONS PROMPT
```

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `ITERATIONS` | u32 | `1` | Number of iterations to run |
| `PROMPT` | String | `prompt.md` | Prompt file path or inline text string |

### Flags and Options

| Flag/Option | Env Var | Default | Description |
|-------------|---------|---------|-------------|
| `-a`, `--afk` | — | `false` | Run in AFK mode (non-interactive) |
| `--loop-id` | — | — | Loop identifier (sgf-generated, included in banner output) |
| `--max-iterations` | `RALPH_MAX_ITERATIONS` | `100` | Safety limit for iterations |
| `--auto-push` | `RALPH_AUTO_PUSH` | `true` | Auto-push after new commits |
| `--command` | `RALPH_COMMAND` | — | Override: path to executable replacing agent invocation (for testing) |
| `--spec` | `SGF_SPEC` | — | Spec stem — appends ./specs/<stem>.md as a system prompt file |
| `--prompt-file` | — | — | Additional prompt file path (repeatable) |
| `--log-file` | — | — | Path to log file — ralph tees its output here |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENT_CMD` | Path to the agent binary (e.g., `claude`). Required unless `--command` is set. |
| `PROMPT_FILES` | Colon-separated list of prompt files to pass as `--append-system-prompt` study instructions. Supports `$HOME` and `~` expansion. |

### Examples

```bash
ralph 10                              # Interactive, 10 iterations, prompt.md
ralph -a 5                            # AFK mode, 5 iterations
ralph 10 my-task.md                   # Custom prompt file
ralph 5 "fix the login bug"           # Inline text prompt
ralph -a 3 "refactor auth module"     # AFK mode with inline text
RALPH_AUTO_PUSH=false ralph -a 10     # Disable auto-push
ralph -a --loop-id build-auth-20260226T143000 10 prompt.md  # With loop ID
```

## Modes

### Interactive (default)

Spawns the agent with full terminal passthrough (stdin/stdout/stderr inherited). The user interacts with Claude directly. A background watcher thread monitors for the `.ralph-ding` sentinel file and plays a notification sound when Claude needs input.

### AFK (`--afk`)

Spawns the agent with piped stdout. Output is read line-by-line as NDJSON and formatted for human readability. Tool calls are shown as compact one-liners instead of raw JSON.

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Completion file `.ralph-complete` detected |
| `1` | Error (prompt file missing, AGENT_CMD not set, etc.) |
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
