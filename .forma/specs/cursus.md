# cursus Specification

Pipeline orchestration — declarative TOML-defined multi-iter workflows with context passing, sentinel-based transitions, and stall recovery

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | stable |

## Overview

Cursus is the pipeline orchestration subsystem of sgf. It provides declarative TOML pipeline definitions for all commands.

A **cursus** (Latin: "a running, course, path") is a named pipeline comprising one or more **iters** (Latin: "journey, passage") — discrete execution stages that run sequentially. Each iter invokes a prompt via sgf's iteration runner (AFK) or `cl` (interactive), with sentinel files controlling transitions between iters.

Cursus provides:
- **Declarative pipeline definitions**: TOML files in `.sgf/cursus/` define the iter sequence, execution modes, iteration counts, and transition rules
- **Context passing between iters**: Each iter can produce a summary file; subsequent iters consume it via prompt injection
- **Sentinel-based transitions**: Well-known sentinel files signal success, rejection, revision, or exhaustion — controlling which iter runs next
- **Stall recovery**: When an iter exhausts its iterations, the pipeline enters a stalled state the user can inspect and resume
- **Programmatic mode**: When driven by an outer agent (piped stdin), cursus emits structured NDJSON events describing pipeline state — iter starts, turns, transitions, completions, and stalls. The outer agent drives interactive iters turn-by-turn via `--resume <run-id>`. Cursus TOML definitions are unchanged; `mode: "interactive"` means "this iter needs a conversation" regardless of whether a human or outer agent is conversing.
- **Auto-retry**: Automatic retry on agent process crashes with configurable immediate retries and backoff intervals. Resumes the crashed session automatically.
- **Structured event protocol**: In programmatic mode, all pipeline state changes are emitted as NDJSON events on stdout, enabling outer agents to track iter progression, make decisions, and drive the pipeline.
- **Unified command model**: Every `sgf <command>` resolves to a cursus definition, whether it has one iter or many. Single-iter cursus definitions are the standard way to define simple commands
- **Foundation for evolution**: The TOML format accommodates future trigger types (event-driven daemons, cursus chaining) without structural changes. Only manual triggers are supported initially

Cursus definitions follow the same layered resolution as prompts: project-local `./.sgf/cursus/` overrides global `~/.sgf/cursus/`.

## Architecture

Cursus is a module within the `springfield` crate. Pipeline definitions live in `.sgf/cursus/` as TOML files; runtime state lives in `.sgf/run/` as JSON. The module handles TOML parsing, iter sequencing, sentinel detection, context file management, and stall recovery.

## File Layout

```
~/.sgf/
  cursus/                      # global pipeline definitions (TOML)
    build.toml
    doc.toml
    incarnatur.toml
    install.toml
    precatur.toml
    spec-gen.toml
  prompts/                     # prompt content (markdown)
    build.md
    doc.md
    incarnatur-0-spec-to-issues.md
    incarnatur-1-spec-to-issues-final-pass.md
    incarnatur-2-build.md
    install.md
    precatur-0-gather-preces.md
    precatur-1-discuss-and-interview.md
    precatur-1-write.md
    precatur-2-review.md
    precatur-3-revise.md
    precatur-4-approve.md
    spec-gen-0-discuss-and-interview.md
    spec-gen-1-write.md
    spec-gen-2-review.md
    spec-gen-3-revise.md
    spec-gen-4-approve.md

.sgf/                          # per-project overrides
  cursus/                      # project-local pipeline overrides
    build.toml                 # overrides ~/.sgf/cursus/build.toml
  prompts/                     # project-local prompt overrides
    build.md
```

## Runtime State

```
.sgf/run/
  <run-id>/                    # per-run directory (gitignored)
    meta.json                  # run metadata: cursus name, current iter, status, timestamps
    context/                   # produced summary files
      discuss-summary.md
      draft-presentation.md
```

## Code Location

Cursus is implemented in the `springfield` crate at `crates/springfield/src/cursus/`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `toml` (0.8) | TOML parsing for cursus definitions |
| `serde` (1, derive) | Deserialization of TOML and JSON run state |
| `serde_json` (1) | Run state serialization/deserialization |
| `uuid` (1, v4) | Run ID and session ID generation |
| `chrono` (0.4) | Timestamps for run metadata |
| `tracing` (0.1) | Structured logging |

These are part of the `springfield` crate's Cargo.toml (cursus is a module within springfield, not a separate crate).

Binary invocations (child processes, not crate-level dependencies):

| Binary | Source | Purpose |
|--------|--------|---------|
| `cl` | crates/claude-wrapper/ | Agent invocation in both AFK and interactive modes |

Workspace crate dependencies (linked at compile time via springfield):

| Crate | Purpose |
|-------|---------|
| `vcs-utils` (workspace) | Git operations (auto-push) |
| `shutdown` (workspace) | Graceful shutdown handling, ChildGuard |

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Cursus TOML not found (neither local nor global) | Exit 1: `unknown command: <name>` |
| Cursus TOML parse error | Exit 1: `failed to parse cursus definition: <path>: <error>` |
| Validation failure (duplicate iter names, missing transition targets, etc.) | Exit 1: descriptive error at parse time |
| `[retry]` fields invalid types | Exit 1: descriptive parse error |
| `consumes` value has no matching `produces` in any iter | Warning at parse time (not an error — the producing iter may not have run yet on first pass) |
| Prompt file not found for an iter | Exit 1: `prompt not found: <path>` (checked at cursus load time, before execution starts) |
| Iter exhausts iterations (no sentinel found) | Pipeline enters stalled state. Run metadata updated. User notified with status, options, and resume command |
| Sentinel file `.iter-reject` with no `on_reject` transition defined | Exit 1: `iter '<name>' signaled reject but no on_reject transition is defined` |
| Sentinel file `.iter-revise` with no `on_revise` transition defined | Exit 1: `iter '<name>' signaled revise but no on_revise transition is defined` |
| Run directory creation failure | Exit 1: `failed to create run directory: <error>` |
| Run metadata read/write failure | `tracing::error\!`, continue if possible (non-fatal for execution, fatal for resume) |
| `produces` file not written by agent | `tracing::warn\!` — continue to next iter. The consuming iter will run without that context. Not fatal because the agent may have communicated through other means (spec updates, pn comments) |
| Stale run directory from previous crashed run | Detected at cursus startup: scan `.sgf/run/*/meta.json` for entries with `status: running`, check if PID file (`.sgf/run/<run-id>/<run-id>.pid`) exists and process is alive via `kill -0`. If PID is stale (process dead), update `meta.json` status to `interrupted`. This is separate from the non-cursus PID scan in springfield recovery (which handles flat `.sgf/run/*.pid` files) |
| SIGINT/SIGTERM during iter execution | Delegated to sgf/cl signal handling. Pipeline status updated to `interrupted` on exit. Resume command printed |
| Agent process crash (retryable) | Auto-retry triggered: 3 immediate retries, then backoff every 5 minutes for up to 12 hours. Resume crashed session on success. See [springfield spec](springfield.md) Error Handling |
| Agent process crash (non-retryable) | Startup failure (exit 1 within first seconds). No retry. Error event emitted in programmatic mode |
| `--resume` with invalid run-id | Exit 1: `run not found: <run-id>` |
| `--resume` with mismatched cursus name | Exit 1: `run <run-id> belongs to cursus '<name>', not '<requested>'` |

## Testing

### Unit Tests

#### `cursus/toml.rs`
- Parse valid single-iter cursus definition (e.g., `build.toml`)
- Parse valid multi-iter cursus definition with transitions (e.g., `spec.toml`)
- Parse cursus with `produces` and `consumes` fields
- Parse cursus with `banner` field (true, false, default)
- Parse cursus with `[retry]` section (all fields, partial fields, defaults)
- Reject duplicate iter names
- Reject transition targets that reference non-existent iters
- Reject `consumes` referencing non-existent `produces` keys
- Default values: `mode` defaults to `interactive`, `iterations` defaults to 1, `trigger` defaults to `manual`, `auto_push` defaults to false, `banner` defaults to false, retry defaults to 3/300/43200
- Alias validation: reject duplicate aliases, reject aliases that shadow cursus names

#### `cursus/runner.rs`
- Sentinel `.iter-complete` advances to next iter
- Sentinel `.iter-reject` follows `on_reject` transition
- Sentinel `.iter-revise` follows `on_revise` transition
- Missing sentinel with iterations exhausted enters stalled state
- Transition to a previous iter (back-edge) works correctly
- Final iter completion marks pipeline as completed
- `produces` file path is correctly constructed under run directory
- `consumes` files are correctly resolved and injected via `--append-system-prompt`
- Missing `produces` file emits warning but continues
- `banner` flag is passed to the iteration runner when true, omitted when false
- Iter with both `next` and `transitions` correctly uses transitions on reject/revise and `next` on complete

#### `cursus/state.rs`
- Run metadata serialization/deserialization roundtrip
- Status transitions: `running` → `completed`, `running` → `stalled`, `running` → `interrupted`
- Stale run detection (status `running` but PID not alive)
- Resume from stalled state restores correct iter position

#### `cursus/context.rs`
- `produces` file written to correct path: `.sgf/run/<run-id>/context/<key>.md`
- `consumes` resolves files from run context directory
- Multiple `consumes` entries are concatenated in order for prompt injection
- Missing consumed file returns empty string with warning

#### `cursus/events.rs` (new)
- Each event type serializes to valid JSON with correct `event` field
- Event ordering matches expected sequence for single-iter, multi-iter, and stall scenarios
- Events include correct iter, session_id, and run_id fields
- `turn` event includes `waiting_for_input` field
- `stall` event includes `actions` array
- `run_complete` event includes `resume_command`

### Integration Tests

Binary-level tests using `cargo test -p springfield`. Each test:
1. Creates a `tempfile::TempDir` with a cursus TOML and prompt files
2. Uses `SGF_AGENT_COMMAND` to mock agent execution
3. Verifies iter sequencing, sentinel transitions, and context file flow

| Test | Scenario | Asserts |
|------|----------|---------|
| Single-iter cursus | `build.toml` equivalent | Runs single iter, exits normally |
| Multi-iter happy path | discuss → draft → review → approve | All iters run in sequence, exit 0 |
| Reject transition | review signals `.iter-reject` | Pipeline jumps back to draft iter |
| Revise transition | review signals `.iter-revise` | Pipeline jumps to revise, then back to review |
| Context passing | discuss produces summary, draft consumes it | Summary content appears in draft's system prompt |
| Stall recovery | draft exhausts iterations | Pipeline enters stalled state, metadata persisted, resume command printed |
| Resume stalled | Load stalled run via `--resume` | Pipeline continues from stalled iter |
| Layered resolution | Local cursus overrides global | Local TOML takes precedence |
| Banner flag | iter with `banner = true` | Iteration runner displays banner |
| Programmatic events | Run with piped stdin | NDJSON events emitted in correct order on stdout |
| Programmatic turn-by-turn | Multi-turn interactive iter | Outer agent drives via stdin/`--resume`, receives turn events |
| Programmatic stall | Iter exhaustion with piped stdin | `stall` event emitted with actions |
| Programmatic AFK iter | AFK iter with piped stdin | Iter runs to completion, emits `iter_start` + `iter_complete` events |
| Retry config parsing | Cursus with `[retry]` section | Config values override defaults |
| Retry config defaults | Cursus without `[retry]` section | Default values (3/300/43200) used |
| Resume command on exit | All exit paths | Resume command printed to stderr or included in JSON events |

## TOML Format

## Cursus Definition

Each `.toml` file in `.sgf/cursus/` defines one cursus. The filename (minus `.toml`) is the cursus name and the `sgf` subcommand.

### Top-Level Fields

```toml
description = "Spec creation and refinement"
alias = "s"
trigger = "manual"
auto_push = true

[retry]
immediate = 3
interval_secs = 300
max_duration_secs = 43200
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | required | Human-readable description of the cursus. Used by `sgf list` |
| `alias` | string | — | Short alias for the command (e.g., `"s"` for spec). Optional |
| `trigger` | string | `"manual"` | How the cursus is started. Only `"manual"` is supported initially |
| `auto_push` | bool | `false` | Auto-push after commits (applies to all iters unless overridden) |

### Retry Configuration

The optional `[retry]` table configures auto-retry behavior for agent process failures (API rate limits, network errors, crashes). If omitted, defaults apply.

```toml
[retry]
immediate = 3           # immediate retry attempts before backoff (default: 3)
interval_secs = 300     # backoff interval in seconds (default: 300 = 5 minutes)
max_duration_secs = 43200  # max total retry duration in seconds (default: 43200 = 12 hours)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `immediate` | u32 | `3` | Number of immediate retry attempts (no delay) before switching to backoff |
| `interval_secs` | u64 | `300` | Seconds between backoff retries |
| `max_duration_secs` | u64 | `43200` | Maximum total time (seconds) to keep retrying. After this, sgf exits with an error |

See [springfield spec](springfield.md) Error Handling for the full retry strategy and failure classification.

### Iter Definition

Iters are defined as an array of tables using `[[iter]]` (singular):

```toml
[[iter]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
iterations = 1
produces = "discuss-summary"
consumes = []
auto_push = false
banner = false

[[iter]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
produces = "draft-presentation"
consumes = ["discuss-summary"]
auto_push = true
banner = true

[[iter]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iter.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iter]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iter]]
name = "approve"
prompt = "spec-approve.md"
mode = "interactive"
consumes = ["draft-presentation"]
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Unique identifier for this iter within the cursus |
| `prompt` | string | required | Prompt file name, resolved via layered `.sgf/prompts/` lookup |
| `mode` | `"interactive"` or `"afk"` | `"interactive"` | Execution mode. In programmatic mode (piped stdin), `"interactive"` iters are driven turn-by-turn by the outer agent; `"afk"` iters run to completion internally |
| `iterations` | u32 | `1` | Max iterations for this iter (only meaningful for `afk` mode) |
| `produces` | string | — | Key name for the summary file this iter writes. Stored at `.sgf/run/<run-id>/context/<key>.md` |
| `consumes` | array of strings | `[]` | Keys of summary files from previous iters, injected into this iter's system prompt |
| `auto_push` | bool | cursus-level default | Override auto-push for this specific iter |
| `banner` | bool | `false` | Whether the iteration runner displays the ASCII art startup banner. Default off |
| `next` | string | — | Override: after completion, go to this iter instead of the next in the list |
| `transitions` | table | — | Named transition overrides triggered by sentinel files |

### Transitions Table

```toml
[iter.transitions]
on_reject = "draft"
on_revise = "revise"
```

| Field | Sentinel File | Description |
|-------|---------------|-------------|
| `on_reject` | `.iter-reject` | Jump to named iter on rejection |
| `on_revise` | `.iter-revise` | Jump to named iter for minor revision |

Transition targets must reference an iter name defined in the same cursus. This is validated at parse time.

An iter can define both `next` and `transitions`. They do not conflict — transitions take precedence when their sentinel is present; `next` only applies on successful completion (`.iter-complete`).

### Single-Iter Cursus

A single-iter cursus defines a simple command:

```toml
# build.toml
description = "Implementation loop"
alias = "b"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
mode = "afk"
iterations = 30
banner = true
```

### Validation Rules

Enforced at parse time (before any iter executes):
1. Iter names must be unique within a cursus
2. Transition targets (`on_reject`, `on_revise`, `next`) must reference existing iter names
3. `consumes` values must match a `produces` value from some iter in the cursus (warning, not error — the produces iter may not have run yet on first pass)
4. Aliases must be unique across all cursus definitions in scope
5. Aliases must not shadow cursus file names
6. Prompt files must exist (resolved via layered lookup)
7. `[retry]` field types must be valid (u32 for `immediate`, u64 for `interval_secs` and `max_duration_secs`)
8. `iterations > 1` on an interactive iter emits a warning at parse time (interactive iters always run a single `cl` session regardless of `iterations` value)

## Sentinel Protocol

Cursus uses well-known sentinel files for transition control. Sentinels are named `.iter-*` — named after what the signal is about (the iter), not any specific tool.

### Sentinel Files

| File | Meaning | Cursus Behavior |
|------|---------|-----------------| 
| `.iter-complete` | Iter succeeded | Advance to next iter (or `next` override). If this is the final iter, pipeline completes |
| `.iter-reject` | Iter rejected by reviewer | Follow `on_reject` transition. Error if no `on_reject` defined |
| `.iter-revise` | Minor revision requested | Follow `on_revise` transition. Error if no `on_revise` defined |

### Detection

After each `cl` invocation returns, sgf checks for sentinel files in priority order:

1. `.iter-complete` — highest priority. If found alongside other sentinels, complete wins.
2. `.iter-reject` — checked second.
3. `.iter-revise` — checked third.
4. None found, iterations exhausted — treated as exhausted (pipeline enters stalled state).
5. None found, iterations remaining (interactive single-iteration iter) — treated as `.iter-complete` (interactive iters are assumed to complete in one invocation).

All detected sentinel files are deleted after processing. Sentinel search is recursive (depth <= 2) following the existing cleanup pattern.

### Interactive Iter Completion

Interactive iters (`mode = "interactive"`) with `iterations = 1` (the default) have special completion semantics: when the `cl` session ends without any sentinel file, the iter is treated as successfully completed. This is because interactive sessions end when the user is done — the absence of a rejection sentinel means implicit approval.

For interactive iters to signal rejection or revision, the agent must explicitly create `.iter-reject` or `.iter-revise` before the session ends. The prompt for review iters should instruct the agent to do this based on user feedback.

## Context Passing

Context passing allows iters to share information without a dedicated messaging tool. Each iter can produce a summary file that subsequent iters consume via prompt injection.

### Produces

When an iter defines `produces = "discuss-summary"`, the agent is expected to write a summary file at the end of its execution. The prompt for that iter should instruct the agent where to write and what to include.

The file path is: `.sgf/run/<run-id>/context/<key>.md`

The cursus runner creates the `context/` directory before the iter starts. The agent writes the file as part of its normal execution (via `Write` tool or `Bash`).

If the agent fails to write the file, the cursus runner emits a warning but continues. This is non-fatal because:
- The agent may have communicated through other means (spec updates in `fm`, pn comments)
- The consuming iter's prompt should be resilient to missing context

### Consumes

When an iter defines `consumes = ["discuss-summary", "draft-presentation"]`, the cursus runner:

1. Reads each file from `.sgf/run/<run-id>/context/<key>.md`
2. Concatenates them in order, with a header per file:
   ```
   === Context from iter: discuss (discuss-summary) ===

   <file contents>

   === Context from iter: draft (draft-presentation) ===

   <file contents>
   ```
3. Injects the concatenated content via `--append-system-prompt` to `cl`

The header includes both the iter name and the key name so the consuming agent knows the provenance of each context block. The iter name in the header reflects whichever iter last wrote the file — when `revise` overwrites a key originally produced by `draft`, the header shows `revise`, not `draft`.

### Produces Overwriting

When multiple iters produce the same key (e.g., both `draft` and `revise` produce `draft-presentation`), the later iter's file overwrites the earlier one. This is intentional: the revise iter produces an updated presentation that supersedes the draft's version. Subsequent consumers always get the latest version.

To track which iter last wrote each key, the cursus runner maintains a mapping of key → iter name in `meta.json` (`context_producers` field). After post-iter evaluation, if the iter defines a `produces` key and the file exists at `.sgf/run/<run-id>/context/<key>.md`, the mapping is updated. If the file does not exist, the mapping is left unchanged and a warning is emitted.

### Environment Variable

The run context directory path is set as an environment variable `SGF_RUN_CONTEXT` so agents can reference it programmatically in prompts. The value is an **absolute path** — e.g., `/home/user/myproject/.sgf/run/<run-id>/context/`. An absolute path is required because the agent process's working directory is not guaranteed to be the repository root (e.g., subprocesses spawned by the agent may change directories), so a relative path would resolve incorrectly.


## Structured Events

In programmatic mode (`isatty(stdin) == false` or `--output-format json`), cursus emits structured NDJSON events on stdout. Each line is a self-contained JSON object with an `event` field identifying the event type.

### Event Types

#### `run_start`

Emitted once when the pipeline begins.

```json
{
  "event": "run_start",
  "run_id": "change-20260422T150000",
  "cursus": "change",
  "iters": [
    {"name": "change", "mode": "interactive", "iterations": 1}
  ]
}
```

#### `iter_start`

Emitted when an iter begins execution.

```json
{
  "event": "iter_start",
  "iter": "change",
  "mode": "interactive",
  "iteration": 1,
  "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

#### `turn`

Emitted after each agent turn in an interactive iter. Contains the agent's response and whether it is waiting for input from the outer agent.

```json
{
  "event": "turn",
  "content": "I've reviewed the codebase. Should I use bcrypt or argon2 for password hashing?",
  "waiting_for_input": true,
  "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

When `waiting_for_input` is true, the sgf process exits and the outer agent should send its response via a new invocation with `--resume <run-id>`. When false, the agent completed its work and sgf continues to post-iter evaluation.

#### `iter_complete`

Emitted when an iter finishes.

```json
{
  "event": "iter_complete",
  "iter": "change",
  "outcome": "complete",
  "iterations_used": 1
}
```

`outcome` is one of: `complete`, `reject`, `revise`, `exhausted`.

#### `transition`

Emitted between iters when the pipeline advances.

```json
{
  "event": "transition",
  "from_iter": "review",
  "to_iter": "revise",
  "reason": "revise"
}
```

`reason` is one of: `complete` (normal advance), `reject` (on_reject transition), `revise` (on_revise transition).

#### `context_produced`

Emitted when an iter produces context for downstream iters.

```json
{
  "event": "context_produced",
  "key": "discuss-summary",
  "iter": "discuss"
}
```

#### `context_consumed`

Emitted when an iter consumes context from a previous iter.

```json
{
  "event": "context_consumed",
  "key": "discuss-summary",
  "from_iter": "discuss"
}
```

#### `stall`

Emitted when an iter exhausts its iterations without completing.

```json
{
  "event": "stall",
  "iter": "implement",
  "iterations_attempted": 5,
  "actions": ["retry", "skip", "abort"]
}
```

The outer agent should respond with one of the listed actions as plain text input via `--resume`.

#### `retry`

Emitted when auto-retry is triggered for a failed agent process.

```json
{
  "event": "retry",
  "attempt": 4,
  "reason": "process_crash",
  "next_retry_secs": 300
}
```

#### `run_complete`

Emitted when the pipeline finishes (successfully or not).

```json
{
  "event": "run_complete",
  "status": "completed",
  "run_id": "change-20260422T150000",
  "resume_command": "sgf change --resume change-20260422T150000"
}
```

`status` is one of: `completed`, `stalled`, `interrupted`, `error`.

#### `error`

Emitted on fatal errors.

```json
{
  "event": "error",
  "message": "prompt not found: change.md",
  "fatal": true,
  "iter": "change"
}
```

### Event Ordering

A typical programmatic session produces events in this order:

```
run_start
  iter_start (iter A)
    context_consumed (if applicable)
    turn (waiting_for_input: true)
    # outer agent resumes with response
    turn (waiting_for_input: false)
  iter_complete
  context_produced (if applicable)
  transition
  iter_start (iter B)
    ...
  iter_complete
run_complete
```

For AFK iters, no `turn` events are emitted — the iter runs to completion internally and the outer agent sees `iter_start` → `iter_complete`.

### Terminal Mode

In terminal mode (human at keyboard), these events are not emitted. The existing console output (badge boxes, banners, iteration headers) continues to work as before.

## Run State

Each cursus execution creates a run, tracked by metadata in `.sgf/run/`.

### Run ID Format

`<cursus-name>-<timestamp>` — e.g., `spec-20260317T140000`. Same format as the existing loop ID.

### Run Directory

`.sgf/run/<run-id>/` contains:

```
.sgf/run/spec-20260317T140000/
  meta.json           # run metadata
  context/            # produced summary files
    discuss-summary.md
    draft-presentation.md
  spec-20260317T140000.pid   # PID file (while running)
```

### Run Metadata (`meta.json`)

```json
{
  "run_id": "spec-20260317T140000",
  "cursus": "spec",
  "status": "running",
  "current_iter": "draft",
  "current_iter_index": 1,
  "iters_completed": [
    {
      "name": "discuss",
      "session_id": "a1b2c3d4-...",
      "completed_at": "2026-03-17T14:05:00Z",
      "outcome": "complete"
    }
  ],
  "context_producers": {
    "discuss-summary": "discuss"
  },
  "mode_override": null,
  "created_at": "2026-03-17T14:00:00Z",
  "updated_at": "2026-03-17T14:10:00Z"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `run_id` | string | Unique run identifier |
| `cursus` | string | Name of the cursus being run |
| `status` | string | `running`, `completed`, `stalled`, `interrupted` |
| `current_iter` | string | Name of the iter currently executing (or stalled at) |
| `current_iter_index` | u32 | Position in the iters array (for ordered resumption) |
| `iters_completed` | array | Record of each completed iter with session ID, timestamp, and outcome |
| `context_producers` | object | Mapping of produces key → iter name that last wrote it. Updated whenever an iter successfully writes its `produces` file |
| `mode_override` | string or null | CLI mode override (`-a` or `-i`) that applies to all iters |
| `created_at` | string | RFC3339 timestamp |
| `updated_at` | string | RFC3339 timestamp (updated after each iter) |

### Status Values

| Status | Meaning |
|--------|---------|
| `running` | Pipeline is actively executing an iter |
| `completed` | All iters finished successfully (final iter produced `.iter-complete`) |
| `stalled` | An iter exhausted its iterations without completing |
| `interrupted` | Pipeline was interrupted by signal (SIGINT/SIGTERM) |

### Cursus Resume via `--resume <run-id>`

Resume dispatch (how `--resume <run-id>` determines whether to route to cursus or non-cursus resume) is owned by the [session-resume spec](session-resume.md) Architecture section.

When a cursus run is resumed:
1. Load `meta.json` from the run directory
2. Restore full pipeline state: current iter, iteration count, accumulated context, context producers
3. For stalled runs (interactive mode): present options — Retry, Skip, or Abort
4. For stalled runs (programmatic mode): emit a `stall` event with available actions. On the next `--resume` invocation, the first line of stdin is parsed as an action: `retry` (re-run the stalled iter with the same iteration count), `skip` (advance to the next iter), `abort` (mark the run as `interrupted` and exit). Unrecognized actions emit an `error` event and exit 1
5. Continue the pipeline from the restored point

On any run exit (stall, interrupt, completion, error), sgf prints a copy-pasteable resume command:

```
To resume:  sgf change --resume change-20260422T150000
```

This is printed to stderr in terminal mode and included in structured events in programmatic mode.

### Stall Recovery

When a pipeline enters the `stalled` state:

1. Run metadata is persisted with `status: "stalled"` and `current_iter` set to the stalled iter
2. The cursus runner prints a stall banner:
   ```
   ╭─ Cursus STALLED ─────────────────────────────────╮
   │  Cursus:    spec                                  │
   │  Iter:      draft                                 │
   │  Reason:    Iterations exhausted (10/10)          │
   │                                                   │
   │  To resume: sgf spec --resume spec-20260317T140000│
   ╰───────────────────────────────────────────────────╯
   ```
3. The runner exits with code 2

When the user resumes with `sgf spec --resume spec-20260317T140000`:
1. Load `meta.json` from the run directory
2. Present the stalled state: which iter stalled, how many iterations were used, what context was accumulated
3. Offer options:
   - **Retry**: Re-run the stalled iter with the same iteration count
   - **Skip**: Advance to the next iter (if the user deems the iter's work sufficient)
   - **Abort**: Mark the run as interrupted and exit
4. Continue the pipeline from the chosen point

## Iter Execution

Each iter in a cursus pipeline follows this execution sequence:

### Pre-Iter Setup

1. **Resolve prompt file** — look up `prompt` via layered `.sgf/prompts/` resolution (local → global). Prompt resolution happens at cursus load time, before any iter executes.
2. **Prepare consumed context** — if `consumes` is defined, read each key from `.sgf/run/<run-id>/context/<key>.md`, concatenate with headers (see Context Passing), and build the `--append-system-prompt` argument.
3. **Ensure context directory** — create `.sgf/run/<run-id>/context/` if it does not exist.
4. **Generate session UUID** — `Uuid::new_v4()` for the `--session-id` flag.

### Invocation

5. **Invoke `cl`** — delegate to sgf's iteration runner (AFK), direct `cl` call (interactive), or programmatic runner (piped stdin). The effective mode is determined by: `mode_override` (from CLI `-a`/`-i`) if set, else the iter's `mode` field. See [springfield spec](springfield.md) Agent Invocation for flag details.
6. **For AFK iters** — run the iteration loop (up to `iterations` count). After each `cl` invocation, proceed to Post-Iter Evaluation. If the evaluation does not produce a transition, continue to the next iteration.
7. **For interactive iters** — run a single `cl` session (default `iterations = 1`). Proceed to Post-Iter Evaluation on exit.

### Post-Iter Evaluation

8. **Check sentinel files** — in priority order: `.iter-complete` > `.iter-reject` > `.iter-revise` > none. Delete all detected sentinels after reading.
9. **Update context producers** — if the iter defines `produces` and the file exists at `.sgf/run/<run-id>/context/<key>.md`, update `context_producers` mapping in `meta.json`.
10. **Update run metadata** — append an iteration record to `iters_completed` in `meta.json` with session_id, timestamp, and outcome.

### Transition Resolution

11. **`.iter-complete` found** — if this is the final iter, mark pipeline `completed` and exit. Otherwise advance to the next iter in sequence (or to the `next` override if defined).
12. **`.iter-reject` found** — follow `on_reject` transition. Error if no `on_reject` is defined.
13. **`.iter-revise` found** — follow `on_revise` transition. Error if no `on_revise` is defined.
14. **No sentinel, iterations remaining** — continue to next iteration of the same iter (AFK only).
15. **No sentinel, iterations exhausted (AFK)** — pipeline enters stalled state.
16. **No sentinel, interactive iter with `iterations = 1`** — treated as `.iter-complete` (implicit approval).

### Between Iters

17. **Emit transition event** (programmatic mode) — `transition` event with `from_iter`, `to_iter`, `reason`.
18. **Continue** — loop back to Pre-Iter Setup for the next iter.

## Command Resolution Changes

The full command resolution order is owned by the [springfield spec](springfield.md) CLI Commands section. Cursus participates in steps 3-5 of that sequence (local cursus TOML → global cursus TOML → alias matching).

### What This Means

- All commands are defined by cursus TOML files. There is no fallback mechanism.
- Prompt files (`.sgf/prompts/*.md`) remain unchanged — they are referenced by cursus definitions but not directly resolved by `sgf`.
- The layered resolution logic (local → global) is the same, applied to `.sgf/cursus/`.
- Adding a new command is as simple as creating a new `.toml` file in `.sgf/cursus/`.

### CLI Changes

No CLI changes. `sgf build -a -n 30` works as expected. The flags map to:
- `-a` → `mode_override: "afk"` (overrides iter-level `mode`)
- `-n 30` → overrides `iterations` on all iters (or on the single iter for single-iter cursus)
- `--no-push` → overrides `auto_push` to false on all iters

## Design Decisions

### Why TOML

TOML was chosen over YAML (whitespace sensitivity, implicit typing footguns), a custom DSL (parser maintenance cost), Rust DSL (requires recompilation), and Markdown (fragile parsing). TOML is consistent with the Rust ecosystem (Cargo.toml), provides strong typing via serde deserialization, and catches config errors at parse time.

### Why Annotated-Linear Over Graph

Iters are an ordered list (the happy path is readable top-to-bottom) with optional transition overrides for back-edges. This was chosen over a full graph model (harder to scan, requires explicit `start` node and `transitions` arrays) and a purely linear model (can't express review loops). The annotated-linear approach keeps simple cursus definitions simple while supporting the review/revise cycles needed for spec refinement.

### Why Sentinel Files Over Exit Codes

Exit codes are limited (one integer) and consumed by sgf's existing protocol (0=complete, 2=exhausted). Sentinel files allow multiple distinct signals, are the established pattern, and are visible/debuggable on disk.

### Why Context Passing Via Prompt Injection

Five alternatives were evaluated: accumulating context files, structured handoff files, using fm specs as context, pipeline-scoped variables, and prompt injection. Prompt injection was chosen because it requires no new tooling (uses existing `--append-system-prompt`), the cursus runner handles it transparently, and agents receive context exactly where they need it — in their prompt. The tradeoff is that agents must write a summary file, but this is a simple prompt instruction.

### Why Unified Command Model

All commands resolve to cursus definitions — a single-iter cursus is functionally identical to a raw prompt invocation. This provides one mental model for users and eliminates the need for separate configuration mechanisms.

### Why `iter` Not `stage`

"Iter" (Latin: journey, passage) aligns with the project's Latin naming convention (forma, pensa, cursus). Each iter is a discrete passage through the pipeline.

### Why `[[iter]]` Not `[[iters]]`

TOML array tables use `[[iter]]` (singular) to match the naming convention of defining one iter per table entry. Each `[[iter]]` block defines a single iter; TOML's array-of-tables syntax naturally pluralizes by repetition.


## Future Evolution

The cursus TOML format and runtime are designed to accommodate future capabilities without structural changes. These are explicitly out of scope for the initial implementation but inform the design.

### Event-Driven Triggers

The `trigger` field currently only supports `"manual"`. Future values:

```toml
# Watch for new pn issues of type "bug"
trigger = { watch = "pn", filter = { type = "bug", status = "open" } }

# Watch for fm spec status changes
trigger = { watch = "fm", filter = { status = "stable" } }

# Triggered by another cursus completing
trigger = { on_complete = "fix" }

# Periodic polling
trigger = { interval = "5m" }
```

### Daemon Cursus

Cursus definitions with non-manual triggers run as background daemons, continuously watching for events and spawning pipeline runs. This enables a reactive agent ecosystem where bugs trigger fix pipelines, fixes trigger spec amendments, and spec changes trigger verification.

### Cursus Chaining

A cursus can trigger another cursus on completion:

```toml
on_complete = "verify-cohesion"
```

Combined with event triggers, this enables multi-pipeline workflows: fix → amend-spec → verify-cohesion.

### Cross-Cursus Context

When cursus A triggers cursus B, context from A's run may need to flow into B. The `consumes` mechanism can extend to reference other run IDs:

```toml
consumes = ["<run-id>:fix-summary"]
```

The summary file path structure (`.sgf/run/<run-id>/context/<key>.md`) already supports this — it's just a matter of resolving paths across run directories.

### Concurrency Management

Multiple daemon cursus running simultaneously introduces contention. Mitigations already in place:
- `pn update --claim` provides atomic issue claiming
- Git handles file-level merge conflicts

Additional mechanisms needed for daemon mode:
- Run-level locking to prevent duplicate pipeline runs for the same trigger event
- Priority queuing for approval iters (user can only review one at a time)
- Rate limiting to prevent runaway pipeline spawning

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [session-resume](session-resume.md) — Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via --resume flag on any sgf subcommand
- [shutdown](shutdown.md) — Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle
- [test-harness](test-harness.md) — Cross-crate integration test harness — concurrency control, process lifecycle guards, mock infrastructure, and environment isolation
- [vcs-utils](vcs-utils.md) — Shared VCS utilities — git HEAD detection, auto-push
