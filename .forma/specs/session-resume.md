# session-resume Specification

Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via sgf resume

| Field | Value |
|-------|-------|
| Src | `crates/springfield/,crates/ralph/` |
| Status | draft |

## Overview

Resume interrupted or completed `sgf` sessions by persisting Claude Code session IDs and loop configuration in JSON sidecar files. Users can jump back into any previous session with `sgf resume [loop_id]`.

The feature adds:
- **Session metadata persistence**: JSON sidecar files in `.sgf/run/{loop_id}.json` storing the Claude session ID, loop config, and status
- **Pre-assigned session IDs**: Generate a UUID before each `cl` invocation and pass it via `--session-id <uuid>`, ensuring we always know the session ID without parsing output
- **`sgf resume` command**: Resume a session by loop ID, or interactively pick from recent sessions
- **Ralph `--resume` flag**: Pass `--resume <session_id>` to `cl` to continue a previous Claude conversation
- **Both modes**: Works for interactive and AFK sessions

## Architecture

Changes span two crates:

### springfield (crates/springfield/)

- `loop_mgmt.rs`: New functions for session metadata read/write/list
- `orchestrate.rs`: Generate session UUID, write metadata after `cl` / ralph exits, new `sgf resume` command handler
- `main.rs`: Parse `sgf resume [loop_id]` as a reserved built-in command

### ralph (crates/ralph/)

- `main.rs`: New `--resume <session_id>` CLI flag, passed through to `cl` as `--resume <session_id>`. New `--session-id <uuid>` flag, passed through to `cl` as `--session-id <uuid>`. On iteration 1, uses `--session-id` to create a new session. On iterations 2+, uses `--resume` with the same UUID to continue the existing session (and omits the prompt argument). This prevents "session ID already in use" errors from Claude Code on multi-iteration runs.

### Session Metadata File

```
.sgf/run/{loop_id}.json
```

Co-located with existing `.sgf/run/{loop_id}.pid` files. Gitignored (`.sgf/run/` is already in `.gitignore`).

### Data Flow

```
sgf                                    ralph                        cl (claude-wrapper)
 │                                      │                            │
 ├─ generate session UUID ──────────────┤                            │
 ├─ write metadata (status: running) ───┤                            │
 ├─ pass --session-id <uuid> ───────────┤                            │
 │                                      │                            │
 │  [iteration 1]                       │                            │
 │                                      ├─ pass --session-id <uuid> ─┤
 │                                      │                            │
 │  [iterations 2+]                     │                            │
 │                                      ├─ pass --resume <uuid> ─────┤
 │                                      │  (prompt arg omitted)      │
 │                                      │                            │
 │  [session ends]                      │                            │
 │                                      │                            │
 ├─ update metadata (status: final) ────┤                            │
 │                                      │                            │
 ├─ [later] sgf resume <loop_id> ──────►│                            │
 ├─ read metadata, get session_id ──────┤                            │
 ├─ pass --resume <session_id> ─────────┼─ pass --resume <id> ──────►│
```

For interactive mode (no ralph), sgf calls `cl` directly with `--session-id <uuid>`, then writes metadata on exit.

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
| `sgf resume` with no sessions available | Print "No sessions found." to stderr, exit 1 |
| `sgf resume <loop_id>` with unknown loop_id | Print "Session not found: <loop_id>" to stderr, exit 1 |
| Metadata file missing/corrupt | Print "Session metadata not found or corrupt for <loop_id>" to stderr, exit 1 |
| Metadata write failure (disk full, permissions) | `tracing::warn\!`, continue without metadata (session still runs, just can't be resumed) |
| `cl --resume` fails (session expired/gone) | Claude Code handles this — it falls back to a new session. Not our error to handle. |
| Picker selection cancelled (Ctrl+C during picker) | Exit 0 |

## Testing

### Unit Tests (springfield — loop_mgmt.rs)

| Test | Asserts |
|------|---------|
| Write and read session metadata roundtrip | JSON file created, fields match |
| List sessions returns sorted by timestamp (newest first) | Order correct, all fields present |
| List sessions skips corrupt JSON files | Corrupt file ignored, valid files returned |
| List sessions returns empty vec when no JSON files | No panic, empty result |
| Read metadata for nonexistent loop_id | Returns None |

### Unit Tests (ralph — main.rs)

| Test | Asserts |
|------|---------|
| `--resume` flag adds `--resume <id>` to cl args | Arg vector contains `--resume` and session_id |
| `--session-id` flag adds `--session-id <uuid>` to cl args | Arg vector contains `--session-id` and uuid |

### Integration Tests (springfield — E2E)

| Test | Asserts |
|------|---------|
| AFK session writes metadata with session_id | Run sgf with mock ralph, check `.sgf/run/{loop_id}.json` exists and contains `session_id` |
| Interactive session writes metadata with session_id | Run sgf with mock cl, check metadata file |
| `sgf resume <loop_id>` passes `--resume` to cl | Mock cl receives `--resume <session_id>` in args |
| `sgf resume` with no sessions exits 1 | Exit code 1, stderr contains error |
| `sgf resume <bad_id>` exits 1 | Exit code 1, stderr contains error |
| Metadata survives interrupted session (Ctrl+C) | Send SIGINT, metadata file still has `session_id` and status `interrupted` |

### CLI Verification (manual / scripted)

```bash
# Start a session, Ctrl+C out
sgf spec -i
# Check metadata was written
cat .sgf/run/spec-*.json
# Resume it
sgf resume   # should show picker
sgf resume spec-20260316T120000   # direct resume
```

## Session Metadata Schema

### File: `.sgf/run/{loop_id}.json`

```json
{
  "loop_id": "spec-20260316T120000",
  "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "stage": "spec",
  "spec": null,
  "mode": "interactive",
  "prompt": ".sgf/prompts/spec.md",
  "iterations_completed": 1,
  "iterations_total": 1,
  "status": "interrupted",
  "created_at": "2026-03-16T12:00:00Z",
  "updated_at": "2026-03-16T12:05:30Z"
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `loop_id` | string | The loop identifier (same as filename stem) |
| `session_id` | string (UUID) | Claude Code session ID, pre-assigned via `--session-id` |
| `stage` | string | Prompt stage name (e.g., `spec`, `build`, `verify`) |
| `spec` | string \| null | Spec stem if provided, null otherwise |
| `mode` | string | `"interactive"` or `"afk"` |
| `prompt` | string | Resolved prompt file path |
| `iterations_completed` | u32 | Number of iterations completed so far |
| `iterations_total` | u32 | Total iterations configured |
| `status` | string | One of: `running`, `completed`, `interrupted`, `exhausted` |
| `created_at` | string (ISO 8601) | When the session started |
| `updated_at` | string (ISO 8601) | When the metadata was last written |

### Status Values

| Status | When set |
|--------|----------|
| `running` | Written before spawning cl/ralph |
| `completed` | `.ralph-complete` sentinel detected (exit 0) |
| `interrupted` | SIGINT/SIGTERM (exit 130) |
| `exhausted` | Max iterations reached (exit 2) |

### Write Timing

1. **Before spawn**: Write metadata with `status: running`, `iterations_completed: 0`
2. **On exit**: Update `status` based on exit code, update `iterations_completed` and `updated_at`

For AFK mode, ralph reports iterations completed via its exit. Sgf maps exit codes to status values.

## sgf resume Command

### Usage

```
sgf resume [loop_id]
```

`resume` is a reserved built-in command (alongside `init`, `logs`, `status`).

### Behavior

**With `loop_id`**:
1. Read `.sgf/run/{loop_id}.json`
2. If not found → error, exit 1
3. Launch `cl --resume <session_id> --verbose --dangerously-skip-permissions --settings '{...}'`
4. Always resumes in interactive mode (full terminal passthrough), regardless of original mode
5. Update metadata on exit: `status`, `updated_at`

**Without `loop_id`** (interactive picker):
1. Scan `.sgf/run/*.json` files
2. If none found → "No sessions found.", exit 1
3. Display numbered list, newest first:
   ```
   Recent sessions:
     1. spec-20260316T120000       interactive  interrupted  2m ago
     2. build-auth-20260316T110000 afk          exhausted    1h ago
     3. verify-20260315T090000     afk          completed    1d ago
   Select session (1-3):
   ```
4. Read user input (line from stdin)
5. Resume selected session

### Display Format

Each line: `{index}. {loop_id}  {mode}  {status}  {relative_time}`

- `relative_time`: humanized from `updated_at` (e.g., "2m ago", "1h ago", "1d ago", "3d ago")
- Sorted by `updated_at` descending (newest first)
- Show at most 20 sessions

### Resume Always Interactive

Regardless of the original session's mode (AFK or interactive), `sgf resume` always launches in interactive mode. The user is resuming to interact with the session directly. The session metadata is updated to reflect the new interaction.

## Ralph Changes

### New CLI Flags

| Flag | Type | Description |
|------|------|-------------|
| `--session-id <uuid>` | String | Pre-assigned Claude session ID. Passed through to `cl` as `--session-id <uuid>` on iteration 1. On iterations 2+, automatically switches to `--resume <uuid>` instead. |
| `--resume <session_id>` | String | Resume a previous Claude session. Passed through to `cl` as `--resume <session_id>`. Mutually exclusive with `--session-id`. |

### Iteration-Aware Session Handling

Both `run_interactive` and `run_afk` accept an `iteration` parameter (the 1-based loop counter `i`):

- **Iteration 1**: Pass `--session-id <uuid>` to `cl` to create a new Claude session. Include the prompt argument.
- **Iterations 2+**: Pass `--resume <uuid>` to `cl` to continue the existing session. Omit the prompt argument (Claude continues from the previous conversation context).

This prevents Claude Code from rejecting the session ID with "already in use" on subsequent iterations, since `--session-id` creates a session while `--resume` reconnects to one.

### cl Invocation (Iteration 1)

```
cl --verbose --session-id <uuid> [existing flags...] @prompt.md
```

### cl Invocation (Iterations 2+)

```
cl --verbose --resume <uuid> [existing flags...]
```

Note: When resuming, the prompt argument is omitted — Claude Code continues from the previous conversation context.

### Session ID in sgf-to-ralph Contract

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `--session-id` | string (UUID) | sgf-generated | Pre-assigned session ID for new sessions |
| `--resume` | string (UUID) | sgf (from metadata) | Session ID to resume |

## Springfield Changes

### orchestrate.rs

**New session flow (both modes)**:

1. Generate `session_id = Uuid::new_v4().to_string()`
2. Write initial metadata: `write_session_metadata(root, &metadata)` with `status: "running"`
3. Pass `--session-id <uuid>` to ralph (AFK) or `cl` (interactive)
4. On exit, update metadata based on exit code:
   - Exit 0 → `status: "completed"`
   - Exit 2 → `status: "exhausted"`
   - Exit 130 → `status: "interrupted"`
   - Other → `status: "interrupted"`

**Interactive mode** currently calls `cl` directly without a loop_id. This changes:
- Generate a loop_id for interactive sessions too (reusing `loop_mgmt::generate_loop_id`)
- No PID file needed (interactive is foreground)
- Metadata file written for resume capability

### main.rs

Add `resume` to the reserved built-in commands list (alongside `init`, `logs`, `status`).

```rust
"resume" => {
    let loop_id = positional_args.first().map(|s| s.as_str());
    return run_resume(&root, loop_id);
}
```

### loop_mgmt.rs

New functions:

```rust
#[derive(Serialize, Deserialize)]
pub struct SessionMetadata {
    pub loop_id: String,
    pub session_id: String,
    pub stage: String,
    pub spec: Option<String>,
    pub mode: String,
    pub prompt: String,
    pub iterations_completed: u32,
    pub iterations_total: u32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn write_session_metadata(root: &Path, metadata: &SessionMetadata) -> io::Result<()>;
pub fn read_session_metadata(root: &Path, loop_id: &str) -> io::Result<Option<SessionMetadata>>;
pub fn list_session_metadata(root: &Path) -> io::Result<Vec<SessionMetadata>>;
```

`write_session_metadata` writes atomically (write to `.tmp`, rename) to prevent corrupt files on crash.
`list_session_metadata` returns sessions sorted by `updated_at` descending, skipping corrupt files.

## Related Specifications

- [ralph](ralph.md) — Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle
