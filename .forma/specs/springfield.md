# springfield Specification

CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | stable |

## Overview

CLI entry point for Springfield. All developer interaction goes through this binary. It handles project scaffolding, cursus pipeline orchestration, direct agent invocation (iteration loops), recovery, and daemon lifecycle. Delegates persistent memory to pensa.

`sgf` provides:
- **Project scaffolding**: `sgf init` creates the project structure (`.sgf/`, `.pensa/`, `.forma/`, Claude deny settings, git hooks)
- **Unified command dispatch**: `sgf <command>` resolves to a cursus TOML pipeline definition (local `./.sgf/cursus/` → global `~/.sgf/cursus/`)
- **Cursus orchestration**: Parse cursus TOML definitions, execute multi-iter pipelines with sentinel-based transitions, context passing, and stall recovery (see [cursus spec](cursus.md))
- **Simple prompt mode**: `sgf <file>` runs a prompt file as a simple iteration loop (no cursus TOML needed)
- **Iteration runner**: Direct `cl` invocation with NDJSON formatting, completion detection, terminal settings preservation, and git auto-push — absorbed from the former `ralph` crate
- **Programmatic mode**: When stdin is not a TTY (piped from an outer agent), sgf automatically switches to programmatic mode — emitting structured NDJSON events on stdout and accepting plain text input on stdin. Enables outer agents to drive cursus pipelines turn-by-turn. Explicit override via `--output-format json`.
- **Auto-retry**: Automatic retry on agent process crashes (API failures, rate limits, network errors) with configurable immediate retries and backoff intervals. Resumes the crashed session automatically.
- **Loop orchestration**: Launch iteration loops with the correct flags, manage PID files, tee logs
- **Recovery**: Pre-launch cleanup of dirty state from crashed iterations
- **Daemon lifecycle**: Start the pensa and forma daemons before launching loops

## Implementation Order

The springfield spec is large. For implementers, the recommended reading and implementation order:

1. **Architecture** — understand the project structure, module layout, and file purposes
2. **CLI Commands** — the public interface; defines what the binary does
3. **sgf init** — scaffolding (no runtime dependencies, good starting point)
4. **Pre-launch Lifecycle** → **Recovery** — daemon startup and dirty-state cleanup
5. **Agent Invocation** — how `cl` is called in both modes (core runtime loop)
6. **Prompt Delivery** — how prompts are resolved and passed to `cl`
7. **Console Output** — the `style.rs` badge box system (needed by all output)
8. **Logging** — log tee and `sgf logs`
9. **Workflow Stages** — the build/verify/test/spec stage behaviors
10. **Session metadata** — see [session-resume spec](session-resume.md) for the resume system
11. **Cursus integration** — see [cursus spec](cursus.md) for pipeline orchestration

Sections like Defaults, Key Design Principles, and Future Work are reference material — read as needed.

## Architecture

## Per-Repo Project Structure

After `sgf init` and ongoing development, a project contains:

```
.pensa/
├── issues.jsonl               (committed — git-portable export)
├── deps.jsonl                 (committed)
├── comments.jsonl             (committed)
├── src_refs.jsonl             (committed)
└── doc_refs.jsonl             (committed)
.sgf/
├── MEMENTO.md                 (fm/pn workflow reference — authored per-project)
├── BACKPRESSURE.md            (build/test/lint/format reference — authored per-project)
├── cursus/                    (project-local cursus pipeline overrides)
├── logs/                      (gitignored — AFK loop output)
│   └── <loop-id>.log
├── run/                       (gitignored — PID files and session metadata for running/completed loops)
│   ├── <loop-id>.pid
│   └── <loop-id>.json         (session metadata — session_id, loop config, status; see session-resume spec)
└── prompts/                   (optional — project-local overrides only)
    └── build.md               (example: overrides just build.md, other prompts fall through to ~/.sgf/)
.pre-commit-config.yaml        (prek hooks for pensa sync)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (`ln -s` to AGENTS.md)
test-report.md                 (generated — overwritten each test run, committed)
verification-report.md         (generated — overwritten each verify run, committed)
```

Specs are managed by forma and read via `fm show <stem> --json`. The `.forma/specs/*.md` files are generated read-only artifacts for humans.

### Global Home Structure

Populated by `just install` (rsync from the springfield repo's `.sgf/`):

```
~/.sgf/
├── MEMENTO.md                 (universal agent instructions — fm/pn workflows, conventions)
├── BACKPRESSURE.md            (universal build/test/lint/format reference)
├── cursus/                    (global cursus pipeline definitions)
│   ├── build.toml
│   ├── spec.toml
│   ├── verify.toml
│   └── ...
└── prompts/
    ├── build.md               (default prompts for all projects)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    ├── issues-log.md
    └── doc.md
```

### Installation

All crates are installed via `just install`, which also syncs the global `~/.sgf/` directory:

```just
install:
    cargo install --path crates/pensa
    cargo install --path crates/springfield
    cargo install --path crates/claude-wrapper
    rsync -av --delete --exclude='logs/' --exclude='run/' .sgf/ ~/.sgf/
```

The rsync copies prompts, cursus definitions, MEMENTO.md, and BACKPRESSURE.md to `~/.sgf/`. The `--delete` flag removes files from `~/.sgf/` that no longer exist in the repo. Runtime directories (`logs/`, `run/`) are excluded.

### Iteration Runner Module

The iteration runner is built into sgf (absorbed from the former `ralph` crate). It provides direct `cl` invocation with:

```
crates/springfield/
├── src/
│   ├── iter_runner/
│   │   ├── mod.rs       # Iteration loop, agent invocation, completion detection
│   │   ├── format.rs    # NDJSON parsing, tool call/result formatting (pure, no ANSI)
│   │   ├── style.rs     # ANSI escape code helpers (bold, dim, green, yellow, red), NO_COLOR support
│   │   └── banner.rs    # Box-drawing banner renderer (render_box)
│   ├── ...
```

Key components:
- **`format.rs`** — Pure function `format_line()` that parses NDJSON and returns structured `FormattedOutput`
- **`style.rs`** — ANSI styling with `NO_COLOR` support (reconciled from both former style modules)
- **`banner.rs`** — Box-drawing banner renderer for iteration/completion/stall banners
- **`TeeWriter`** — Writes styled output to stdout and stripped output to log file
- **Stdout reader thread** — Reads agent stdout via `mpsc` channel with 100ms poll for interrupt checking
- **Notification watcher** — Monitors `.iter-ding` sentinel for interactive notification sounds
- **Terminal settings save/restore** — `tcgetattr`/`tcsetattr` around agent invocations
- **`cl`-in-PATH check** — Verifies `cl` is available before starting the loop

### File Purposes

**`~/.sgf/BACKPRESSURE.md`** — Universal build, test, lint, and format commands. Developer-editable. Override per-project by placing a `BACKPRESSURE.md` in `./.sgf/`. Injected into every Claude session by `cl` (see claude-wrapper spec).

**`~/.sgf/MEMENTO.md`** — Universal agent instructions (fm/pn workflows, conventions). Override per-project by placing a `MEMENTO.md` in `./.sgf/`. Injected into every Claude session by `cl`.

**`AGENTS.md`** — Hand-authored operational guidance. Contains code style preferences, runtime notes, and special instructions. Created as an empty file by `sgf init`.

**`CLAUDE.md`** — Entry point for Claude Code. Symlinks to AGENTS.md. Auto-loaded by Claude Code at the start of every session.

**`~/.sgf/cursus/`** — Global cursus pipeline definitions. Each `.toml` file defines a command available via `sgf <name>`. Synced from the springfield repo via `just install`. To override a cursus for a specific project, create `./.sgf/cursus/<name>.toml` — that file takes precedence for that project only.

**`~/.sgf/prompts/`** — Default prompts for all projects. Synced from the springfield repo via `just install`. To override a prompt for a specific project, create `./.sgf/prompts/<name>.md` — that file takes precedence for that project only.

**`.sgf/run/{loop_id}.json`** — Session metadata file. Contains `session_id` (UUID), loop config (`mode`, `prompt`, `iterations_completed`, `iterations_total`), and `status` (`running`, `completed`, `interrupted`, `exhausted`). Written before spawning cl and updated on exit. Enables `--resume <run-id>` to restart previous sessions. See [session-resume spec](session-resume.md) for the full schema.

**`.sgf/` and `.claude/` protection** — Both `.sgf/` and `.claude/` are protected from agent modification via Claude deny settings. `sgf init` scaffolds these rules. `.sgf/` protection prevents agents from modifying local overrides and reference files. `.claude/` protection prevents agents from weakening sandbox configuration or deny rules.


## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (4, derive + env) | CLI argument parsing |
| `serde` (1, derive) | Serialization for run state and NDJSON parsing |
| `serde_json` (1) | JSON handling for run metadata and NDJSON stream |
| `chrono` (0.4) | Timestamps for run metadata and loop IDs |
| `toml` (0.8) | Cursus TOML pipeline definition parsing |
| `sha2` (0.10) | SHA-256 for daemon port derivation |
| `uuid` (1, v4) | UUIDv4 session ID generation |
| `shutdown` (workspace) | Shared graceful shutdown handling, ChildGuard, ProcessSemaphore (see [shutdown spec](shutdown.md)) |
| `vcs-utils` (workspace) | Git operations — auto-push (see [vcs-utils spec](vcs-utils.md)) |
| `libc` (0.2) | Terminal settings save/restore (`tcgetattr`/`tcsetattr`) |
| `tracing` (0.1) | Structured logging |
| `tracing-subscriber` (0.3, fmt + env-filter) | Log output formatting, `RUST_LOG` env filter |

Dev dependencies:

| Crate | Purpose |
|-------|---------|
| `tempfile` (3) | Temporary directories for test isolation |
| `assert_cmd` (2) | CLI testing assertions |
| `predicates` (3) | Output matching predicates |
| `portpicker` (0.1) | Random port selection for test daemons |
| `nix` (0.29, signal) | Signal delivery in tests |

Note: `cl` (claude-wrapper), `pn` (pensa), and `fm` (forma) are all invoked as child process binaries via `std::process::Command`, not linked as crate dependencies.

## Error Handling

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|---|
| `0` | Sentinel found (`.iter-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

### Iteration Runner Errors

| Scenario | Behavior |
|----------|----------|
| `cl` not found in PATH (no `SGF_AGENT_COMMAND`) | `tracing::error\!` + exit 1 (before loop starts) |
| Prompt file missing | `tracing::error\!` + exit 1 |
| Agent/command spawn failure | `tracing::warn\!`, continue to next iteration |
| NDJSON parse error (line starts with `{`) | Skip line, log at debug level |
| Non-JSON line (no `{` prefix) | Skip line silently (expected verbose debug output) |
| stdout read error | `tracing::warn\!`, continue reading |
| Git `rev-parse` failure | Return `None`, skip push check |
| Git push failure | `tracing::warn\!`, continue |
| SIGINT/Ctrl+D received (all modes) | First press: print confirmation to stderr, start 2s timeout. Second press of same key: kill child process group, exit 130. Timeout: reset counter, continue. |
| SIGTERM received | Kill child process group, exit 130 (immediate, single signal) |

### Recovery Failure Modes

- **Git checkout/clean failure**: Fatal — loop launch is aborted. Proceeding with dirty state would violate the atomic iteration guarantee.
- **`pn doctor --fix` failure**: Warning only — supplementary, not critical for state consistency.
- **Daemon startup failure**: Fatal — loop cannot proceed without pensa/forma daemons. 5-second deadline with exponential backoff.

### Auto-Retry on Agent Process Failure

When the `cl` process fails mid-execution (API rate limit, usage limit, network error, OOM, unexpected crash), sgf automatically retries the failed invocation. This applies to all modes: interactive, AFK, and programmatic.

#### Failure Classification

Retryable failures are distinguished from non-retryable ones using exit code/signal analysis plus a duration heuristic:

| Failure | Retryable | Detection |
|---------|-----------|-----------|
| API rate limit / usage limit | Yes | Non-zero exit after sustained execution |
| Network error | Yes | Non-zero exit after sustained execution |
| OOM / crash (signal) | Yes | Process killed by signal (SIGSEGV, SIGKILL, etc.) |
| User Ctrl+C / Ctrl+D | No | SIGINT detected by shutdown controller |
| Bad arguments / config error | No | Exit code 1 within first few seconds of startup |

The **duration heuristic** acts as a safety net: if the process ran for less than a few seconds before failing with exit code 1, it is treated as a non-retryable startup error (bad args, missing prompt, config issue). If it ran longer, the failure is treated as retryable (the agent was working and something went wrong mid-execution).

#### Retry Strategy

1. **Immediate retries**: Up to 3 attempts with no delay between them
2. **Backoff retries**: If all immediate retries fail, retry every 5 minutes
3. **Maximum duration**: 12 hours total from the first failure. After 12 hours of failed retries, sgf exits with an error.
4. **On success**: Resume the crashed session via `cl --resume <session_id>`. The agent picks up the conversation where it left off — no context is lost.

#### Configuration

Retry behavior is configurable per-cursus in the TOML definition (see [cursus spec](cursus.md) TOML Format):

```toml
[retry]
immediate = 3           # immediate retry attempts (default: 3)
interval_secs = 300     # backoff interval in seconds (default: 300)
max_duration_secs = 43200  # max total retry duration in seconds (default: 43200 = 12 hours)
```

If the `[retry]` section is omitted, the defaults above apply.

Simple prompt mode (`sgf <file>`) uses the same defaults. Retry behavior is not configurable in simple prompt mode — users who need custom retry settings should create a cursus TOML.

#### Retry State

Retry state is purely in-process — it does not persist across sgf restarts. If sgf itself is killed during the retry window, the retry loop dies with it. The run can still be resumed manually via `--resume <run-id>`.

#### Notifications

During retries, sgf emits status messages:
- **Terminal mode**: Badge box messages (e.g., `retrying in 5m (attempt 4/147)...`)
- **Programmatic mode**: Structured `retry` events in the NDJSON stream

### Resume Command on Exit

On any run exit — stall, interrupt, completion, or error — sgf prints a copy-pasteable resume command:

```
To resume:  sgf change --resume change-20260422T150000
```

This is printed even on Ctrl+C / Ctrl+D exits. In programmatic mode, this information is included in the `run_complete` or `error` event.

## Testing

Springfield is tested via integration tests that exercise the full CLI. All integration tests use a shared test harness (see [test-harness spec](test-harness.md)).

### Key test scenarios

| Area | Scenario | Assertion |
|------|----------|-----------|
| Init | `sgf init` idempotence | Re-running creates no duplicates in .gitignore, settings.json, hooks |
| Init | `sgf init --force` safety | Fails on uncommitted changes; prompts before overwrite |
| Init | Frontend scaffolding runs by default | `package.json`, `vite.config.ts`, `eslint.config.js` exist after init |
| Init | Frontend scaffolding skipped with `--no-fe` | No `package.json` or `vite.config.ts` created |
| Init | Frontend scaffolding skipped when `package.json` exists | create-vite not invoked; existing package.json preserved |
| Init | Frontend scaffolding skipped when `vite.config.ts` exists | create-vite not invoked; existing config preserved |
| Init | pnpm not found error | Exits with error message when pnpm missing and no `--no-fe` |
| Init | create-vite failure warning | Warns with command, continues SGF scaffolding |
| Init | `--force` does not re-run create-vite | Existing frontend files untouched by `--force` |
| Init | Sandbox domains include npm registries | `registry.npmjs.org` and `registry.yarnpkg.com` in allowedDomains |
| Init | SvelteKit not in .gitignore | `.svelte-kit/` entry absent |
| Command resolution | Alias resolution | `sgf b` resolves to `build` cursus when `alias = "b"` |
| Command resolution | Local overrides global | `./.sgf/cursus/build.toml` takes precedence over `~/.sgf/cursus/build.toml` |
| Command resolution | Simple prompt mode | `sgf my-task.md` detected as file path, runs iteration loop |
| Command resolution | Unknown command | `sgf nonexistent` exits 1 with "unknown command" message |
| Cursus | TOML parsing and validation | Duplicate iter names, missing transitions → exit 1 |
| Cursus | Sentinel transitions | `.iter-complete`, `.iter-reject`, `.iter-revise` trigger correct next iter |
| Cursus | Context passing | `produces` files written to run dir; `consumes` files injected into prompt |
| Cursus | Stall recovery | Iter exhaustion → stalled state → `--resume` works |
| Pre-launch | Recovery | Stale `.pid` files cleaned up; stale run metadata marked interrupted |
| Pre-launch | Daemon lifecycle | Daemons start, become ready, survive across iterations |
| Pre-launch | Data export | pn export and fm export run after daemons, before loop |
| Pre-launch | Export failure non-fatal | pn/fm export failure logs warning, does not block loop |
| Shutdown | Double Ctrl+C | Process group killed with escalation |
| Shutdown | Double Ctrl+D | Same as double Ctrl+C |
| Loop | Loop ID generation | Format: `<command>-<timestamp>`, unique per invocation |
| Loop | Iteration clamping | `-n 5000` clamps to 1000 with warning |
| Loop | `--no-push` flag | Auto-push suppressed; no git push after commits |
| Loop | Post-result timeout (AFK) | Agent killed after 30s of no exit following result event |
| Output | Console formatting | Banners, iteration headers, stall messages formatted correctly |
| List | `sgf list` output | Shows cursus commands with descriptions and built-ins |
| List | Local override display | Local cursus overrides global; only one entry per command name |
| Logs | `sgf logs <loop-id>` | Tails the correct log file |
| Logs | Missing log file | Exits 1 with error message |
| Flags | `-a` and `-i` mutual exclusion | Passing both exits 1 with error message |
| Programmatic | isatty detection | Piped stdin triggers programmatic mode (NDJSON events on stdout) |
| Programmatic | `--output-format json` flag | Explicit flag triggers programmatic mode even with TTY stdin |
| Programmatic | Structured events emitted | `run_start`, `iter_start`, `turn`, `iter_complete`, `run_complete` events in correct order |
| Programmatic | Turn-by-turn driving | Outer agent sends message via stdin, receives structured JSON response, resumes with `--resume` |
| Programmatic | AFK iters in programmatic mode | AFK iters run to completion, emit iter events, no input needed |
| Programmatic | Stall event | Iter exhaustion emits `stall` event with iter info and available actions |
| Programmatic | Resume command on exit | All exit paths (stall, interrupt, complete, error) print resume command |
| Resume | `--resume <run-id>` on subcommand | Restores full cursus state (iter, iteration, context) and continues |
| Resume | `--resume` with invalid run-id | Exits 1 with "run not found" error |
| Resume | Resume interrupted run | Resumes from interruption point |
| Resume | Resume stalled run (interactive) | Offers Retry/Skip/Abort options |
| Resume | Resume stalled run (programmatic) | Emits stall event, waits for input |
| Auto-retry | Retryable failure detection | Non-zero exit after sustained execution triggers retry |
| Auto-retry | Non-retryable failure detection | Exit code 1 within first seconds treated as config error, no retry |
| Auto-retry | Signal-killed process | Process killed by signal (SIGSEGV) triggers retry |
| Auto-retry | User interrupt not retried | SIGINT/SIGTERM detected by shutdown controller does not trigger retry |
| Auto-retry | Immediate retries | Up to 3 immediate retries before backoff |
| Auto-retry | Backoff interval | After 3 immediate failures, retries every 5 minutes |
| Auto-retry | Max duration | Retries stop after 12 hours, exits with error |
| Auto-retry | Session resume on success | Successful retry uses `--resume <session_id>` to continue crashed session |
| Auto-retry | Custom config | Cursus TOML `[retry]` section overrides defaults |

### Test infrastructure

- **Shared mock binaries** (`MOCK_BINS` / `mock_bin_path()`) — single set of mock `pn`/`fm` scripts reused by all tests
- **Concurrency semaphore** (`SGF_PERMITS` / `run_sgf()`) — limits concurrent `sgf` subprocess invocations to prevent resource exhaustion
- **Automatic preflight skip** — `sgf_cmd()` injects `SGF_SKIP_PREFLIGHT=1` and mock `PATH` by default
- **Mock pnpm** — Frontend scaffolding tests use a mock `pnpm` binary that creates expected files (package.json, vite.config.ts, etc.) without network access. Tests verify that `sgf init` invokes `pnpm create vite@latest . -- --template react-ts` with the correct arguments and handles success/failure appropriately.

## CLI Commands

```
sgf <command> [-a | -i] [-n N] [--no-push] [--skip-preflight] [--output-format json] [--resume <run-id>]   — run a cursus pipeline
sgf <file>                                                       — run a prompt file as a simple iteration loop
sgf init [--force] [--no-fe]                                     — scaffold a new project
sgf list                                                         — show available commands with descriptions
sgf logs <loop-id>                                               — tail a running loop's output
```

Where `<command>` resolves to a cursus TOML pipeline definition. Commands can also be invoked by alias (e.g., `sgf b` for `sgf build` if `alias = "b"` is configured in the cursus TOML).

### Command Resolution

1. Check if `<command>` matches a reserved built-in (`init`, `list`, `logs`). If so, run the built-in.
2. Check if the argument resolves to an existing file path. If so, run it as a simple iteration loop (see [Simple Prompt Mode](#simple-prompt-mode)).
3. Check if `./.sgf/cursus/<command>.toml` exists (local override). If so, parse and run the cursus.
4. Check if `~/.sgf/cursus/<command>.toml` exists (global default). If so, parse and run the cursus.
5. Check if `<command>` matches an alias in any resolved cursus TOML. If so, resolve to the aliased cursus and run it.
6. Error: `unknown command: <command>`.

### Simple Prompt Mode (`sgf <file>`)

When the argument resolves to an existing file path, sgf runs it as a simple iteration loop — no cursus TOML needed. This replaces the former standalone `ralph` usage.

```bash
sgf my-task.md                     # Interactive, 1 iteration
sgf my-task.md -a -n 10            # AFK mode, 10 iterations
sgf .sgf/prompts/build.md -a       # AFK mode, 1 iteration
```

Behavior:
- Runs the iteration runner directly with the file as the prompt
- Checks `.iter-complete` after each iteration (same as cursus mode)
- Supports `-a`/`-i`, `-n`, `--no-push` flags
- No context injection via `consumes` — keep simple mode simple. `cl` still injects MEMENTO/BACKPRESSURE independently.
- Exit codes: 0 (`.iter-complete` found), 2 (iterations exhausted), 130 (interrupted)

### Resume via `--resume <run-id>`

Any sgf subcommand accepts `--resume <run-id>` to resume a stalled or interrupted run:

```bash
sgf change --resume change-20260422T150000
sgf spec --resume spec-20260317T140000
```

Behavior:
1. Load `.sgf/run/<run-id>/meta.json` (for cursus runs) or `.sgf/run/<run-id>.json` (for non-cursus sessions)
2. Restore full pipeline state: current iter, iteration count, accumulated context
3. Continue execution from the stalled/interrupted point
4. For stalled runs, offers options: Retry, Skip, or Abort (interactive mode). In programmatic mode, emits a stall event and waits for input.

On any run exit (stall, interrupt, completion, error), sgf prints a copy-pasteable resume command:

```
To resume:  sgf change --resume change-20260422T150000
```

This appears in both terminal output (for humans) and as a structured JSON event (for outer agents in programmatic mode).

The former `sgf resume` built-in command is removed. All resume functionality is accessed via `--resume <run-id>` on the original subcommand.

### Programmatic Mode

When stdin is not a TTY (`isatty(stdin) == false`), sgf automatically switches to programmatic mode. This can also be forced explicitly with `--output-format json`.

In programmatic mode:
- **Output**: Structured NDJSON events on stdout (see [cursus spec](cursus.md) Structured Events section)
- **Input**: Plain text on stdin — the same thing a human would type. Passed through to the inner `cl` session.
- **Execution model**: Each invocation runs until it needs input (interactive iter waiting for response) or completes (AFK iter finished, pipeline done). The outer agent reads the JSON output, decides what to respond, and sends the next message via a new invocation with `--resume <run-id>`.

The outer agent drives sgf turn-by-turn:

```bash
# Turn 1: start pipeline
echo "add login validation" | sgf change
# → JSON: {run_id, iter_start, turn with agent response, waiting_for_input}

# Turn 2: respond to agent
echo "yes, use bcrypt" | sgf change --resume change-20260422T150000
# → JSON: {turn with agent response, iter_complete, run_complete}
```

Cursus TOML files are unchanged. `mode: "interactive"` means "this iter needs a conversation" — whether the conversant is a human (terminal) or an outer agent (piped stdin) is determined at runtime by `isatty(stdin)`.

AFK iters run to completion internally in both modes. The outer agent receives status events but does not need to send input during AFK iters.

### Reserved Built-in: `list`

```
sgf list
```

Displays available commands with their descriptions. Reads `.sgf/cursus/` directories (local `./.sgf/cursus/` first, then global `~/.sgf/cursus/`), parses the `description` field from each TOML file, and displays a table of available commands. Local cursus definitions override global ones of the same name. Populated on the fly — no caching.

Output format:

```
Available commands:

  build        Implementation loop
  spec         Spec creation and refinement
  verify       Verification loop
  test         Test execution loop
  test-plan    Test plan generation
  doc          Documentation triage
  issues-log   Bug reporting

Built-ins:

  init         Scaffold a new project
  list         Show available commands
  logs         Tail a running loop's output
```

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | from cursus TOML | AFK mode: overrides iter-level `mode` for all iters |
| `-i` / `--interactive` | from cursus TOML | Interactive mode: overrides iter-level `mode` for all iters |
| `--no-push` | `false` | Disable auto-push after commits (overrides `auto_push` on all iters) |
| `-n` / `--iterations` | from cursus TOML | Number of iterations (overrides `iterations` on all iters) |
| `--skip-preflight` | `false` | Disable all pre-launch checks including recovery and daemon startup |
| `--output-format` | — | Output format. `json` enables programmatic mode with structured NDJSON events on stdout. Auto-detected when stdin is not a TTY. |
| `--resume <run-id>` | — | Resume a stalled or interrupted run. Restores full pipeline state (iter, iteration, context) and continues execution. |

`-a` and `-i` are mutually exclusive — passing both is an error (exit 1 with a clear message). When neither is passed, the default comes from the cursus TOML iter definition (or `interactive` for simple prompt mode).

### Examples

```bash
sgf build -a -n 30             # AFK build loop, 30 iterations
sgf b                          # same as sgf build (alias from cursus TOML)
sgf spec                       # multi-iter spec refinement pipeline (discuss → draft → review → approve)
sgf build -i                   # force interactive build (overrides cursus TOML mode)
sgf verify -a                  # force AFK verify
sgf issues-log                 # interactive bug reporting
sgf doc                        # interactive doc triage
sgf list                       # show available commands
sgf change --resume change-20260422T150000  # resume specific run
sgf my-task.md -a -n 5         # simple prompt mode
sgf init --no-fe               # scaffold without frontend (backend-only project)

# Programmatic mode (outer agent driving sgf):
echo "add login validation" | sgf change
echo "yes, proceed" | sgf change --resume change-20260422T150000
```

## sgf init

Scaffolds a new project. Creates the project-local directory structure and configuration files. Does **not** write prompt files or context files — those live in the global `~/.sgf/` (synced via `just install`). Accepts `--force` to overwrite skeleton files with built-in defaults. Accepts `--no-fe` to skip frontend scaffolding.

### Execution order

1. **Frontend scaffolding** (unless `--no-fe` or frontend already exists)
2. **SGF infrastructure** (directories, skeleton files, config merges, prek hooks)

### Frontend scaffolding

By default, `sgf init` runs `pnpm create vite@latest . -- --template react-ts` to scaffold a React + TypeScript frontend project with Vite.

**Skip conditions** — Frontend scaffolding is skipped when:
- `--no-fe` flag is passed
- `package.json` already exists in the target directory
- `vite.config.ts` already exists in the target directory

**pnpm requirement** — If `pnpm` is not found on `$PATH` (and `--no-fe` is not set and skip conditions are not met), `sgf init` exits with an error: `pnpm not found — install pnpm or pass --no-fe`.

**Failure handling** — If `pnpm create vite@latest` exits non-zero, `sgf init` prints a warning with the exact command that needs to be run manually, then continues with SGF infrastructure scaffolding. This way a network failure does not block project setup.

**`--force` interaction** — `--force` does not re-run create-vite. It only affects SGF skeleton files.

### What it creates

```
.pensa/                                (directory only — daemon creates db.sqlite on start)
.forma/                                (directory only — daemon creates db.sqlite on start)
.sgf/
├── cursus/                            (empty — project-local cursus overrides)
├── logs/                              (empty, gitignored)
└── run/                               (empty, gitignored)
.claude/settings.json                  (deny rules for .sgf/** and .claude/**)
.pre-commit-config.yaml                (prek hooks for pensa + forma sync)
.gitignore                             (Springfield entries + stack-specific entries)
AGENTS.md                              (empty file)
CLAUDE.md                              (`ln -s` to AGENTS.md)
```

When frontend scaffolding runs, create-vite additionally produces:

```
eslint.config.js
index.html
package.json
public/
src/
tsconfig.json
tsconfig.app.json
tsconfig.node.json
vite.config.ts
```

No `.sgf/prompts/` directory is created — prompts resolve via layered lookup (local `./.sgf/prompts/` → global `~/.sgf/prompts/`). Users create `./.sgf/prompts/` manually when they need project-local overrides.

### Claude settings

`sgf init` creates or updates `.claude/settings.json` with deny rules protecting `.sgf/` and `.claude/` from agent modification, plus native sandbox configuration:

```json
{
  "permissions": {
    "deny": [
      "Edit .sgf/**",
      "Write .sgf/**",
      "Bash rm .sgf/**",
      "Bash mv .sgf/**",
      "Edit .claude/**",
      "Write .claude/**",
      "Bash rm .claude/**",
      "Bash mv .claude/**"
    ]
  },
  "sandbox": {
    "enabled": true,
    "autoAllowBashIfSandboxed": true,
    "network": {
      "allowedDomains": [
        "localhost",
        "github.com",
        "*.githubusercontent.com",
        "crates.io",
        "*.crates.io",
        "registry.npmjs.org",
        "registry.yarnpkg.com"
      ],
      "allowLocalBinding": true
    }
  }
}
```

If `.claude/settings.json` already exists, `sgf init` merges both deny rules and sandbox settings into the existing file without duplicating entries or removing existing rules. Array fields (`permissions.deny`, `sandbox.network.allowedDomains`) are merged additively — existing entries are preserved, new entries are appended if not already present. Scalar fields (`sandbox.enabled`, `sandbox.autoAllowBashIfSandboxed`, `sandbox.network.allowLocalBinding`) are set only if not already present in the file.

#### Sandbox configuration

Claude Code's native sandbox provides OS-level filesystem and network isolation using Seatbelt (macOS) and bubblewrap (Linux/WSL2). The scaffolded configuration enables sandbox for all sessions — both interactive and automated.

| Setting | Value | Rationale |
|---------|-------|-----------|
| `sandbox.enabled` | `true` | OS-level enforcement for all sessions |
| `sandbox.autoAllowBashIfSandboxed` | `true` | Bash commands auto-approved within sandbox bounds, reducing prompt fatigue |
| `sandbox.network.allowedDomains` | `["localhost", "github.com", "*.githubusercontent.com", "crates.io", "*.crates.io", "registry.npmjs.org", "registry.yarnpkg.com"]` | `localhost` for pensa daemon access; GitHub for git operations; crates.io for cargo; npmjs/yarnpkg for frontend deps |
| `sandbox.network.allowLocalBinding` | `true` | Allows test servers (e.g., `cargo test`) to bind localhost ports |

**Automated stages:** The sandbox configuration in `.claude/settings.json` applies to automated agents. Combined with `--dangerously-skip-permissions`, automated agents operate freely within sandbox bounds but cannot break out.

**Interactive stages:** Use project settings as-is. The sandbox is active; `allowUnsandboxedCommands` is left to the developer's global settings.

**Extending for other stacks:** The default domains cover Rust and frontend development. Developers add domains for their stack (e.g., `pypi.org` for Python) by editing `.claude/settings.json`. Additional filesystem write paths (e.g., `~/.npm`, `~/.cache`) follow the same pattern via global settings.

### Prek hooks

[prek](https://github.com/j178/prek) is a Rust-based git hook manager that reads `.pre-commit-config.yaml`. It replaces the Python-based [pre-commit](https://pre-commit.com/) — same config format, no Python dependency. `sgf init` generates the config and runs `prek install` to wire the hooks into `.git/hooks/`.

`sgf init` creates `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: pensa export
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: pensa import
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
      - id: forma-export
        name: forma export
        entry: fm export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: forma-import
        name: forma import
        entry: fm import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
```

If `.pre-commit-config.yaml` already exists, `sgf init` appends the pensa and forma hooks without duplicating them.

### Gitignore

`sgf init` creates `.gitignore` or appends entries to an existing one. Entries are added idempotently — existing lines are not duplicated.

#### Entries added

```gitignore
# Springfield
.pensa/db.sqlite
**/.pensa/daemon.port
**/.pensa/daemon.project
**/.pensa/daemon.url
**/.forma/daemon.port
**/.forma/daemon.project
**/.forma/daemon.url
.sgf/logs/
.sgf/run/
.iter-*

# Rust
/target

# Node
node_modules/

# Playwright
/test-results/
/playwright-report/
/blob-report/
/playwright/.cache/

# Environment
.env
.env.local
.env.*.local

# macOS
.DS_Store
```

All entries are always added regardless of what exists in the directory. If an entry already exists anywhere in the file, it is not added again.

### CLAUDE.md

`ln -s` to AGENTS.md.

### Idempotence

`sgf init` is safe to re-run. Frontend scaffolding is skipped when `package.json` or `vite.config.ts` already exists. It skips files that already exist (AGENTS.md, CLAUDE.md) and only merges additive content (deny rules, git hooks, gitignore entries). It never overwrites existing content. `prek install` is always run to ensure hooks are wired into `.git/hooks/`.

### --force

`sgf init --force` overwrites skeleton files with built-in defaults. `--force` does not re-run create-vite — it only affects SGF infrastructure files.

| File | `--force` behavior |
|------|--------------------|
| `.pre-commit-config.yaml` | Rebuilt from scratch (existing hooks replaced with built-in defaults) |
| `.claude/settings.json` | Deny rules and sandbox settings merged additively (same as normal init) |
| `.gitignore` | Entries re-appended if missing (same as normal init) |
| `CLAUDE.md` | Symlink recreated only if missing or broken |
| `AGENTS.md` | Never overwritten (user-authored content) |
| `.sgf/`, `.pensa/`, `.forma/` directories | Created if missing (never deleted) |
| Frontend files (`package.json`, etc.) | Not touched — `--force` never re-runs create-vite |

Safety checks:
- Fails if any target file has uncommitted changes or is untracked by git.
- Lists files to be overwritten and requires `y` confirmation before proceeding.

Config merges (`.gitignore`, `.claude/settings.json`) are unaffected by `--force` — they always use additive merge logic.

### --no-fe

`sgf init --no-fe` skips frontend scaffolding (the `pnpm create vite@latest` step). All SGF infrastructure is still created. Use this for backend-only projects.

## Prompt Delivery

sgf does not assemble, transform, or preprocess prompt files. Prompts in `.sgf/prompts/` are final — passed directly to `cl`.

### What sgf Does

1. **Resolve prompt** — find `.sgf/prompts/<stage>.md` via layered lookup (local `./.sgf/prompts/` → global `~/.sgf/prompts/`). Fail with a clear error if not found in either location.
2. **Pass the raw path** — give `cl` the resolved prompt path directly (no intermediate files).

### System Prompt Injection

**Context files (MEMENTO, BACKPRESSURE):** `cl` handles injection for all modes — both interactive and automated. `cl` resolves each context file via layered `.sgf/` lookup, builds `--append-system-prompt "study @<file>;..."`, and forwards to the downstream binary. See [claude-wrapper spec](claude-wrapper.md).

**sgf does not inject context study args** (MEMENTO, BACKPRESSURE — those belong to `cl`). Sgf only passes the prompt path and cursus context (consumed summary files) to `cl`.

### Prompt Files

Prompts are plain markdown files with no variable substitution.

---

## Agent Invocation

### Agent Invocation

sgf invokes `cl` (claude-wrapper) directly for all modes. It never calls `claude` or `claude-wrapper-secret`. Context file injection (MEMENTO.md, BACKPRESSURE.md) is handled by `cl` — sgf does not manage these files.

When `SGF_AGENT_COMMAND` is set (testing mode), the command override replaces `cl` — used for integration tests with mock scripts.

### cl Flags by Mode

**Interactive mode** (default):

```
cl \
  --verbose \
  --dangerously-skip-permissions \
  [--session-id <uuid>]           # always (fresh UUID per invocation)
  [--append-system-prompt '<consumed context>']
  @<PROMPT_FILE>
```

Spawns with full terminal passthrough (stdin/stdout/stderr inherited). No `setsid()` — the agent stays in sgf's process group for natural signal delivery.

**AFK mode** (`-a`):

```
cl \
  --verbose \
  --print \
  --output-format stream-json \
  --dangerously-skip-permissions \
  [--session-id <uuid>]           # always (fresh UUID per invocation)
  [--append-system-prompt '<consumed context>']
  @<PROMPT_FILE>
```

Spawns via `ChildGuard::spawn()` (which calls `setpgid(0, 0)` in `pre_exec` for process group isolation) with piped stdout, `Stdio::null()` for stdin, and inherited stderr. Stdout is read line-by-line via `BufRead`, parsed as NDJSON, and formatted with ANSI-styled output.

**Programmatic mode** (piped stdin or `--output-format json`):

```
cl \
  --verbose \
  --print \
  --output-format json \
  --dangerously-skip-permissions \
  [--session-id <uuid>]           # always (fresh UUID per invocation)
  [--resume <session_id>]         # when resuming a previous turn
  [--append-system-prompt '<consumed context>']
  @<PROMPT_FILE>                   # only on first turn (not on --resume)
```

Spawns with piped stdout and piped stdin (the outer agent's message is written to stdin). The `cl` process runs one turn: it reads the input, the inner agent processes it and responds, then `cl` exits. sgf captures the JSON output, wraps it with cursus metadata (current iter, iteration, session_id), and emits structured NDJSON events on its own stdout.

Programmatic mode is activated when:
1. `isatty(stdin) == false` (stdin is piped — automatic detection), OR
2. `--output-format json` is passed explicitly

On `--resume`, the outer agent's stdin message is passed as input to the resumed conversation. The prompt file is not re-sent (the conversation already has it from the first turn).

### Post-Result Timeout (AFK Mode)

In AFK mode, after the agent emits a result event (NDJSON `result` or `usage` message), sgf starts a 30-second countdown. If the agent process does not exit within 30 seconds, sgf kills it via the `ChildGuard` (which calls `kill_process_group()`). This prevents hung agent processes from blocking the iteration loop indefinitely.

The timeout is stored in `IterRunnerConfig.post_result_timeout` (default: `Duration::from_secs(30)`). The timer starts on whichever event arrives first — `result` or `usage`. A single successful check resets nothing; the timeout is one-shot per invocation.

This timeout only applies to AFK mode. Interactive mode inherits stdio and has no NDJSON parsing, so there is no result event to trigger the countdown. Programmatic mode uses a similar timeout mechanism.

### Execution Model

| Mode | Execution | Description |
|------|-----------|-------------|
| `interactive` | `cl` directly | Full terminal passthrough; calls `cl --verbose --dangerously-skip-permissions [--session-id UUID] [--append-system-prompt ...] @{prompt_path}`, inheriting stdio |
| `afk` | `cl` via iteration runner | Autonomous execution; `cl` invoked with `--dangerously-skip-permissions`, NDJSON stream formatting |
| `programmatic` | `cl` via programmatic runner | Turn-by-turn agent-driven execution; `cl` invoked with `--output-format json`, structured NDJSON events emitted on stdout |

**Interactive mode**: Calls `cl` directly with `--dangerously-skip-permissions`. No PID file, no log tee. Generates a loop_id and writes session metadata to `.sgf/run/{loop_id}.json` for resume capability. `cl` handles context file injection (MEMENTO, BACKPRESSURE). When `auto_push` is true, auto-pushes after the session if HEAD changed. Passes `--session-id <uuid>` to `cl` for session tracking.

**AFK mode**: Calls `cl` directly via the iteration runner. PID file, log tee, and loop ID are managed by sgf. Session metadata (`.sgf/run/{loop_id}.json`) is written before spawn and updated on exit.

**Programmatic mode**: Calls `cl` with `--output-format json` and piped stdin/stdout. Each invocation runs one turn: sgf sends the outer agent's message as stdin, captures the JSON response, wraps it with cursus metadata, and emits structured NDJSON events on stdout. The outer agent reads these events and decides whether to send another message (via a new `sgf --resume <run-id>` invocation) or stop. Session metadata and run state are persisted between invocations so `--resume` can restore the full pipeline state.

#### Session Metadata

For all modes, sgf generates a fresh session UUID before each `cl` invocation and writes session metadata. The metadata includes the session ID, loop config, and status. On exit, the status is updated based on exit code (`completed`, `interrupted`, `exhausted`). See [session-resume spec](session-resume.md) for the full schema.

#### Auto-push for interactive commands

Interactive commands with `auto_push = true` auto-push after the Claude session exits using `vcs_utils::auto_push_if_changed()` from the shared [vcs-utils](vcs-utils.md) crate: capture `vcs_utils::git_head()` before the session, then call `auto_push_if_changed()` after. Push failures are non-fatal (logged as warnings). Suppressed with `--no-push`.

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|-----|
| `0` | Sentinel found (`.iter-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

Interrupt handling uses the shared `shutdown` crate's `ShutdownController` (see [shutdown spec](shutdown.md)). The controller configuration depends on the mode:

**AFK mode** (`sgf build -a`, `sgf verify -a`, etc.): sgf spawns `cl` via `ChildGuard::spawn()` with `Stdio::null()` for stdin. Stdin isolation prevents the agent from inheriting the terminal fd and modifying terminal settings (e.g., disabling ISIG via `tcsetattr`), which would cause Ctrl+C/Ctrl+D to emit raw bytes instead of generating signals/EOF. The controller is created with `monitor_stdin: true` — stdin is free since no user interaction occurs. Both double Ctrl+C (SIGINT) and double Ctrl+D (stdin EOF) trigger shutdown. First press prints "Press Ctrl-C again to exit" (or "Press Ctrl-D again to exit") to stderr. Second press of the same key within 2 seconds: the `ChildGuard` is dropped, killing the process group via `kill_process_group(pid, 200ms)`, exit code 130. Timeout resets the counter. SIGTERM always triggers immediate shutdown (single signal).

**Non-AFK mode** (`sgf build`, `sgf verify`, etc.): sgf spawns `cl` **without** `setsid()` — `cl` and the agent stay in sgf's process group, receiving terminal signals naturally and retaining full terminal access. The controller is created with `monitor_stdin: false` — stdin belongs to the child for user interaction with Claude. Only double Ctrl+C works for shutdown; Ctrl+D goes to Claude as normal input. Both sgf and the child receive SIGINT on Ctrl+C; sgf's handler prints the confirmation prompt while Claude handles the signal with its own logic.

**Programmatic mode**: sgf spawns `cl` with piped stdin and stdout. The controller is created with `monitor_stdin: false` — stdin is managed by the outer agent's piped input. Ctrl+C/Ctrl+D are not applicable (no terminal). SIGTERM triggers immediate shutdown.

**Interactive stages** (`sgf spec`, `sgf issues log`): Same as non-AFK — no `setsid()`, `monitor_stdin: false`. The user types directly into Claude.

Signal handlers are registered just before spawning the child — during pre-launch checks, daemon startup, and other steps before handler registration, default signal behavior applies (single SIGINT exits).

Agent process failures trigger auto-retry (see Error Handling). Retryable failures resume the crashed session via `cl --resume <session_id>`.

### Completion Sentinel

The agent creates an `.iter-complete` file when the task is complete (e.g., `pn ready` returns no tasks). sgf checks for this file after each iteration. If found, sgf deletes it, performs a final auto-push (if enabled), and exits with code `0`.

### Iteration Loop

Before the loop:
- Verify `cl` is in PATH (unless `SGF_AGENT_COMMAND` is set)
- Search for and delete any stale `.iter-complete` sentinel file (from a previous crashed/killed run), searching recursively up to depth 2
- Delete stale `.iter-ding` sentinel file if present
- Save terminal settings (`tcgetattr`)

For each iteration `i` in `1..=iterations`:

1. Remove any stale `.iter-complete` sentinel
2. Print iteration banner (includes loop ID if provided)
3. Record `vcs_utils::git_head()` as `head_before`
4. Execute agent via `cl` (or `SGF_AGENT_COMMAND` override):
   - **Session ID handling**: Each invocation receives a fresh `--session-id <uuid>`.
   - **Resume handling**: If `--resume` is provided from the CLI, it only applies on the first invocation.
   - Interactive: start notification watcher thread, `.status()` with inherited stdio, stop watcher thread
   - AFK: `ChildGuard::spawn()` with piped stdout, read lines via reader thread + channel through `format_line()`
   - Programmatic: spawn with piped stdin/stdout, write outer agent's message to stdin, read JSON response, emit structured NDJSON events
5. Restore terminal settings (`tcsetattr`)
6. If interrupted: log warning, exit 130
7. If agent process failed (retryable): trigger auto-retry (see Error Handling)
8. Search for `.iter-complete` recursively (depth <= 2): if found, delete it, print completion banner, auto-push, exit 0
9. Print "Iteration N complete, continuing..."
10. Sleep 2 seconds (interruptible, polled in 100ms increments)
11. If interrupted: log warning, exit 130
12. If `auto_push`: call `vcs_utils::auto_push_if_changed()`

After loop: search for and delete sentinel files (depth <= 2), print max iterations banner, print resume command, exit 2.

### Iteration Clamping

Iterations are clamped to a hard limit of 1000. If a higher value is provided (via `-n` or cursus TOML), sgf logs a warning and clamps to 1000.

### Inter-Iteration Sleep

A 2-second sleep between iterations allows git operations to settle and prevents rapid-fire agent invocations. The sleep is interruptible — polled in 100ms increments, checking the shutdown controller between polls.

## NDJSON Stream Format

The iteration runner parses Claude Code's NDJSON output (`cl --output-format stream-json`) line by line. Each line is a JSON object with a `type` field that determines the event kind. The parser is forward-compatible — unknown event types and malformed lines are skipped silently.

Lines not starting with `{` are skipped without logging (expected verbose debug output from `cl`). Lines starting with `{` that fail to parse are logged at debug level and skipped.

### Event Types

| `type` | Structure | Parsed as |
|--------|-----------|-----------|
| `assistant` | `{"type":"assistant","message":{"content":[...]}}` | Text output or tool call summaries |
| `user` | `{"type":"user","message":{"content":[...]}}` | Tool results |
| `result` | `{"type":"result","result":"...","session_id":"...","usage":{...}}` | Final result text or usage stats |
| `system` | `{"type":"system"}` | Skipped |
| Unknown | Any other `type` value | Skipped |

### Content Blocks (`assistant` events)

The `message.content` array contains blocks with a `type` field:

| Block `type` | Fields | Display |
|--------------|--------|---------|
| `text` | `text: string` | Text output (multiple text blocks joined with newline) |
| `tool_use` | `name: string`, `input: object` | Tool name + summarized input (see below) |

If an assistant message contains both text and tool_use blocks, only the tool calls are displayed (tool calls take precedence).

### Tool Call Summaries

Each tool call is formatted with the tool name and a one-line summary extracted from the input:

| Tool | Summary shows |
|------|---------------|
| `Read` | `file_path [offset:limit]` |
| `Edit`, `Write` | `file_path` |
| `Bash` | `command` (truncated to 100 chars) |
| `Glob` | `pattern` |
| `Grep` | `pattern` |
| `TodoWrite` | `N items` |
| Other | First string value from the input object (truncated to 80 chars) |

### Content Blocks (`user` events)

The `message.content` array contains blocks with a `type` field:

| Block `type` | Fields | Display |
|--------------|--------|---------|
| `tool_result` | `content: string \| array \| null`, `is_error: bool` | Tool output lines (truncated to 15 lines, with count of truncated lines) |

Tool result `content` can be a plain string, an array of `{"type":"text","text":"..."}` objects, or null/absent.

### Result Events

| Field | Type | Description |
|-------|------|-------------|
| `result` | string | Final result text from the session |
| `session_id` | string (optional) | Claude Code session identifier |
| `usage` | object (optional) | Token usage: `{"input_tokens": N, "output_tokens": N}` |

When both `input_tokens` and `output_tokens` are present, the event is displayed as usage stats. Otherwise, the `result` text is displayed.

## Loop ID Format

`sgf` generates loop IDs with the pattern: `<stage>[-<spec>]-<YYYYMMDDTHHmmss>`

Examples:
- `build-auth-20260226T143000` (build loop for auth spec)
- `verify-20260226T150000` (verify loop, no spec filter)
- `issues-plan-20260226T160000` (issues plan loop)

sgf includes the loop ID in log output. `sgf logs` uses the loop ID to locate log files.

---

## Logging

`sgf` tees the agent's stdout to both the terminal and `.sgf/logs/<loop-id>.log`. The iteration runner owns formatting — in AFK mode it parses the NDJSON stream and emits human-readable one-liners (tool calls, text blocks); in interactive mode it passes through the terminal. `sgf` does not parse the agent's output in interactive mode.

The `.sgf/logs/` directory is gitignored.

### sgf logs

`sgf logs <loop-id>` runs `tail -f .sgf/logs/<loop-id>.log`. If the log file does not exist, print an error and exit 1.

---

## Console Output

sgf uses a rounded-box badge for all status output to stderr. Every message is wrapped in a 3-line box drawn with Unicode box-drawing characters (\`╭╮╰╯│─\`). The \`sgf\` label appears on the middle line in bold. The box borders are dim. Message text sits to the right of the box on the middle line — its color conveys semantic state.

### Visual Format

Each message gets its own 3-line box. The box is always 7 characters wide (\`╭─────╮\`). The \`sgf\` label is centered inside on the middle line in bold. The message text appears to the right of the closing \`│\` on the middle line.

\`\`\`
╭─────╮
│ sgf │ launching iteration runner [build-20260312T143000]
╰─────╯ iterations: 10 · mode: afk
╭─────╮
│ sgf │ recovering from stale state...
╰─────╯
╭─────╮
│ sgf │ recovery complete
╰─────╯
╭─────╮
│ sgf │ starting pensa daemon...
╰─────╯
╭─────╮
│ sgf │ pensa daemon ready
╰─────╯
╭─────╮
│ sgf │ pn export ok
╰─────╯
╭─────╮
│ sgf │ fm export ok
╰─────╯
╭─────╮
│ sgf │ pushing → origin/main...
╰─────╯
╭─────╮
│ sgf │ loop complete [build-20260312T143000]
╰─────╯
╭─────╮
│ sgf │ agent exited with error [build-20260312T143000]
╰─────╯
╭─────╮
│ sgf │ iterations exhausted [build-20260312T143000]
╰─────╯
\`\`\`

### Color Scheme

| State | Message Color | When |
|-------|---------------|------|
| Action | White | In-progress operations: launching, recovering, pushing, starting daemon |
| Success | Green | Completed operations: recovery complete, daemon ready, pn export ok, fm export ok, loop complete, pushed |
| Warning | Yellow | Non-fatal issues: pn export skipped, fm export skipped, pn doctor failed, iterations exhausted |
| Error | Red | Fatal failures: agent exited with error, pn export failed, fm export failed |
| Detail | Dim (gray) | Supplementary info: iterations, mode (below box, no badge) |

The box borders (\`╭─────╮\`, \`│\`, \`╰─────╯\`) are always **dim**. The \`sgf\` text inside the box is always **bold** (\`\x1b[1m sgf \x1b[0m\`) — normal text color regardless of message state.

### Box Construction

The badge box is 3 lines emitted to stderr:

1. **Top**: \`dim(╭─────╮)\`
2. **Middle**: \`dim(│) bold( sgf ) dim(│)\` + space + colored message
3. **Bottom**: \`dim(╰─────╯)\` + optional detail text

The box is stateless — each semantic output call (\`print_action\`, \`print_success\`, etc.) emits its own complete 3-line box. No buffering or grouping.

### Detail Lines

Detail lines appear on the bottom line of the box, to the right of \`╰─────╯\`, aligned with the message text on the middle line (8 characters: 7-char box width + 1-space gap). They are rendered in dim gray.

Detail lines appear for:
- **Iteration runner launch**: \`iterations: <n> · mode: <afk|interactive>\`
- **Interactive launch**: \`mode: interactive\`

### NO_COLOR Support

When the \`NO_COLOR\` environment variable is set, all ANSI codes and box-drawing characters are suppressed. The badge falls back to plain \`sgf:\` prefix. Detail lines are indented with plain spaces. Message text has no color formatting.

\`\`\`
sgf: launching iteration runner [build-20260312T143000]
     iterations: 30
sgf: recovery complete
sgf: agent exited with error [build-20260312T143000]
\`\`\`

### style.rs Module

\`crates/springfield/src/style.rs\` provides styling primitives and semantic output functions. Provides ANSI primitives and sgf-specific badge box and message functions.

**ANSI Primitives**:
- \`bold(s)\`, \`dim(s)\`, \`green(s)\`, \`yellow(s)\`, \`red(s)\`, \`white(s)\`
- \`no_color()\` — checks \`NO_COLOR\` environment variable
- \`strip_ansi(s)\` — removes ANSI escape sequences

**Badge Box**:
- \`badge_top()\` — returns the top border: \`dim(╭─────╮)\`
- \`badge_mid()\` — returns the middle line badge: \`dim(│) bold( sgf ) dim(│)\`
- \`badge_bot()\` — returns the bottom border: \`dim(╰─────╯)\`

**Semantic Output** (all write to stderr via 3-line box):
- \`action(msg)\` — box + bold white message
- \`success(msg)\` — box + bold green message
- \`warning(msg)\` — box + bold yellow message
- \`error(msg)\` — box + bold red message
- \`detail(msg)\` — indented dim message, no box (appended to bottom line of preceding box)

### Auto-push Callback

The \`vcs_utils::auto_push_if_changed()\` callback emits raw messages (e.g., \`"New commits detected, pushing..."\`, \`"push failed (non-fatal): ..."\`). The callback in \`orchestrate.rs\` wraps these with the appropriate styled output function — action for "pushing", warning for "push failed".

### Message Catalog

Every \`eprintln\\\!("sgf: ...")\` and \`println\\\!(...)\` call in the springfield crate is replaced with a styled output call.

| Message | Style | Source |
|---------|-------|--------|
| recovering from stale state... | action | recovery.rs |
| recovery complete | success | recovery.rs |
| pn doctor --fix exited with {status} | warning | recovery.rs |
| pn doctor --fix failed: {e} | warning | recovery.rs |
| starting pensa daemon on port {port}... | action | recovery.rs |
| starting forma daemon on port {port}... | action | recovery.rs |
| pensa daemon ready | success | recovery.rs |
| forma daemon ready | success | recovery.rs |
| pn export ok | success | recovery.rs |
| pn export failed: {err} | error | recovery.rs |
| pn export skipped (pn not found: {e}) | warning | recovery.rs |
| fm export ok | success | recovery.rs |
| fm export failed: {err} | error | recovery.rs |
| fm export skipped (fm not found: {e}) | warning | recovery.rs |
| launching interactive session | action | orchestrate.rs |
| launching iteration runner [{loop_id}] | action | orchestrate.rs |
| loop complete [{loop_id}] | success | orchestrate.rs |
| agent exited with error [{loop_id}] | error | orchestrate.rs |
| iterations exhausted [{loop_id}] | warning | orchestrate.rs |
| interrupted [{loop_id}] | warning | orchestrate.rs |
| agent exited with unexpected code [{loop_id}] | error | orchestrate.rs |
| New commits detected, pushing... | action | orchestrate.rs (auto-push callback) |
| push failed (non-fatal): {err} | warning | orchestrate.rs (auto-push callback) |
| .sgf/MEMENTO.md not found — agents won't have fm/pn workflow reference | warning | init.rs |
| .sgf/BACKPRESSURE.md not found — agents won't have build/test/lint reference | warning | init.rs |
| project scaffolded successfully | success | init.rs |
| {stage}: {e} | error | main.rs |

---

## Recovery

The iteration runner does not perform iteration-start cleanup. Recovery is `sgf`'s responsibility, executed before invoking `cl`.

### PID Files

`sgf` writes `.sgf/run/<loop-id>.pid` on launch (containing its process ID) and removes it on clean exit. The `.sgf/run/` directory is gitignored.

Cursus runs use a separate PID file layout: `.sgf/run/<run-id>/<run-id>.pid` (nested inside the run directory). Cursus has its own stale run detection via `meta.json` status — the flat PID scan described below applies only to non-cursus sessions.

### Pre-launch Cleanup

Before invoking `cl`, `sgf` scans PID files at `.sgf/run/*.pid` (non-cursus sessions only):

- **Any PID alive** (verified via `kill -0`) → another loop is running. Skip cleanup and launch normally — the dirty tree or in-progress claims may belong to that loop.
- **All PIDs stale** (process dead) → no loops are running. Remove stale PID files, then recover:
  1. `git checkout -- .` — discard modifications to tracked files. **Failure is fatal** — loop launch is aborted.
  2. `git clean -fd` — remove untracked files (respects `.gitignore`, so `db.sqlite` and logs are safe). **Failure is fatal** — loop launch is aborted.
  3. `pn doctor --fix` — release stale claims and repair integrity (warning only — supplementary, not critical for state consistency)

**Principle**: Work is only preserved when committed. Uncommitted changes from crashed iterations are discarded — the agent that produced them is gone and cannot continue them. Git recovery failures are hard errors that prevent loop launch — proceeding with dirty state would violate the atomic iteration guarantee.

---

## Pre-launch Lifecycle

Before launching any loop, `sgf` runs pre-launch checks. The checks vary by stage:

**All stages** (build, verify, test-plan, test, spec, issues log):

1. **Recovery** — clean up stale state from crashed iterations (see Recovery)
2. **Daemons** — start the pensa and forma daemons if not already running
3. **Data export** — run `pn export` and `fm export` to sync SQLite → JSONL before the agent starts

After pre-launch checks, automated stages invoke `cl` via the iteration runner; interactive stages call `cl` directly with `--verbose @{prompt_path}`, inheriting stdio.

**`SGF_SKIP_PREFLIGHT`** (env var) — When set, skips daemon startup and data export while still running recovery. This allows two-tier control: the `--skip-preflight` CLI flag disables all pre-launch checks (including recovery), while `SGF_SKIP_PREFLIGHT` disables only the infrastructure checks (daemons, export). Used by integration tests to exercise recovery logic without requiring a running pensa daemon.

### Daemons

`sgf` starts both the pensa and forma daemons automatically before launching any loop (if not already running):

#### Port derivation

Each daemon uses its own port derivation to avoid collisions:

- **Pensa**: `SHA256(canonical_project_path)`, bytes [8,9] mapped to range [10000, 59999]
- **Forma**: `SHA256("forma:" + canonical_project_path)`, bytes [8,9] mapped to range [10000, 59999]

The `"forma:"` prefix ensures forma and pensa derive different ports for the same project. The authoritative port derivation functions are `project_port()` in each daemon's `db.rs` (`pensa/src/db.rs` and `forma/src/db.rs`). Springfield's `pensa_port()` and `forma_port()` in `recovery.rs` replicate this logic and must stay in sync — a mismatch causes silent daemon startup failures where sgf starts a daemon on a different port than the CLI expects.

#### Pensa daemon

1. Check if the daemon is reachable (`pn daemon status`)
2. If not, start it: `pn daemon --project-dir <project-root> --port <pensa-derived> &` (backgrounded)
3. Wait for readiness (poll `pn daemon status` with exponential backoff: 50ms initial, doubling up to 800ms cap, 5s total deadline)

#### Forma daemon

1. Check if the daemon is reachable (`fm daemon status`)
2. If not, start it: `fm daemon --project-dir <project-root> --port <forma-derived> &` (backgrounded)
3. Wait for readiness (poll `fm daemon status` with exponential backoff: 50ms initial, doubling up to 800ms cap, 5s total deadline)

Both daemons are started in parallel. Both must be ready before proceeding with loop launch. The daemons run for the duration of the `sgf` session. They stop on SIGTERM or when `sgf` shuts down.

### Data Export

After daemons are ready, `sgf` runs `pn export` and `fm export` to sync each daemon's SQLite database to the committed JSONL files. This ensures the JSONL artifacts reflect the latest state before the agent begins work. Both exports are non-fatal — failures are logged as warnings (`pn export failed`, `fm export failed`) and do not block loop launch. If a binary is not found, the export is skipped with a warning.

---

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready` returns nothing (all tasks done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., multiple `sgf build` instances) can run concurrently on the same branch. The pensa daemon serializes all database access, providing atomic claims via `pn update --claim` (fails with `already_claimed` if another agent got there first). `pn export` runs at commit time via the pre-commit hook. Concurrent agents share the same filesystem and git history. **Stop build loops before running `sgf spec`** to avoid task-supersession race conditions.

### Standard Loop Iteration

Build, Test, and Issues Plan stages share a common iteration pattern. Each iteration:

1. **Orient** — context files (MEMENTO, BACKPRESSURE) are injected by `cl` via `study` instructions. Agents fetch spec content via prompt instructions (e.g., `fm show <stem> --json`).
2. **Query** — find work items via pensa (stage-specific query). If none, write `.iter-complete` and exit.
3. **Choose & Claim** — pick a task from the results, then `pn update <id> --claim`. If the claim fails (`already_claimed`), re-query and pick another.
4. **Work** — stage-specific implementation
5. **Log issues** — if problems are discovered: `pn create "description" -t bug`
6. **Close/release** — close or release the work item
7. **Commit** — prefix the commit message with `[<task-id>]` (e.g., `[pn-a1b2c3d4] Implement login validation`). The pre-commit hook runs `pn export` automatically, syncing SQLite to JSONL. The prefix enables `git log --grep` for per-task history.

Each iteration gets fresh context. The pensa database persists state between iterations.

| Stage | Query | Work | Close |
|-------|-------|------|-------|
| Build | `pn ready [--spec <stem>] --json` | Implement the task (or plan the bug — see below); apply backpressure | `pn close <id> --reason "..."` (tasks) / `pn release <id>` (bugs) |
| Test | `pn ready -t test [--spec <stem>] --json` | Execute the test | `pn close <id> --reason "..."` |

#### Bug Handling in the Build Loop

`pn ready` includes unplanned bugs (see pensa spec). When the build loop claims a bug, the agent studies the codebase then decides how to proceed:

**Small bugs (fixable in this iteration):** Fix it directly — implement, test, apply backpressure, and close the bug. Treat it like a normal task.

**Large bugs (multiple files/crates, significant refactor):** Decompose into fix tasks:

1. Create fix task(s): `pn create -t task "fix: <description>" --fixes <bug-id> [--spec <stem>] [-p <priority>] [--dep <id>]`
2. Comment lessons learned on the bug: `pn comment add <bug-id> "..."`
3. Release the bug: `pn release <bug-id>` (the bug drops out of `pn ready` — it now has fix children)
4. Commit with `[<bug-id>]` prefix

The fix tasks appear in subsequent `pn ready` calls and are implemented as normal tasks. When all fix tasks for a bug are closed, pensa auto-closes the bug.

### 1. Spec (`sgf spec`)

Multi-iter cursus pipeline. The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates deliverables:

1. Create or update specs via `fm` (Spec Create and/or Spec Update Workflow from MEMENTO)
2. Create implementation plan items via `pn create -t task --spec <stem>`, with dependencies and priorities
3. Commit and push

The interview and generation happen across cursus iters (discuss → draft → review → approve). The prompts instruct the agent to design specs so the result can be end-to-end tested from the command line.

Tasks linked to a spec *are* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Spec revision**: Run `sgf spec` again. **Stop any running build loops before revising specs.** When revising, the agent:
1. Reviews existing tasks for the spec: `pn list --spec <stem> --json`
2. Closes tasks that are no longer relevant: `pn close <id> --reason "superseded by revised spec"`
3. Creates new tasks for the delta: `pn create "..." -t task --spec <stem>`
4. Updates the spec via `fm`
5. Restart build loops after revision is committed

### 2. Build (`sgf build`)

Follows the standard loop iteration. Uses `.sgf/prompts/build.md`. The prompt instructs the agent to fetch relevant specs via `fm show` as needed.

The build stage adds **backpressure** — after implementing the task, the agent runs build, test, and lint commands per `BACKPRESSURE.md`.

Run interactively first for a few supervised rounds, then switch to AFK mode (`-a`) for autonomous execution.

### 3. Verify (`sgf verify`)

Uses `.sgf/prompts/verify.md`. Each iteration handles one spec:

1. List all specs via `fm list --json`
2. Pick one unverified spec and investigate it against the codebase (read via `fm show <stem> --json`)
3. Mark conformance: ✅ Matches spec, ⚠️ Partial match, ❌ Missing/different
4. Update `verification-report.md`
5. Log any gaps as pensa bugs: `pn create "..." -t bug`
6. Commit

When all specs have been verified, write `.iter-complete`.

### 4. Test Plan (`sgf test-plan`)

Uses `.sgf/prompts/test-plan.md`. The agent:

1. Studies specs and codebase
2. Generates a testing plan
3. Ensures tests are automatable (can be run by agents in loops)
4. Creates test items via `pn create -t test --spec <stem>`, with dependencies and priorities
5. Commits

### 5. Test (`sgf test`)

Follows the standard loop iteration. Uses `.sgf/prompts/test.md`. The prompt instructs the agent to fetch relevant specs via `fm show` as needed.

After all test items are closed, a final iteration generates `test-report.md` — a summary of all test results, pass/fail status, and any bugs logged.

### 6. Issues Log (`sgf issues-log`)

Calls `cl` directly using `.sgf/prompts/issues-log.md`. Each session handles one bug:

1. The developer describes a bug they've observed
2. The agent interviews them to capture details — steps to reproduce, expected vs actual behavior, relevant context
3. Logs the bug via `pn create -t bug`

One bug per session. The developer runs `sgf issues-log` again for additional bugs — fresh context each time prevents accumulation across unrelated issues.

### 7. Doc (`sgf doc`)

Calls `cl` directly using `.sgf/prompts/doc.md`. Runs `pn doctor --json` and triages the results:

1. Run `pn doctor --json`
2. For each reported issue, investigate whether it has been completed or is still valid
3. Comment pertinent findings on affected issues
4. Close any completed or invalid issues

Auto-pushes after the session if HEAD changed (like `sgf spec`). Suppressed with `--no-push`.

### 8. Inline Issue Logging

Issues are also logged by agents during any stage via `pn create`. The build loop logs bugs it discovers during implementation. The verify loop logs spec gaps. The test loop logs test failures. `sgf issues-log` is for developer-reported bugs; inline logging is for agent-discovered bugs.

---

## Shipped Prompts

Each command has a corresponding prompt file. The defaults live in `~/.sgf/prompts/` (synced from the springfield repo's `.sgf/prompts/` via `just install`). Override any prompt per-project by creating `./.sgf/prompts/<name>.md`.

| Prompt | Purpose |
|--------|---------|
| `spec.md` | Interactive spec discussion and implementation planning |
| `build.md` | Claim one pn issue, implement it, apply backpressure, commit |
| `verify.md` | Verify one spec against codebase, update verification report |
| `test-plan.md` | Generate test items from specs using pn |
| `test.md` | Claim one pn test item, execute it, apply backpressure |
| `issues-log.md` | Interactive bug reporting session |
| `doc.md` | Interactive pensa doctor triage |

The canonical prompts live in the springfield repo's `.sgf/prompts/` — do not duplicate their contents here.

### Custom Prompts

Users can add custom prompts by creating a new `.md` file in `./.sgf/prompts/` (project-local) or `~/.sgf/prompts/` (global) and a corresponding cursus TOML in `.sgf/cursus/`. For example, adding `deploy.md` and `deploy.toml` enables `sgf deploy`.

---

## Backpressure

`BACKPRESSURE.md` lives in the springfield repo's `.sgf/` directory and is synced to `~/.sgf/` via `just install`. It contains universal build, test, lint, and format commands for common project types. The developer deletes sections that don't apply to their project by creating a project-local override in `./.sgf/BACKPRESSURE.md`.

---

## Defaults

Per-command defaults are defined in cursus TOML files (see [cursus spec](cursus.md)). CLI flags override cursus TOML values:

| Setting | Fallback Default | Override |
|---------|-----------------|----------|
| Mode | `interactive` | `-a` / `-i` flags |
| Iterations | `1` | `-n` / `--iterations` |
| Auto-push | `false` | `--no-push` flag (disables), cursus TOML `auto_push` field |
| Pensa daemon port | per-project derived (`SHA256(path)`) | `--port` flag on `pn daemon` |
| Forma daemon port | per-project derived (`SHA256("forma:" + path)`) | `--port` flag on `fm daemon` |

---

## Key Design Principles

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt enforces: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**One task, fresh context**: Each iteration picks one unblocked task, implements it fully, commits, and exits. The loop restarts with a clean context window. No accumulated confusion, no multi-task sprawl.

**Atomic iterations**: An iteration either commits fully or is discarded entirely. Partial work from crashed iterations is never preserved — sgf's pre-launch recovery wipes uncommitted state before the next run.

**Structured memory over markdown**: Pensa replaces markdown-based issue logging and plan tracking. A single CLI command replaces the error-prone multi-step process of creating directories and writing files. `pn` is the exclusive task tracker — agents must never use TodoWrite, TaskCreate, or markdown files for tracking work.

**Tasks as implementation plan**: There is no separate "implementation plan" entity. The living set of pensa tasks linked to a spec *is* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Editable prompts**: Prompts are plain markdown files. Global defaults live in `~/.sgf/prompts/` (synced from the springfield repo). Override per-project by creating `./.sgf/prompts/<name>.md`. New commands are defined by creating a cursus TOML in `.sgf/cursus/` and a corresponding prompt file — no code changes required.

**Layered context injection**: `cl` (claude-wrapper) resolves context files (MEMENTO.md, BACKPRESSURE.md) via layered `.sgf/` lookup (local `./.sgf/` → global `~/.sgf/`) and injects them as `study` instructions into every Claude session. This applies uniformly to both interactive and automated stages. sgf does not inject context — it resolves prompt paths, then delegates to `cl`.

**Protected scaffolding**: `.sgf/` and `.claude/` are protected from agent writes via Claude deny settings. The developer is the authority on prompts, settings, and project configuration.

**Layered projects**: Springfield uses two-tier `.sgf/` resolution — project-local `./.sgf/` overrides global `~/.sgf/` on a file-by-file basis. Projects only need local overrides for project-specific customizations; everything else falls through to the global defaults.

**Direct execution with native sandbox**: All stages invoke `cl` on the host — no Docker sandboxes, no Mutagen sync. Claude Code's native sandbox (Seatbelt on macOS, bubblewrap on Linux) provides OS-level filesystem and network isolation, enabled by default via `.claude/settings.json`. Automated stages use `--dangerously-skip-permissions` — agents operate freely within sandbox bounds but cannot escape. Interactive stages use the sandbox with developer-controlled settings.

---


## Future Work

- **Context-efficient backpressure**: Swallow all build/test/lint output on success (show only a checkmark), dump full output only on failure. Preserves context window budget. See HumanLayer's `run_silent()` pattern.
- **Claude Code hooks for enforcement**: Use `PreToolUse` / `PostToolUse` hooks to enforce backpressure at the framework level — auto-run linters after file edits, block destructive commands. Could be scaffolded by `sgf init`.
- **TUI**: CLI-first for now. TUI can be added later as a view layer. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [forma](forma.md) — Specification management — forma daemon and fm CLI
- [pensa](pensa.md) — Agent persistent memory — SQLite-backed issue/task tracker with pn CLI
- [shutdown](shutdown.md) — Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts
- [vcs-utils](vcs-utils.md) — Shared VCS utilities — git HEAD detection, auto-push
