# Pensa — Agent Persistent Memory

## Purpose

Pensa is a Rust CLI (`pn`) that serves as the agent's persistent structured memory. It replaces markdown-based issue logging and implementation plan tracking with a single command interface backed by SQLite. A single command like `pn create "login crash on empty password" -p p0 -t bug` replaces the error-prone multi-step process of creating directories and writing markdown files.

Pensa lives at `crates/pensa/` in the Springfield workspace. It produces one binary: `pn`.

---

## Storage Model

Dual-layer storage:

- **`.pensa/db.sqlite`** — the working database, gitignored. Owned by the pensa daemon. Rebuilt from JSONL on clone.
- **`.pensa/*.jsonl`** — the git-committed exports. Separate files per entity: `issues.jsonl`, `deps.jsonl`, `comments.jsonl`. Events are not exported (derivable from issue history, avoids monotonic file growth). Human-readable, diffs cleanly.

Git sync is automated via prek (git hooks):

- **Pre-commit hook**: runs `pn export` to write SQLite → JSONL and stage the JSONL files.
- **Post-merge/post-checkout/post-rewrite hooks**: run `pn import` to rebuild JSONL → SQLite.

---

## Runtime Architecture

Pensa uses a client/daemon model.

### Why a daemon?

Docker sandboxes use Mutagen-based file synchronization (not bind mounts). POSIX file locks don't propagate across the sync boundary — two sandboxes writing to the same SQLite file would corrupt it. The daemon keeps SQLite on the host behind a single process, making concurrent access from multiple sandboxes safe.

### Daemon (`pn daemon`)

- Listens on a local port (default: `7533`).
- Owns `.pensa/db.sqlite` directly via `rusqlite`.
- Sets pragmas on every connection: `busy_timeout=5000`, `foreign_keys=ON`.
- All mutation is serialized through the daemon — no concurrent SQLite writers.
- Runs in the foreground (daemonization is the caller's responsibility — `sgf` backgrounds it).
- Stops on SIGTERM.
- The daemon needs to know the project root (where `.pensa/` lives). It accepts a `--project-dir` flag, defaulting to the current working directory.

### CLI client

- Every `pn` command (create, list, ready, close, etc.) sends an HTTP request to the daemon.
- The CLI discovers the daemon via `PN_DAEMON` env var (default: `http://localhost:7533`). Inside Docker sandboxes, this is `http://host.docker.internal:7533`.
- If the daemon is unreachable, the CLI fails with a clear error message and non-zero exit.

### Technology choices

- **Daemon HTTP server**: `axum` (tokio-based, good routing ergonomics for the ~20 endpoints).
- **CLI HTTP client**: `reqwest` (blocking mode — the CLI doesn't need async).
- **SQLite**: `rusqlite` with bundled SQLite (the `bundled` feature — avoids system SQLite version issues across host and sandbox environments).

---

## Schema

Everything is an issue (following the GitHub model). Issues are distinguished by `issue_type` — a required enum — rather than separate entity types.

### Issues table

```sql
CREATE TABLE issues (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    issue_type  TEXT NOT NULL CHECK (issue_type IN ('bug', 'task', 'test', 'chore')),
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'in_progress', 'closed')),
    priority    TEXT NOT NULL DEFAULT 'p2' CHECK (priority IN ('p0', 'p1', 'p2', 'p3')),
    spec        TEXT,
    fixes       TEXT REFERENCES issues(id),
    assignee    TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    closed_at   TEXT,
    close_reason TEXT
);
```

**`id`** — Format: `pn-` prefix + 8 hex chars from UUIDv7 (timestamp component + random bytes). Example: `pn-a1b2c3d4`. Short enough for agents to type, collision-resistant across concurrent agents and branches. Not content-based — two agents logging the same bug get different IDs.

**`issue_type`** (required, immutable after creation):
- **`bug`** — problems discovered during build/verify/test
- **`task`** — implementation plan items from the spec phase
- **`test`** — test plan items from the test-plan phase
- **`chore`** — tech debt, refactoring, dependency updates, CI fixes

**`spec`** (optional) — filename stem of the spec this issue implements (e.g., `auth` for `specs/auth.md`). There is no separate "implementation plan" entity — the living set of tasks linked to a spec *is* the implementation plan for that spec.

**`fixes`** (optional) — ID of a bug that this issue resolves. Set on `task` items created by `sgf issues plan`. When a task with a `fixes` link is closed, the linked bug is automatically closed with reason `"fixed by <task-id>"`.

**`priority`** — `p0` (critical), `p1` (high), `p2` (normal, default), `p3` (low). Smaller number = more urgent.

**`status`** — `open`, `in_progress`, `closed`.

### Dependencies table

```sql
CREATE TABLE deps (
    issue_id      TEXT NOT NULL REFERENCES issues(id),
    depends_on_id TEXT NOT NULL REFERENCES issues(id),
    PRIMARY KEY (issue_id, depends_on_id),
    CHECK (issue_id != depends_on_id)
);
```

Models blocking relationships. `pn ready` uses this to filter to unblocked issues.

### Comments table

```sql
CREATE TABLE comments (
    id         TEXT PRIMARY KEY,
    issue_id   TEXT NOT NULL REFERENCES issues(id),
    actor      TEXT NOT NULL,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

Comment IDs use the same format as issue IDs (`pn-` + 8 hex). Agents record observations about issues between fresh-context iterations without overwriting the description.

### Events table (audit log)

```sql
CREATE TABLE events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    issue_id   TEXT NOT NULL REFERENCES issues(id),
    event_type TEXT NOT NULL,
    actor      TEXT,
    detail     TEXT,
    created_at TEXT NOT NULL
);
```

Every mutation (create, update, close, reopen, claim, comment, dep add/remove) gets logged. Powers `pn history`. Events are not exported to JSONL — they are derivable from issue history and excluding them avoids monotonic file growth.

### Timestamps

All timestamps are ISO 8601 UTC strings (`2026-02-27T14:30:00Z`). Generated by the daemon, not the client.

---

## CLI Commands

The binary is named `pn`. All commands support `--json` for structured agent consumption (see JSON Output below).

### Global flags

- `--actor <name>` — who is running this command, for audit trail. Resolution order: `--actor` flag > `PN_ACTOR` env var > `git config user.name` > `$USER`.

### Working with issues

```
pn create "title" -t <issue_type> [-p <pri>] [-a <assignee>] [--spec <stem>] [--fixes <bug-id>] [--description <text>] [--dep <id>]
pn update <id> [--title <t>] [--status <s>] [--priority <p>] [-a <assignee>] [--description <d>] [--claim] [--unclaim]
pn close <id> [--reason "..."] [--force]
pn reopen <id> [--reason "..."]
pn release <id>
pn delete <id> [--force]
pn show <id> [--short]
```

**`--claim`** is atomic: `UPDATE ... SET status = 'in_progress', assignee = <actor> WHERE id = <id> AND status = 'open'`. If another agent already claimed the issue, the command fails with an `already_claimed` error (and reports who holds it). The agent should re-run `pn ready` and pick a different task.

**`--unclaim`** is shorthand for `--status open -a ""`.

**`pn release <id>`** is an alias for `pn update <id> --unclaim`.

**`pn close`** with `--force` allows closing regardless of current status. Without `--force`, closing a `closed` issue is an error. When closing an issue that has a `fixes` field, the linked bug is automatically closed with reason `"fixed by <task-id>"`.

**`pn delete`** requires `--force` if the issue has dependents or comments. Deletes the issue and all associated deps, comments, and events.

### Views and queries

```
pn list [--status <s>] [--priority <p>] [-a <assignee>] [-t <issue_type>] [--spec <stem>] [--sort <field>] [-n <limit>]
pn ready [-n <limit>] [-p <pri>] [-a <assignee>] [-t <issue_type>] [--spec <stem>]
pn blocked
pn search <query>
pn count [--by-status] [--by-priority] [--by-issue-type] [--by-assignee]
pn status
pn history <id>
```

**`pn ready`** returns open, unblocked issues sorted by priority then creation time. It only returns items with `issue_type` in (`task`, `test`, `chore`) — bugs are excluded entirely. Bugs are problem reports, not actionable work items. Returns `[]` when nothing matches.

**`pn list`** default sort is by priority (ascending) then created_at (ascending). The `--sort` flag accepts: `priority`, `created_at`, `updated_at`, `status`, `title`.

**`pn blocked`** returns issues that have at least one open dependency.

**`pn search`** does case-insensitive substring match on title + description.

**`pn count`** without grouping flags returns `{"count": N}` for all non-closed issues. With grouping flags returns breakdowns.

**`pn status`** returns a project health snapshot: open/in_progress/closed counts broken down by issue type.

**`pn history`** returns the event log for a single issue, newest first.

### Dependencies

```
pn dep add <child> <parent>
pn dep remove <child> <parent>
pn dep list <id>
pn dep tree <id> [--direction up|down]
pn dep cycles
```

**`pn dep add`** fails with `cycle_detected` if adding the dependency would create a cycle. The daemon checks for cycles before inserting.

**`pn dep tree`** with `--direction down` (default) shows what the issue blocks. `--direction up` shows what blocks the issue.

**`pn dep cycles`** scans for cycles and reports them. Should return `[]` in a healthy database.

### Comments

```
pn comment add <id> "text"
pn comment list <id>
```

### Daemon

```
pn daemon [--port <port>] [--project-dir <path>]
pn daemon status
```

**`pn daemon`** starts the daemon in the foreground on the specified port (default: `7533`). The `--project-dir` flag tells the daemon where `.pensa/` lives (default: current working directory). The daemon creates `.pensa/` and `db.sqlite` if they don't exist, runs migrations, and starts serving.

**`pn daemon status`** checks if the daemon is running and reachable. Prints the daemon URL and project directory if connected. Exits 0 if reachable, 1 if not.

### Data and maintenance

```
pn export
pn import
pn doctor [--fix]
pn where
```

**`pn export`** — dumps SQLite → JSONL files in `.pensa/`, then runs `git add .pensa/*.jsonl` to stage them. Issues, deps, and comments each get their own file.

**`pn import`** — rebuilds SQLite from the committed JSONL files. Drops and recreates tables, then inserts from JSONL. Used after clone or post-merge.

**`pn doctor [--fix]`** — health checks:
- Stale claims (in_progress issues with no recent activity)
- Orphaned dependencies (deps referencing non-existent issues)
- JSONL/SQLite sync drift

With `--fix`: releases all in_progress claims (set status → open, clear assignee) and repairs integrity issues (remove orphaned deps).

**`pn where`** — prints the `.pensa/` directory path. Useful for scripts and debugging.

---

## JSON Output

Following beads' pattern: no envelope, direct data to stdout.

### Routing

- Success data → stdout
- Errors → stderr
- Both as JSON when `--json` is active

### Exit codes

- `0` — success
- `1` — error

### Error shape (stderr)

```json
{"error": "issue not found: pn-a1b2c3d4", "code": "not_found"}
```

The `code` field is present only when there's a machine-readable error code. Known codes: `not_found`, `already_claimed`, `cycle_detected`, `invalid_status_transition`.

### Null arrays

Always `[]`, never `null`.

### Per-command output shapes (stdout)

| Command | Shape |
|---------|-------|
| `create`, `update`, `close`, `reopen`, `release` | Single issue object |
| `show` | Single issue detail object (issue fields + `deps`, `comments` arrays) |
| `list`, `ready`, `blocked`, `search` | Array of issue objects |
| `count` | `{"count": N}` or `{"total": N, "groups": [...]}` when grouped |
| `status` | Summary object (open/in_progress/closed counts by type) |
| `history` | Array of event objects |
| `dep add`, `dep remove` | `{"status": "added"/"removed", "issue_id": "...", "depends_on_id": "..."}` |
| `dep list` | Array of issue objects |
| `dep tree` | Array of tree nodes |
| `dep cycles` | Array of arrays (each inner array is one cycle) |
| `comment add` | Single comment object |
| `comment list` | Array of comment objects |
| `doctor` | Report object (findings array + fixes applied) |
| `export`, `import` | `{"status": "ok", "issues": N, "deps": N, "comments": N}` |

### Issue object fields

Mirror the schema: `id`, `title`, `description`, `issue_type`, `status`, `priority`, `spec`, `fixes`, `assignee`, `created_at`, `updated_at`, `closed_at`, `close_reason`. Absent optional fields are omitted (not `null`).

---

## Human-Readable Output

When `--json` is not set, commands produce human-readable table or list output suitable for terminal use. Specific formatting is left to implementation, but should be compact and scannable — similar to `git log --oneline` density.

---

## HTTP API

The daemon exposes a REST API. The CLI translates subcommands into HTTP requests.

### Endpoint mapping

| CLI command | Method | Path |
|-------------|--------|------|
| `create` | POST | `/issues` |
| `update` | PATCH | `/issues/:id` |
| `close` | POST | `/issues/:id/close` |
| `reopen` | POST | `/issues/:id/reopen` |
| `release` | POST | `/issues/:id/release` |
| `delete` | DELETE | `/issues/:id` |
| `show` | GET | `/issues/:id` |
| `list` | GET | `/issues` |
| `ready` | GET | `/issues/ready` |
| `blocked` | GET | `/issues/blocked` |
| `search` | GET | `/issues/search?q=...` |
| `count` | GET | `/issues/count` |
| `status` | GET | `/status` |
| `history` | GET | `/issues/:id/history` |
| `dep add` | POST | `/deps` |
| `dep remove` | DELETE | `/deps` |
| `dep list` | GET | `/issues/:id/deps` |
| `dep tree` | GET | `/issues/:id/deps/tree` |
| `dep cycles` | GET | `/deps/cycles` |
| `comment add` | POST | `/issues/:id/comments` |
| `comment list` | GET | `/issues/:id/comments` |
| `export` | POST | `/export` |
| `import` | POST | `/import` |
| `doctor` | POST | `/doctor` |
| `where` | GET | `/where` |

All endpoints accept and return JSON. Query parameters map to CLI filter flags.

---

## JSONL Format

Each JSONL file contains one JSON object per line. Objects use the same field names as the JSON output (see Issue object fields above).

### `issues.jsonl`

One line per issue. All fields included (optional fields omitted when absent). Sorted by `created_at` for stable diffs.

### `deps.jsonl`

One line per dependency: `{"issue_id": "...", "depends_on_id": "..."}`. Sorted by `issue_id` then `depends_on_id`.

### `comments.jsonl`

One line per comment: `{"id": "...", "issue_id": "...", "actor": "...", "text": "...", "created_at": "..."}`. Sorted by `created_at`.

---

## Database Initialization

When the daemon starts:

1. Create `.pensa/` directory if it doesn't exist.
2. Open (or create) `.pensa/db.sqlite`.
3. Set pragmas: `busy_timeout=5000`, `foreign_keys=ON`.
4. Run migrations — create tables if they don't exist.
5. If JSONL files exist but the database is empty, automatically import from JSONL (handles fresh clone scenario).

---

## Testing Strategy

Pensa should be end-to-end testable from the command line. Tests start a daemon on a random port, run `pn` commands against it via `PN_DAEMON`, and assert on stdout/stderr/exit codes.

Key scenarios:

- **CRUD lifecycle**: create → show → update → close → reopen → close
- **Claim semantics**: create → claim → second claim fails with `already_claimed` → release → claim succeeds
- **Dependencies**: add dep → verify `ready` excludes blocked item → close blocker → verify item now appears in `ready`
- **Cycle detection**: add deps forming a cycle → verify `cycle_detected` error
- **`fixes` auto-close**: create bug → create task with `--fixes` → close task → verify bug is also closed
- **`ready` excludes bugs**: create bug → verify `pn ready` does not include it
- **Export/import round-trip**: create issues with deps and comments → export → delete db → import → verify all data intact
- **Doctor**: create in_progress issues → `doctor --fix` → verify all released to open
- **Concurrent claims**: two rapid claim attempts on the same issue → exactly one succeeds
- **JSON output**: verify `--json` output matches documented shapes for each command
