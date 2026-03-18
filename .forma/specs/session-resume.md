# session-resume Specification

Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via sgf resume

| Field | Value |
|-------|-------|
| Src | `crates/springfield/` |
| Status | draft |

## Overview

Session tracking and resume mechanism for sgf. Persists Claude Code session IDs and loop configuration in JSON sidecar files, enabling users to resume interrupted sessions with `sgf resume`. Cursus uses this mechanism for per-iteration session tracking within pipeline runs.

The feature adds:
- **Session metadata persistence**: JSON sidecar files in `.sgf/run/{loop_id}.json` storing all iteration session IDs, loop config, and status
- **Pre-assigned session IDs**: Generate a UUID before each iteration and pass it via `--session-id <uuid>`, ensuring we always know the session ID without parsing output. Each iteration gets its own fresh session ID.
- **`sgf resume` command**: For cursus runs, resumes from the stalled/interrupted iter (see [cursus spec](cursus.md) Stall Recovery). For non-cursus sessions, picks from a flat list of all iterations across all loops, then resumes the selected session.
- **Ralph `--session-id` flag**: Every iteration passes `--session-id <uuid>` and includes the prompt. Iteration 1 uses the sgf-provided UUID; iterations 2+ generate a fresh UUID.
- **Both modes**: Works for interactive and AFK sessions

## Architecture

Changes span two crates:

### springfield (crates/springfield/)

- `loop_mgmt.rs`: New functions for session metadata read/write/list
- `orchestrate.rs`: Generate session UUID, write metadata after `cl` / ralph exits, new `sgf resume` command handler
- `main.rs`: Parse `sgf resume [loop_id]` as a reserved built-in command

### ralph (crates/ralph/)

- `main.rs`: New `--session-id <uuid>` CLI flag, passed through to `cl` as `--session-id <uuid>`. Every iteration gets its own session ID and includes the prompt. Iteration 1 uses the CLI-provided or sgf-generated UUID; iterations 2+ generate a fresh UUID.

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
 │                                      │  (uses sgf-provided uuid)  │
 │                                      │  (includes prompt)         │
 │                                      │                            │
 │  [iterations 2+]                     │                            │
 │                                      ├─ generate fresh UUID ──────┤
 │                                      ├─ pass --session-id <uuid> ─┤
 │                                      │  (includes prompt)         │
 │                                      │                            │
 │  [after each iteration]             │                            │
 │                                      ├─ report session_id to sgf ─┤
 │                                      │                            │
 ├─ append iteration to metadata ───────┤                            │
 │                                      │                            │
 │  [session ends]                      │                            │
 │                                      │                            │
 ├─ update metadata (status: final) ────┤                            │
 │                                      │                            │
 ├─ [later] sgf resume ────────────────►│                            │
 ├─ read metadata, show flat list ──────┤                            │
 │  of all iterations                   │                            │
 ├─ user picks one ─────────────────────┤                            │
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
  "iterations": [
    { "iteration": 1, "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", "completed_at": "2026-03-16T12:02:30Z" },
    { "iteration": 2, "session_id": "f9e8d7c6-b5a4-3210-fedc-ba9876543210", "completed_at": "2026-03-16T12:05:30Z" }
  ],
  "stage": "spec",
  "mode": "interactive",
  "prompt": ".sgf/prompts/spec.md",
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
| `running` | Written before spawning cl/ralph |
| `completed` | `.ralph-complete` sentinel detected (exit 0) |
| `interrupted` | SIGINT/SIGTERM (exit 130) |
| `exhausted` | Max iterations reached (exit 2) |

### Write Timing

1. **Before spawn**: Write metadata with `status: running`, empty `iterations` array
2. **After each iteration**: Append iteration record (with `session_id` and `completed_at`) to the `iterations` array, update `updated_at`
3. **On exit**: Update `status` based on exit code, update `updated_at`

For AFK mode, ralph reports iterations completed via its exit. Sgf maps exit codes to status values.

## sgf resume Command

### Usage

```
sgf resume [loop_id]
```

`resume` is a reserved built-in command (alongside `init`, `list`, `logs`).

### Behavior

**With `loop_id`**:
1. Read `.sgf/run/{loop_id}.json`
2. If not found → error, exit 1
3. Display the iterations for that loop as a numbered list, let user pick one
4. Launch `cl --resume <session_id> --verbose --dangerously-skip-permissions`
5. Always resumes in interactive mode (full terminal passthrough), regardless of original mode
6. Update metadata on exit: `status`, `updated_at`

**Without `loop_id`** (interactive picker):
1. Scan `.sgf/run/*.json` files
2. If none found → "No sessions found.", exit 1
3. Flatten all iterations across all loops into a single list, sorted by `completed_at` descending (newest first)
4. Display numbered list:
   ```
   Recent sessions:
     1. build-20260316T162408  iter 1   afk          completed    2m ago
     2. build-20260316T162408  iter 2   afk          completed    2m ago
     3. spec-20260316T120000   iter 1   interactive  interrupted  1h ago
   Select session (1-3):
   ```
5. Read user input (line from stdin)
6. Resume selected session using its `session_id`

### Display Format

Each line: `{index}. {loop_id}  iter {iteration}  {mode}  {status}  {relative_time}`

- `relative_time`: humanized from `completed_at` (e.g., "2m ago", "1h ago", "1d ago", "3d ago")
- Sorted by `completed_at` descending (newest first)
- Show at most 20 entries

### Resume Always Interactive

Regardless of the original session's mode (AFK or interactive), `sgf resume` always launches in interactive mode. The user is resuming to interact with the session directly. The session metadata is updated to reflect the new interaction.

## Ralph Changes

### New CLI Flags

| Flag | Type | Description |
|------|------|-------------|
| `--session-id <uuid>` | String | Pre-assigned Claude session ID for iteration 1. Passed through to `cl` as `--session-id <uuid>`. |
| `--resume <session_id>` | String | Resume a previous Claude session (used only by `sgf resume`, not across iterations). Passed through to `cl` as `--resume <session_id>`. Mutually exclusive with `--session-id`. |

### Iteration-Aware Session Handling

Both `run_interactive` and `run_afk` accept an `iteration` parameter (the 1-based loop counter `i`):

- **Iteration 1**: Pass `--session-id <uuid>` to `cl` using the CLI-provided or sgf-generated UUID. Include the prompt argument.
- **Iterations 2+**: Generate a fresh UUID (`Uuid::new_v4()`), pass `--session-id <uuid>` to `cl`. Include the prompt argument.

Every iteration is a completely fresh agent invocation. There is no `--resume` across iterations within a loop. `--resume` is only used by `sgf resume` to let users revisit a previous session externally.

### cl Invocation (Iteration 1)

```
cl --verbose --session-id <uuid> [existing flags...] @prompt.md
```

### cl Invocation (Iterations 2+)

```
cl --verbose --session-id <fresh-uuid> [existing flags...] @prompt.md
```

Note: Every iteration passes `--session-id` with a UUID and includes the prompt. Iterations 2+ use a ralph-generated fresh UUID.

### Session ID in sgf-to-ralph Contract

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `--session-id` | string (UUID) | sgf-generated (iteration 1) | Pre-assigned session ID for the first iteration |
| `--resume` | string (UUID) | sgf (from metadata, `sgf resume` only) | Session ID to resume from a previous run |

## Springfield Changes

### orchestrate.rs

**New session flow (both modes)**:

1. Generate `session_id = Uuid::new_v4().to_string()` for iteration 1
2. Write initial metadata: `write_session_metadata(root, &metadata)` with `status: "running"`, empty `iterations` array
3. Pass `--session-id <uuid>` to ralph (AFK) or `cl` (interactive)
4. After each iteration, append an iteration record to the `iterations` array with the iteration's `session_id` and `completed_at`
5. On exit, update metadata based on exit code:
   - Exit 0 → `status: "completed"`
   - Exit 2 → `status: "exhausted"`
   - Exit 130 → `status: "interrupted"`
   - Other → `status: "interrupted"`

For AFK mode, ralph generates fresh UUIDs for iterations 2+ and reports each iteration's session_id back. Sgf appends each to the metadata.

**Interactive mode** currently calls `cl` directly without a loop_id. This changes:
- Generate a loop_id for interactive sessions too (reusing `loop_mgmt::generate_loop_id`)
- No PID file needed (interactive is foreground)
- Metadata file written for resume capability

### main.rs

Add `resume` to the reserved built-in commands list (alongside `init`, `list`, `logs`).

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

- [ralph](ralph.md) — Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push
- [springfield](springfield.md) — CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle
