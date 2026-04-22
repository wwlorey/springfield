# session-resume Specification

Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via --resume flag on any sgf subcommand

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | draft |

## Overview

Session tracking and resume mechanism for sgf. Persists Claude Code session IDs and loop configuration in JSON sidecar files, enabling users to resume interrupted sessions via `--resume <run-id>` on any sgf subcommand. Cursus uses this mechanism for per-iteration session tracking within pipeline runs.

The feature adds:
- **Session metadata persistence**: JSON sidecar files in `.sgf/run/{loop_id}.json` storing all iteration session IDs, loop config, and status
- **Pre-assigned session IDs**: Generate a fresh UUID before each `cl` invocation and pass it via `--session-id <uuid>`, ensuring we always know the session ID without parsing output.
- **`--resume <run-id>` flag**: Any sgf subcommand accepts `--resume <run-id>` to resume a stalled or interrupted run. For cursus runs, resumes from the stalled/interrupted iter (see [cursus spec](cursus.md) Run State). For non-cursus sessions, resumes the most recent session.
- **Resume command on exit**: On any run exit (stall, interrupt, completion, error), sgf prints a copy-pasteable resume command to stderr: `To resume: sgf <command> --resume <run-id>`
- **Both modes**: Works for interactive, AFK, and programmatic sessions

## Architecture

Changes are within the `springfield` crate:

### springfield (crates/springfield/)

- `loop_mgmt.rs`: Functions for session metadata read/write/list
- `orchestrate.rs`: Generate session UUID, write metadata after `cl` exits, resume handler
- `main.rs`: Parse `--resume <run-id>` flag on all subcommands

### Session Metadata File

Non-cursus sessions (simple loops, `sgf <file>`):

```
.sgf/run/{loop_id}.json
```

Co-located with existing `.sgf/run/{loop_id}.pid` files. Gitignored (`.sgf/run/` is already in `.gitignore`).

Cursus sessions use a different layout: `.sgf/run/{run_id}/meta.json` (see [cursus spec](cursus.md) Run State section). The session-resume spec owns the non-cursus metadata format and the `--resume` dispatch logic. The cursus spec owns the cursus-specific metadata format and resume logic.

### Resume Dispatch (owned by session-resume)

`--resume <run-id>` on any sgf subcommand is the unified entry point. The dispatch logic:

1. If the argument matches a `.sgf/run/<id>/meta.json` with a `cursus` field, delegate to cursus resume logic.
2. Otherwise if `.sgf/run/<id>.json` exists, treat it as a non-cursus session resume.
3. Otherwise, exit 1: `run not found: <run-id>`.

The former `sgf resume` built-in command is removed. All resume is via `--resume <run-id>` on the original subcommand.

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
| `chrono` (0.4) | Timestamp metadata (already a transitive dep via loop_id generation) |

No new external dependencies beyond `uuid`. The `serde` and `serde_json` crates are already workspace dependencies.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| `--resume` with unknown run-id | Print "run not found: <run-id>" to stderr, exit 1 |
| Metadata file missing/corrupt | Print "Session metadata not found or corrupt for <run-id>" to stderr, exit 1 |
| Metadata write failure (disk full, permissions) | `tracing::warn\!`, continue without metadata (session still runs, just can't be resumed) |
| `cl --resume` fails (session expired/gone) | Claude Code handles this — it falls back to a new session. Not our error to handle. |

## Testing

### Unit Tests (springfield — loop_mgmt.rs)

| Test | Asserts |
|------|---------|
| Write and read session metadata roundtrip | JSON file created, fields match |
| List sessions returns sorted by timestamp (newest first) | Order correct, all fields present |
| List sessions skips corrupt JSON files | Corrupt file ignored, valid files returned |
| List sessions returns empty vec when no JSON files | No panic, empty result |
| Read metadata for nonexistent loop_id | Returns None |

### Unit Tests (springfield — orchestrate.rs)

| Test | Asserts |
|------|---------|
| `--resume` flag adds `--resume <id>` to cl args | Arg vector contains `--resume` and session_id |
| `--session-id` flag adds `--session-id <uuid>` to cl args | Arg vector contains `--session-id` and uuid |

### Integration Tests (springfield — E2E)

| Test | Asserts |
|------|---------|
| AFK session writes metadata with session_id | Run sgf with mock agent, check `.sgf/run/{loop_id}.json` exists and contains `session_id` |
| Interactive session writes metadata with session_id | Run sgf with mock agent, check metadata file |
| `--resume` passes `--resume` to cl | Mock agent receives `--resume <session_id>` in args |
| `--resume` with nonexistent run-id exits 1 | Exit code 1, stderr contains "run not found" |
| Metadata survives interrupted session (Ctrl+C) | Send SIGINT, metadata file still has `session_id` and status `interrupted` |
| Resume command printed on stall | Stalled run prints `To resume: sgf <cmd> --resume <run-id>` to stderr |
| Resume command printed on interrupt | Ctrl+C exit prints resume command to stderr |
| Resume command printed on completion | Completed run prints resume command to stderr |
| Resume command in programmatic JSON | Piped stdin run includes `resume_command` in `run_complete` event |

### CLI Verification (manual / scripted)

```bash
# Start a session, Ctrl+C out
sgf spec -i
# Check metadata was written
cat .sgf/run/spec-*.json
# Resume it
sgf spec --resume spec-20260316T120000
```

## Resume Dispatch

### Resume via `--resume <run-id>`

The former `sgf resume [loop-id]` built-in command is removed. Resume is now accessed via `--resume <run-id>` on any sgf subcommand:

```
sgf change --resume change-20260422T150000
sgf spec --resume spec-20260317T140000
sgf build --resume build-20260316T162408
```

### Behavior

1. Read `.sgf/run/<run-id>/meta.json` (cursus) or `.sgf/run/<run-id>.json` (non-cursus)
2. If not found → error: `run not found: <run-id>`, exit 1
3. For cursus runs: delegate to cursus resume logic (restores full pipeline state)
4. For non-cursus sessions: resume the most recent session via `cl --resume <session_id>`
5. Always resumes in interactive mode (full terminal passthrough) unless programmatic mode is detected (piped stdin)
6. Update metadata on exit: `status`, `updated_at`

### Resume Command Output

On any run exit (stall, interrupt, completion, error), sgf prints to stderr:

```
To resume:  sgf change --resume change-20260422T150000
```

In programmatic mode, this is included in the `run_complete` or `error` JSON event as `resume_command`.

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
| `stage` | string | Prompt stage name (e.g., `spec`, `build`, `verify`) |
| `spec` | string (optional) | Forma spec stem associated with this session, if any |
| `cursus` | string (optional) | Cursus name if this session was launched via a cursus pipeline. Used by `sgf resume` dispatch to distinguish cursus runs (delegate to cursus resume) from non-cursus sessions (flat picker). `null` for simple prompt mode (`sgf <file>`) and for non-cursus single-iter commands |
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
| `completed` | Agent exited with code 0 (sgf detected `.iter-complete` sentinel) |
| `interrupted` | SIGINT/SIGTERM (exit 130) |
| `exhausted` | Max iterations reached (exit 2) |

### Write Timing

1. **Before spawn**: Write metadata with `status: running`, empty `iterations` array
2. **After each iteration**: Append iteration record (with `session_id` and `completed_at`) to the `iterations` array, update `updated_at`
3. **On exit**: Update `status` based on exit code, update `updated_at`

All writes use atomic rename (write to `.tmp`, then rename into place). Atomic rename prevents corruption from crashes. Concurrent writes from multiple sgf processes targeting the same loop_id are not expected — each loop_id is unique per run.

For AFK mode, The iteration runner reports status directly to sgf.

## Session Handling

### Session ID Per Invocation

Every `cl` invocation receives a fresh `--session-id <uuid>` generated by sgf. There is no session continuity across iterations — each iteration is a completely fresh agent invocation.

### Resume

`--resume` is only used by `sgf resume` to let users revisit a previous session externally. It is not used across iterations within a loop.

### cl Invocation

```
cl --verbose --session-id <fresh-uuid> [existing flags...] @prompt.md
```

When resuming:

```
cl --verbose --resume <session_id>
```

The `--resume` flag restores the full session context from Claude Code's session store, so no prompt file or additional flags are needed.

## Springfield Changes

### orchestrate.rs

**Session flow (both modes)**:

1. Generate `session_id = Uuid::new_v4().to_string()` for each `cl` invocation
2. Write initial metadata: `write_session_metadata(root, &metadata)` with `status: "running"`, empty `iterations` array
3. Pass `--session-id <uuid>` to `cl`
4. After each invocation, append an iteration record to the `iterations` array with the invocation's `session_id` and `completed_at`
5. On exit, update metadata based on exit code:
   - Exit 0 → `status: "completed"`
   - Exit 2 → `status: "exhausted"`
   - Exit 130 → `status: "interrupted"`
   - Other → `status: "interrupted"`
6. Print resume command to stderr: `To resume: sgf <command> --resume <run-id>`

**Interactive mode** currently calls `cl` directly without a loop_id. This changes:
- Generate a loop_id for interactive sessions too (reusing `loop_mgmt::generate_loop_id`)
- No PID file needed (interactive is foreground)
- Metadata file written for resume capability

### main.rs

`--resume <run-id>` is a common flag parsed by clap on all subcommands. The `resume` built-in command is removed.

```rust
// --resume flag on all subcommands
if let Some(run_id) = args.resume {
    return run_resume(&root, &run_id, &command);
}
```

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

pub fn write_session_metadata(root: &Path, metadata: &SessionMetadata) -> io::Result<()>;
pub fn read_session_metadata(root: &Path, loop_id: &str) -> io::Result<Option<SessionMetadata>>;
pub fn list_session_metadata(root: &Path) -> io::Result<Vec<SessionMetadata>>;
```

`write_session_metadata` writes atomically (write to `.tmp`, rename) to prevent corrupt files on crash. Called after each iteration to persist the latest iteration's session ID.
`list_session_metadata` returns sessions sorted by `updated_at` descending, skipping corrupt files.

## Related Specifications

- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle
