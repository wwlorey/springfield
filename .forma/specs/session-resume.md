# session-resume Specification

Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via --resume flag or sgf resume command

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | proven |

## Overview

Session tracking and resume mechanism for sgf. Persists Claude Code session IDs and loop configuration in JSON sidecar files, enabling users to resume interrupted sessions. Cursus uses this mechanism for per-iteration session tracking within pipeline runs.

The feature provides:
- **Session metadata persistence**: JSON sidecar files in `.sgf/run/{loop_id}.json` storing all iteration session IDs, loop config, and status
- **Pre-assigned session IDs**: Generate a fresh UUID before each `cl` invocation and pass it via `--session-id <uuid>`, ensuring we always know the session ID without parsing output
- **`--resume <run-id>` flag**: Any sgf dynamic subcommand accepts `--resume <run-id>` to resume a stalled or interrupted run. For cursus runs, resumes from the stalled/interrupted iter (see cursus spec Run State). For non-cursus sessions, resumes the most recent session.
- **`sgf resume [run-id]` built-in**: Lists all resumable sessions (both cursus and legacy) in a unified picker. With a run-id argument, resumes directly.
- **Resume command on exit**: On any run exit (stall, interrupt, completion, error), sgf prints a copy-pasteable resume command to stderr: `To resume: sgf <command> --resume <run-id>`
- **Expired session fallback**: If a resumed session fails within 5 seconds (likely expired), sgf prompts the user to restart with the original prompt
- **All modes**: Works for interactive, AFK, and programmatic sessions

## Architecture

Changes are within the `springfield` crate:

### springfield (crates/springfield/)

- `loop_mgmt.rs`: `SessionMetadata` and `IterationRecord` structs, atomic read/write/list functions, `find_resumable_sessions()` for discovering resumable non-cursus sessions, PID file management, loop ID generation
- `orchestrate.rs`: Resume session handler (`run_resume`), expired session fallback (`restart_with_prompt`), interactive picker for multi-iteration sessions, metadata status update on exit
- `main.rs`: `--resume <run-id>` flag parsing on dynamic subcommands, `sgf resume [run-id]` built-in command with unified picker (`collect_resumable`), `resume_dispatch()` routing
- `iter_runner/mod.rs`: Per-iteration UUID generation, `--session-id` and `--resume` flag passthrough to `cl`

### Session Metadata File

Non-cursus sessions (simple loops, `sgf <file>`):

```
.sgf/run/{loop_id}.json
```

Co-located with existing `.sgf/run/{loop_id}.pid` files. Gitignored (`.sgf/run/` is already in `.gitignore`).

Cursus sessions use a different layout: `.sgf/run/{run_id}/meta.json` (see cursus spec Run State section). The session-resume spec owns the non-cursus metadata format and the dispatch logic. The cursus spec owns the cursus-specific metadata format and resume logic.

### Resume Entry Points

There are two ways to resume a session:

1. **`sgf resume [run-id]`** — Built-in command. Without arguments, shows a unified picker of all resumable sessions (both cursus and legacy). With a run-id, resumes directly.
2. **`sgf <cmd> --resume <run-id>`** — Flag on any dynamic subcommand. Routes through the same `resume_dispatch()` logic.

Both entry points use the same dispatch:

1. If `.sgf/run/<run-id>/meta.json` exists (cursus metadata), delegate to cursus resume logic.
2. Otherwise if `.sgf/run/<run-id>.json` exists (non-cursus metadata), treat as a non-cursus session resume.
3. Otherwise, exit 1: `run not found: <run-id>`.

### Data Flow

```
sgf                                    cl (claude-wrapper)
 │                                      │
 ├─ generate fresh session UUID ────────┤
 ├─ write metadata (status: running) ───┤
 ├─ pass --session-id <uuid> ───────────┤
 │                                      │
 │  [each cl invocation]                │
 │                                      │
 │  [after each invocation]             │
 ├─ append iteration to metadata ───────┤
 │                                      │
 │  [session ends]                      │
 │                                      │
 ├─ update metadata (status: final) ────┤
 ├─ print resume command to stderr ─────┤
 │                                      │
 ├─ [later] sgf <cmd> --resume <id> ───►│
 ├─ read metadata ──────────────────────┤
 ├─ pass --resume <session_id> ─────────┤
```

For interactive mode, sgf calls `cl` directly with `--session-id <uuid>`, then writes metadata on exit.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `uuid` (1, v4) | Generate session UUIDs |
| `serde` (1, derive) | Serialize/deserialize session metadata JSON |
| `serde_json` (1) | Read/write JSON files |
| `chrono` (0.4) | Timestamp metadata (RFC 3339 formatting, relative time display) |
| `libc` | PID alive check via `kill(pid, 0)` |
| `tempfile` (dev) | Temp directories for unit tests |

All are existing workspace dependencies.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `--resume` with unknown run-id | Print "run not found: <run-id>" to stderr, exit 1 |
| Metadata file corrupt | Print "Session metadata not found or corrupt for <run-id>: <error>" to stderr, exit 1 |
| Metadata write failure (disk full, permissions) | `tracing::warn\!`, continue without metadata (session still runs, just cannot be resumed) |
| Resume session fails within 5 seconds (likely expired) | Prompt user: "Restart with same prompt (<path>)? [Y/n]". If yes, restart with original prompt via `cl`. If no, return the exit code. |
| `cl --resume` fails (session expired/gone, >5s) | Return the exit code. No special handling. |
| No iterations found for session | Print "No iterations found for session: <loop-id>" to stderr, exit with error |

## Testing

### Unit Tests (springfield — loop_mgmt.rs)

| Test | Asserts |
|------|---------|
| Write and read session metadata roundtrip | JSON file created, all fields match |
| List sessions sorted by updated_at desc | Order correct, all fields present |
| List sessions skips corrupt JSON files | Corrupt file ignored, valid files returned |
| List sessions returns empty when no JSON files | No panic, empty result |
| List sessions returns empty when run dir missing | No panic, empty result |
| Read metadata for nonexistent loop_id | Returns None |
| Write metadata atomic — no tmp left behind | `.tmp` file removed after rename |
| Empty iterations roundtrip | Empty vec serializes and deserializes |
| Multiple iterations roundtrip | 3 iterations with distinct session IDs preserved |
| Append iteration via write | Sequential writes accumulate iterations |
| Iterations len replaces iterations_completed | Derived from vec length |
| Cursus field roundtrip (Some) | Cursus name preserved |
| Cursus field roundtrip (None) | Null cursus preserved |
| Find resumable includes interrupted/exhausted/crashed | Completed excluded, stale running marked crashed |
| Find resumable empty when no sessions | Empty result |
| Find resumable sorted newest first | Order by updated_at desc |

### Unit Tests (springfield — orchestrate.rs)

| Test | Asserts |
|------|---------|
| Exit code to status mappings | 0→completed, 2→exhausted, 130/1/42→interrupted |
| Humanize relative time (seconds/minutes/hours/days) | Correct suffix |
| Humanize invalid timestamp | Returns "unknown" |
| Humanize future timestamp | Returns "just now" |
| Resume unknown loop_id returns NotFound error | Error message contains loop ID |
| Resume valid loop_id launches cl with --resume flag | cl args contain --resume, session_id, --verbose |
| Resume loop_id with no iterations returns error | InvalidData error |
| Picker entries sort newest first | Sorted by completed_at desc |
| Picker entries truncate drops oldest | Capped at 20 entries |
| Print entries display format | Correct columns rendered |
| Print entries multi-session display | Multiple sessions with correct numbering |

### Unit Tests (springfield — main.rs)

| Test | Asserts |
|------|---------|
| Parse --resume with value | DynamicArgs.resume populated, command preserved |
| Parse --resume missing value | Error: requires a value |
| Parse --resume with -a error | Mutually exclusive error |
| Parse --resume with -i error | Mutually exclusive error |
| Resume dispatch not found exits error | NotFound with run-id in message |
| Resume dispatch cursus metadata delegates to cursus | Enters cursus path |
| Resume dispatch legacy metadata delegates to legacy | Enters legacy path |
| Resume dispatch cursus takes priority over legacy | When both exist, cursus wins |
| Resume dispatch corrupt legacy returns error | InvalidData error |
| Collect resumable empty when no sessions | Empty result |
| Collect resumable includes legacy interrupted | Interrupted sessions included |
| Collect resumable includes cursus stalled | Stalled cursus runs included |
| Collect resumable merges and sorts by updated_at | Mixed sources sorted correctly |
| Collect resumable excludes completed, includes crashed | Completed filtered out, stale running → crashed |
| Collect resumable truncates to 20 | Capped at 20 entries |

### Integration Tests (springfield — E2E)

| Test | Asserts |
|------|---------|
| AFK session writes metadata with session_id | `.sgf/run/{loop_id}.json` exists with `session_id` |
| Interactive session writes metadata | Metadata file created |
| `--resume` passes `--resume` to cl | Mock agent receives `--resume <session_id>` in args |
| `--resume` with nonexistent run-id exits 1 | Exit code 1, stderr contains "run not found" |
| Metadata survives interrupted session (Ctrl+C) | Metadata file has status `interrupted` |
| Resume command printed on stall/interrupt/completion | "To resume: sgf <cmd> --resume <run-id>" on stderr |
| Per-iteration session IDs are unique | Each iteration gets distinct UUID |
| Resume picker across multiple loops | Flat list with correct sorting |

## Resume Dispatch

### Resume via `--resume <run-id>`

Any sgf dynamic subcommand accepts `--resume <run-id>`:

```
sgf change --resume change-20260422T150000
sgf spec --resume spec-20260317T140000
sgf build --resume build-20260316T162408
```

The `--resume` flag is mutually exclusive with `-a/--afk` and `-i/--interactive`.

### Resume via `sgf resume`

The `sgf resume` built-in command provides a unified session picker:

- **`sgf resume`** (no args): Lists all resumable sessions (both cursus and legacy) sorted by `updated_at` descending, capped at 20 entries. Displays run_id, label (cursus name or stage), status, and relative time. User selects by number.
- **`sgf resume <run-id>`**: Resumes the specified run directly without showing the picker.

The picker uses `collect_resumable()` which merges cursus runs (from `cursus::state::find_resumable_runs`) and legacy sessions (from `loop_mgmt::find_resumable_sessions`).

### Dispatch Behavior

Both entry points use `resume_dispatch()`:

1. Read `.sgf/run/<run-id>/meta.json` (cursus) or `.sgf/run/<run-id>.json` (non-cursus)
2. If not found → error: `run not found: <run-id>`, exit 1
3. For cursus runs: delegate to cursus resume logic (restores full pipeline state)
4. For non-cursus sessions with a single iteration: resume directly via `cl --resume <session_id>`
5. For non-cursus sessions with multiple iterations: display a picker to choose which iteration to resume
6. Update metadata on exit: `status`, `updated_at`

### Expired Session Fallback

When resuming a non-cursus session, if `cl --resume` exits with a non-zero code within 5 seconds, sgf assumes the session has expired and prompts:

```
session may have expired

Restart with same prompt (.sgf/prompts/build.md)? [Y/n]
```

If the user confirms, sgf restarts with the original prompt via a fresh `cl` invocation (no `--resume`).

### Resume Command Output

On any run exit (stall, interrupt, completion, error), sgf prints to stderr:

```
To resume:  sgf change --resume change-20260422T150000
```

This is printed even on Ctrl+C / Ctrl+D exits, ensuring the user always has a way to get back to where they were.

### Metadata File Lifecycle

Session metadata files in `.sgf/run/` are never pruned automatically. The directory is gitignored, so files accumulate only on the local machine. Manual cleanup via `rm -rf .sgf/run/*` is safe at any time — metadata files are not required for normal operation, only for resume.

## Session Metadata Schema

### File: `.sgf/run/{loop_id}.json`

```json
{
  "loop_id": "build-20260316T120000",
  "iterations": [
    { "iteration": 1, "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", "completed_at": "2026-03-16T12:02:30Z" },
    { "iteration": 2, "session_id": "f9e8d7c6-b5a4-3210-fedc-ba9876543210", "completed_at": "2026-03-16T12:05:30Z" }
  ],
  "stage": "build",
  "spec": "auth",
  "cursus": null,
  "mode": "afk",
  "prompt": ".sgf/prompts/build.md",
  "iterations_total": 2,
  "status": "completed",
  "created_at": "2026-03-16T12:00:00Z",
  "updated_at": "2026-03-16T12:05:30Z"
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `loop_id` | string | The loop identifier (same as filename stem) |
| `iterations` | array | List of iteration records, each with `iteration` (1-based index), `session_id` (UUID), and `completed_at` (ISO 8601 timestamp) |
| `stage` | string | Prompt stage name (e.g., `spec`, `build`, `verify`). For simple prompt mode, this is `"simple"`. |
| `spec` | string (optional) | Forma spec stem associated with this session, if any |
| `cursus` | string (optional) | Cursus name if this session was launched via a cursus pipeline. Used by `--resume <run-id>` dispatch to distinguish cursus runs (delegate to cursus resume) from non-cursus sessions. `null` for simple prompt mode (`sgf <file>`) and for non-cursus single-iter commands |
| `mode` | string | `"interactive"` or `"afk"` |
| `prompt` | string | Resolved prompt file path |
| `iterations_total` | u32 | Total iterations configured |
| `status` | string | One of: `running`, `completed`, `interrupted`, `exhausted` |
| `created_at` | string (ISO 8601) | When the session started |
| `updated_at` | string (ISO 8601) | When the metadata was last written |

The number of completed iterations is derived from `iterations.len()`. There is no separate `iterations_completed` field.

### Status Values

| Status | When set |
|--------|----------|
| `running` | Written before spawning `cl` |
| `completed` | Agent exited cleanly (`IterExitCode::Complete`) |
| `interrupted` | SIGINT/SIGTERM (`IterExitCode::Interrupted`) or agent error (`IterExitCode::Error`) |
| `exhausted` | Max iterations reached (`IterExitCode::Exhausted`) |

Additionally, `find_resumable_sessions()` assigns a synthetic `crashed` status to sessions with `running` status whose PID is no longer alive (stale running sessions). This status is never written to disk — it is only used for display in the resume picker.

### Write Timing

1. **Before spawn**: Write metadata with `status: running`, empty `iterations` array
2. **Before each iteration**: Append an `IterationRecord` with `session_id` and empty `completed_at` via the `on_iteration_start` callback
3. **After each iteration**: Update the last `IterationRecord` with `completed_at` timestamp via the `on_iteration_complete` callback, update `updated_at`
4. **On exit**: Update `status` based on `IterExitCode`, update `updated_at`

All writes use atomic rename (write to `.tmp`, then rename into place). Atomic rename prevents corruption from crashes.

## Session Handling

### Session ID Per Invocation

Every `cl` invocation receives a fresh `--session-id <uuid>` generated by sgf (via `Uuid::new_v4()`). There is no session continuity across iterations — each iteration is a completely fresh agent invocation.

### Resume

`--resume <run-id>` on any sgf dynamic subcommand or `sgf resume <run-id>` resumes a previous session. It is not used across iterations within a loop — each iteration starts fresh. The `--resume` flag applies only on iteration 1 for external session resume.

### cl Invocation (Normal)

```
cl --verbose --dangerously-skip-permissions --settings '{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}' --session-id <fresh-uuid> @prompt.md
```

### cl Invocation (Resume via orchestrate)

```
cl --resume <session_id> --verbose --dangerously-skip-permissions
```

The `--resume` flag restores the full session context from Claude Code's session store, so no prompt file or additional flags are needed.

### cl Invocation (Resume via iter_runner)

When the iter_runner handles resume (iteration 1 with `config.resume` set):

```
cl --verbose --dangerously-skip-permissions --resume <session_id>
```

Iterations 2+ always use `--session-id <fresh-uuid>` with the prompt.

## Springfield Changes

### orchestrate.rs

**`run_resume(root, loop_id)`**: Entry point for non-cursus session resume.

1. Read `SessionMetadata` from `.sgf/run/{loop_id}.json`
2. If no iterations found, return error
3. If single iteration: resume directly via `run_resume_session`
4. If multiple iterations: display picker, user selects which iteration to resume

**`run_resume_session(root, meta, session_id)`**: Spawns `cl --resume <session_id> --verbose --dangerously-skip-permissions` via PTY tee (`run_interactive_with_pty`). If Ctrl+C was forwarded and exit code is 0, treats as exit 130 (interrupted). If exit is non-zero within 5 seconds, prompts to restart with original prompt.

**`restart_with_prompt(root, meta, controller, log_path)`**: Fresh `cl` invocation with the original prompt. Used when a resumed session appears expired.

**`update_metadata_on_exit(root, loop_id, exit_code)`**: Maps exit code to status string (0→completed, 2→exhausted, other→interrupted) and writes updated metadata.

### main.rs

**`parse_dynamic_args`**: Parses `--resume <run-id>` as an optional field on `DynamicArgs`. Mutually exclusive with `-a/--afk` and `-i/--interactive`.

**`resume_dispatch(root, run_id)`**: Unified dispatch — checks cursus metadata first, then legacy metadata, then returns "run not found" error.

**`run_resume_command(root, run_id)`**: The `sgf resume [run-id]` built-in. Without args, calls `collect_resumable()` to merge cursus and legacy resumable sessions into a unified picker (sorted by `updated_at` desc, capped at 20). With args, delegates to `resume_dispatch`.

**`collect_resumable(root)`**: Merges `cursus::state::find_resumable_runs()` and `loop_mgmt::find_resumable_sessions()` into a flat list of `ResumableEntry` structs, sorted by `updated_at` descending, truncated to 20 entries.

### loop_mgmt.rs

```rust
#[derive(Serialize, Deserialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub session_id: String,
    pub completed_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct SessionMetadata {
    pub loop_id: String,
    pub iterations: Vec<IterationRecord>,
    pub stage: String,
    pub mode: String,
    pub prompt: String,
    pub spec: Option<String>,
    pub cursus: Option<String>,
    pub iterations_total: u32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}
```

**`write_session_metadata`**: Writes atomically (write to `.tmp`, rename). Called before spawn, after each iteration, and on exit.
**`read_session_metadata`**: Returns `Ok(None)` if file does not exist, `Err` if file is corrupt.
**`list_session_metadata`**: Returns all sessions sorted by `updated_at` descending, skipping corrupt files.
**`find_resumable_sessions`**: Filters to `interrupted`, `exhausted`, or `running` status. Stale `running` sessions (PID not alive) get synthetic `crashed` status.
**`generate_loop_id(stage, spec)`**: Generates `{stage}[-{spec}]-{YYYYMMDDTHHmmss}` loop IDs.

## Related Specifications

- [claude-wrapper](claude-wrapper.md) — Agent wrapper — layered .sgf/ context injection, cl binary
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle
