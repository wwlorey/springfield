# cursus Specification

Pipeline orchestration — declarative TOML-defined multi-iter workflows with context passing, sentinel-based transitions, and stall recovery

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | draft |

## Overview

Cursus is the pipeline orchestration subsystem of sgf. It replaces the current hardcoded prompt dispatch and `config.toml` with declarative TOML pipeline definitions.

A **cursus** (Latin: "a running, course, path") is a named pipeline comprising one or more **iters** (Latin: "journey, passage") — discrete execution stages that run sequentially. Each iter invokes a prompt via ralph (AFK) or `cl` (interactive), with sentinel files controlling transitions between iters.

Cursus provides:
- **Declarative pipeline definitions**: TOML files in `.sgf/cursus/` define the iter sequence, execution modes, iteration counts, and transition rules
- **Context passing between iters**: Each iter can produce a summary file; subsequent iters consume it via prompt injection
- **Sentinel-based transitions**: Well-known sentinel files signal success, rejection, revision, or exhaustion — controlling which iter runs next
- **Stall recovery**: When an iter exhausts its iterations, the pipeline enters a stalled state the user can inspect and resume
- **Subsumption of prompts**: Single-iter cursus definitions replace the old `config.toml` entries. Every `sgf <command>` resolves to a cursus definition, whether it has one iter or many
- **Foundation for evolution**: The TOML format accommodates future trigger types (event-driven daemons, cursus chaining) without structural changes. Only manual triggers are supported initially

Cursus definitions follow the same layered resolution as prompts: project-local `./.sgf/cursus/` overrides global `~/.sgf/cursus/`.

## Architecture

## File Layout

```
~/.sgf/
  cursus/                      # global pipeline definitions (TOML)
    build.toml
    spec.toml
    verify.toml
    test.toml
    test-plan.toml
    doc.toml
    issues-log.toml
  prompts/                     # prompt content (markdown) — unchanged
    spec-discuss.md
    spec-draft.md
    spec-review.md
    spec-revise.md
    spec-approve.md
    build.md
    verify.md
    ...

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

Cursus is implemented in the `springfield` crate. Key modules:

- `cursus/mod.rs` — public API, TOML parsing, validation
- `cursus/toml.rs` — serde types for the TOML format
- `cursus/runner.rs` — iter execution, sentinel detection, transition logic
- `cursus/state.rs` — run state persistence, stall recovery
- `cursus/context.rs` — produces/consumes file management, prompt injection

The existing `orchestrate.rs` and `loop_mgmt.rs` are refactored into the cursus module. Ralph remains unchanged — cursus invokes it the same way the current orchestration does.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `toml` (0.8) | TOML parsing for cursus definitions |
| `serde` (1, derive) | Deserialization of TOML and JSON run state |
| `serde_json` (1) | Run state serialization/deserialization |
| `uuid` (1, v4) | Run ID and session ID generation |
| `chrono` (0.4) | Timestamps for run metadata |
| `tracing` (0.1) | Structured logging |

Workspace dependencies (already in springfield):
| Crate | Purpose |
|-------|---------|
| `ralph` | Iter execution in AFK mode |
| `claude-wrapper` | Iter execution in interactive mode (via `cl`) |
| `vcs-utils` | Git operations (auto-push) |
| `shutdown` | Graceful shutdown handling |

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Cursus TOML not found (neither local nor global) | Exit 1: `unknown command: <name>` |
| Cursus TOML parse error | Exit 1: `failed to parse cursus definition: <path>: <error>` |
| Validation failure (duplicate iter names, missing transition targets, etc.) | Exit 1: descriptive error at parse time |
| Prompt file not found for an iter | Exit 1: `prompt not found: <path>` (checked at cursus load time, before execution starts) |
| Iter exhausts iterations (`.ralph-exhausted`) | Pipeline enters stalled state. Run metadata updated. User notified with status and options |
| Sentinel file `.ralph-reject` with no `on_reject` transition defined | Exit 1: `iter '<name>' signaled reject but no on_reject transition is defined` |
| Sentinel file `.ralph-revise` with no `on_revise` transition defined | Exit 1: `iter '<name>' signaled revise but no on_revise transition is defined` |
| Run directory creation failure | Exit 1: `failed to create run directory: <error>` |
| Run metadata read/write failure | `tracing::error\!`, continue if possible (non-fatal for execution, fatal for resume) |
| `produces` file not written by agent | `tracing::warn\!` — continue to next iter. The consuming iter will run without that context. Not fatal because the agent may have communicated through other means (spec updates, pn comments) |
| Stale run directory from previous crashed run | Detected at startup. Previous run status updated to `interrupted` if still marked `running` |
| SIGINT/SIGTERM during iter execution | Delegated to ralph/cl signal handling. Pipeline status updated to `interrupted` on exit |

## Testing

### Unit Tests

#### `cursus/toml.rs`
- Parse valid single-iter cursus definition (e.g., `build.toml`)
- Parse valid multi-iter cursus definition with transitions (e.g., `spec.toml`)
- Parse cursus with `produces` and `consumes` fields
- Reject duplicate iter names
- Reject transition targets that reference non-existent iters
- Reject `consumes` referencing non-existent `produces` keys
- Default values: `mode` defaults to `interactive`, `iterations` defaults to 1, `trigger` defaults to `manual`, `auto_push` defaults to false
- Alias validation: reject duplicate aliases, reject aliases that shadow cursus names

#### `cursus/runner.rs`
- Sentinel `.ralph-complete` advances to next iter
- Sentinel `.ralph-reject` follows `on_reject` transition
- Sentinel `.ralph-revise` follows `on_revise` transition
- Missing sentinel with iterations exhausted enters stalled state
- Transition to a previous iter (back-edge) works correctly
- Final iter completion marks pipeline as completed
- `produces` file path is correctly constructed under run directory
- `consumes` files are correctly resolved and injected via `--append-system-prompt`
- Missing `produces` file emits warning but continues

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

### Integration Tests

Binary-level tests using `cargo test -p springfield`. Each test:
1. Creates a `tempfile::TempDir` with a cursus TOML and prompt files
2. Uses `RALPH_COMMAND` to mock agent execution
3. Verifies iter sequencing, sentinel transitions, and context file flow

| Test | Scenario | Asserts |
|------|----------|---------|
| Single-iter cursus | `build.toml` equivalent | Behaves identically to current `sgf build` |
| Multi-iter happy path | discuss → draft → review → approve | All iters run in sequence, exit 0 |
| Reject transition | review signals `.ralph-reject` | Pipeline jumps back to draft iter |
| Revise transition | review signals `.ralph-revise` | Pipeline jumps to revise, then back to review |
| Context passing | discuss produces summary, draft consumes it | Summary content appears in draft's system prompt |
| Stall recovery | draft exhausts iterations | Pipeline enters stalled state, metadata persisted |
| Resume stalled | Load stalled run, resume | Pipeline continues from stalled iter |
| Layered resolution | Local cursus overrides global | Local TOML takes precedence |

## TOML Format

## Cursus Definition

Each `.toml` file in `.sgf/cursus/` defines one cursus. The filename (minus `.toml`) is the cursus name and the `sgf` subcommand.

### Top-Level Fields

```toml
description = "Spec creation and refinement"
alias = "s"
trigger = "manual"
auto_push = true
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | required | Human-readable description of the cursus |
| `alias` | string | — | Short alias for the command (e.g., `"s"` for spec). Optional |
| `trigger` | string | `"manual"` | How the cursus is started. Only `"manual"` is supported initially |
| `auto_push` | bool | `false` | Auto-push after commits (applies to all iters unless overridden) |

### Iter Definition

Iters are defined as an array of tables:

```toml
[[iters]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
iterations = 1
produces = "discuss-summary"
consumes = []
auto_push = false

[[iters]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
produces = "draft-presentation"
consumes = ["discuss-summary"]
auto_push = true

[[iters]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iters.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iters]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iters]]
name = "approve"
prompt = "spec-approve.md"
mode = "interactive"
consumes = ["draft-presentation"]
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Unique identifier for this iter within the cursus |
| `prompt` | string | required | Prompt file name, resolved via layered `.sgf/prompts/` lookup |
| `mode` | `"interactive"` or `"afk"` | `"interactive"` | Execution mode |
| `iterations` | u32 | `1` | Max ralph iterations for this iter (only meaningful for `afk` mode) |
| `produces` | string | — | Key name for the summary file this iter writes. Stored at `.sgf/run/<run-id>/context/<key>.md` |
| `consumes` | array of strings | `[]` | Keys of summary files from previous iters, injected into this iter's system prompt |
| `auto_push` | bool | cursus-level default | Override auto-push for this specific iter |
| `next` | string | — | Override: after completion, go to this iter instead of the next in the list |
| `transitions` | table | — | Named transition overrides triggered by sentinel files |

### Transitions Table

```toml
[iters.transitions]
on_reject = "draft"
on_revise = "revise"
```

| Field | Sentinel File | Description |
|-------|---------------|-------------|
| `on_reject` | `.ralph-reject` | Jump to named iter on rejection |
| `on_revise` | `.ralph-revise` | Jump to named iter for minor revision |

Transition targets must reference an iter name defined in the same cursus. This is validated at parse time.

### Single-Iter Cursus (Prompt Replacement)

A single-iter cursus replaces a `config.toml` entry:

```toml
# build.toml — equivalent to the old [build] config.toml entry
description = "Implementation loop"
alias = "b"
auto_push = true

[[iters]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
```

This is the migration path for all existing prompts. Every `[section]` in the old `config.toml` becomes its own `.toml` file in `.sgf/cursus/`.

### Validation Rules

Enforced at parse time (before any iter executes):
1. Iter names must be unique within a cursus
2. Transition targets (`on_reject`, `on_revise`, `next`) must reference existing iter names
3. `consumes` values must match a `produces` value from some iter in the cursus (warning, not error — the produces iter may not have run yet on first pass)
4. Aliases must be unique across all cursus definitions in scope
5. Aliases must not shadow cursus file names
6. Prompt files must exist (resolved via layered lookup)

## Sentinel Protocol

Cursus extends ralph's existing sentinel file mechanism with additional well-known sentinels for transition control.

### Sentinel Files

| File | Meaning | Cursus Behavior |
|------|---------|-----------------|
| `.ralph-complete` | Iter succeeded | Advance to next iter (or `next` override). If this is the final iter, pipeline completes |
| `.ralph-reject` | Iter rejected by reviewer | Follow `on_reject` transition. Error if no `on_reject` defined |
| `.ralph-revise` | Minor revision requested | Follow `on_revise` transition. Error if no `on_revise` defined |
| `.ralph-exhausted` | Iter used all iterations without completing | Pipeline enters stalled state |

### Detection

After each ralph/cl invocation returns, cursus checks for sentinel files in priority order:

1. `.ralph-complete` — highest priority. If found alongside other sentinels, complete wins.
2. `.ralph-reject` — checked second.
3. `.ralph-revise` — checked third.
4. None found, iterations exhausted — treated as `.ralph-exhausted`.
5. None found, iterations remaining (interactive single-iteration iter) — treated as `.ralph-complete` (interactive iters are assumed to complete in one invocation).

All detected sentinel files are deleted after processing, following ralph's existing cleanup pattern (recursive search, depth <= 2).

### Interactive Iter Completion

Interactive iters (`mode = "interactive"`) with `iterations = 1` (the default) have special completion semantics: when the `cl` session ends without any sentinel file, the iter is treated as successfully completed. This is because interactive sessions end when the user is done — the absence of a rejection sentinel means implicit approval.

For interactive iters to signal rejection or revision, the agent must explicitly create `.ralph-reject` or `.ralph-revise` before the session ends. The prompt for review iters should instruct the agent to do this based on user feedback.

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
3. Injects the concatenated content via `--append-system-prompt` (for ralph) or `--append-system-prompt` (for `cl`)

The header includes both the iter name and the key name so the consuming agent knows the provenance of each context block.

### Produces Overwriting

When multiple iters produce the same key (e.g., both `draft` and `revise` produce `draft-presentation`), the later iter's file overwrites the earlier one. This is intentional: the revise iter produces an updated presentation that supersedes the draft's version. Subsequent consumers always get the latest version.

### Environment Variable

The run context directory path is also set as an environment variable `SGF_RUN_CONTEXT=.sgf/run/<run-id>/context/` so agents can reference it programmatically in prompts.

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
  "spec": "auth",
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
| `spec` | string or null | Spec stem if provided via CLI |
| `mode_override` | string or null | CLI mode override (`-a` or `-i`) that applies to all iters |
| `created_at` | string | RFC3339 timestamp |
| `updated_at` | string | RFC3339 timestamp (updated after each iter) |

### Status Values

| Status | Meaning |
|--------|---------|
| `running` | Pipeline is actively executing an iter |
| `completed` | All iters finished successfully (final iter produced `.ralph-complete`) |
| `stalled` | An iter exhausted its iterations without completing |
| `interrupted` | Pipeline was interrupted by signal (SIGINT/SIGTERM) |

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
   │  To resume: sgf resume spec-20260317T140000       │
   ╰───────────────────────────────────────────────────╯
   ```
3. The runner exits with code 2

When the user runs `sgf resume <run-id>`:
1. Load `meta.json` from the run directory
2. Present the stalled state: which iter stalled, how many iterations were used, what context was accumulated
3. Offer options:
   - **Retry**: Re-run the stalled iter (with same or increased iterations)
   - **Skip**: Advance to the next iter (if the user deems the iter's work sufficient)
   - **Abort**: Mark the run as interrupted and exit
4. Continue the pipeline from the chosen point

### Resume Integration

`sgf resume` is extended to support both legacy session resumes and cursus run resumes:
- If the argument matches a `.sgf/run/<id>/meta.json` with cursus metadata, it's a cursus resume
- Otherwise, fall back to the existing session-resume behavior

## Iter Execution

### Execution Flow

For each iter in the cursus (starting from the first, or from the resume point):

1. **Pre-iter setup**:
   - Create/verify run context directory
   - Update `meta.json` with `current_iter` and `status: "running"`
   - Clean stale sentinel files (`.ralph-complete`, `.ralph-reject`, `.ralph-revise`)
   - Resolve `consumes` files and build system prompt injection content
   - Set environment: `SGF_RUN_CONTEXT`, `SGF_SPEC` (if applicable)

2. **Invoke iter**:
   - **AFK mode**: Invoke ralph with the iter's prompt, iterations, spec, and consumed context via `--append-system-prompt`
   - **Interactive mode**: Invoke `cl` directly with the iter's prompt and consumed context via `--append-system-prompt`
   - Session ID management: fresh UUID per iter invocation (passed to ralph/cl)

3. **Post-iter evaluation**:
   - Check sentinel files in priority order (see Sentinel Protocol)
   - Record iter completion in `meta.json` (`iters_completed` array)
   - Check for `produces` file existence (warn if missing)
   - Determine next iter:
     a. `.ralph-complete` → advance to `next` override or next in list
     b. `.ralph-reject` → jump to `on_reject` target
     c. `.ralph-revise` → jump to `on_revise` target
     d. `.ralph-exhausted` → enter stalled state
     e. Interactive with no sentinel → treat as complete (advance)

4. **Termination**:
   - If the completed iter is the last in the list (and no `next` override): pipeline complete. Update status to `completed`, exit 0
   - If stalled: update status to `stalled`, print stall banner, exit 2
   - If interrupted: update status to `interrupted`, exit 130

### Mode Override

CLI flags `-a` and `-i` override the `mode` field for ALL iters in the cursus. This allows running an otherwise-AFK cursus interactively for debugging, or vice versa. The override is stored in `meta.json` as `mode_override`.

### Spec Passthrough

When `sgf <cursus> <spec>` is invoked with a spec argument:
- `SGF_SPEC=<stem>` is set in the environment for all iters
- Ralph receives `--spec <stem>` for AFK iters
- `cl` receives `--append-system-prompt 'study @./specs/<stem>.md'` for interactive iters
- Same behavior as the current `sgf build auth` pattern

## Command Resolution Changes

Cursus changes how `sgf <command>` resolves what to run. The new resolution order:

1. Check if `<command>` matches a reserved built-in (`init`, `logs`, `resume`, `status`). If so, run the built-in.
2. Check if `./.sgf/cursus/<command>.toml` exists (local override). If so, parse and run the cursus.
3. Check if `~/.sgf/cursus/<command>.toml` exists (global default). If so, parse and run the cursus.
4. Check if `<command>` matches an alias in any resolved cursus TOML. If so, resolve to the aliased cursus and run it.
5. Fall back to `config.toml` resolution (legacy path — see Migration Strategy below).
6. Error: `unknown command: <command>`.

### What Changes

- `config.toml` is superseded by individual cursus TOML files. It remains functional during the transitional period but is removed once all commands have cursus TOMLs.
- Prompt files (`.sgf/prompts/*.md`) remain unchanged — they are referenced by cursus definitions but no longer directly resolved by `sgf`.
- The layered resolution logic (local → global) is the same, just applied to `.sgf/cursus/` instead of `.sgf/prompts/`.

### Migration Strategy

The cursus module is built alongside the existing orchestration code. Both resolution paths coexist during the transition:

1. **Cursus TOML takes precedence**: If a cursus TOML exists for a command, it is used. The `config.toml` entry for that command is ignored.
2. **`config.toml` fallback**: Commands without a cursus TOML continue to resolve via `config.toml` and the existing `orchestrate.rs` / `config.rs` code paths.
3. **Incremental adoption**: Cursus TOMLs are created for each command as they are migrated. Single-iter cursus definitions are functionally identical to the old `config.toml` entries.
4. **Final cleanup**: Once all commands have cursus TOMLs, the last implementation issue removes `config.toml` support, `config.rs`, and the legacy paths in `orchestrate.rs` and `loop_mgmt.rs`.

### Migration Table

Every `[section]` in the old `config.toml` becomes a cursus TOML:

| Old (`config.toml`) | New (`.sgf/cursus/`) |
|----------------------|----------------------|
| `[build]` with `mode`, `iterations`, `auto_push`, `alias` | `build.toml` with one `[[iters]]` entry |
| `[spec]` | `spec.toml` (initially one iter, later multi-iter for spec refinement) |
| `[verify]` | `verify.toml` |
| `[test]` | `test.toml` |
| `[test-plan]` | `test-plan.toml` |
| `[doc]` | `doc.toml` |
| `[issues-log]` | `issues-log.toml` |

### CLI Changes

No CLI changes. `sgf build -a -n 30` works exactly as before. The flags map to:
- `-a` → `mode_override: "afk"` (overrides iter-level `mode`)
- `-n 30` → overrides `iterations` on all iters (or on the single iter for single-iter cursus)
- `--no-push` → overrides `auto_push` to false on all iters

## Design Decisions

### Why TOML

TOML was chosen over YAML (whitespace sensitivity, implicit typing footguns), a custom DSL (parser maintenance cost), Rust DSL (requires recompilation), and Markdown (fragile parsing). TOML is consistent with the Rust ecosystem (Cargo.toml), provides strong typing via serde deserialization, and catches config errors at parse time.

### Why Annotated-Linear Over Graph

Iters are an ordered list (the happy path is readable top-to-bottom) with optional transition overrides for back-edges. This was chosen over a full graph model (harder to scan, requires explicit `start` node and `transitions` arrays) and a purely linear model (can't express review loops). The annotated-linear approach keeps simple cursus definitions simple while supporting the review/revise cycles needed for spec refinement.

### Why Sentinel Files Over Exit Codes

Exit codes are limited (one integer) and consumed by ralph's existing protocol (0=complete, 2=exhausted). Sentinel files allow multiple distinct signals, are already the established pattern in ralph, and are visible/debuggable on disk.

### Why Context Passing Via Prompt Injection

Five alternatives were evaluated: accumulating context files, structured handoff files, using fm specs as context, pipeline-scoped variables, and prompt injection. Prompt injection was chosen because it requires no new tooling (uses existing `--append-system-prompt`), the cursus runner handles it transparently, and agents receive context exactly where they need it — in their prompt. The tradeoff is that agents must write a summary file, but this is a simple prompt instruction.

### Why Subsume Prompts

Rather than having `sgf <command>` resolve prompts directly and a separate `sgf seq` for pipelines, all commands resolve to cursus definitions. A single-iter cursus is functionally identical to a raw prompt invocation. This eliminates the awkward hierarchy where simple prompts had shorter commands than pipelines, and provides one mental model for users.

### Why `iter` Not `stage`

"Iter" (Latin: journey, passage) aligns with the project's Latin naming convention (forma, pensa, cursus). Each iter is a discrete passage through the pipeline.

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
- [ralph](ralph.md) — Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push
- [session-resume](session-resume.md) — Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via sgf resume
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle
