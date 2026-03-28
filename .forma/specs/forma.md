# forma Specification

Specification management — forma daemon and fm CLI

| Field | Value |
|-------|-------|
| Src | `crates/forma/` |
| Status | stable |

## Overview

Forma is a Rust CLI (`fm`) that manages specifications for any repository. It replaces manual markdown-based spec tracking with a single command interface backed by SQLite. A single command like `fm create auth --src crates/auth/ --purpose "Authentication"` replaces the error-prone process of creating markdown files and updating index tables by hand.

Specs are the source of truth for all code. Forma ensures they are structured, validated, and queryable. All spec mutations go through `fm` — agents and humans read the generated markdown, but never edit it directly.

Forma lives at `crates/forma/` in the Springfield workspace. It produces one binary: `fm`.

## Architecture

```
crates/forma/
├── src/
│   ├── main.rs       # CLI entry, clap commands, HTTP client, daemon auto-start
│   ├── lib.rs        # Library root, re-exports modules for integration test access
│   ├── client.rs     # HTTP client for communicating with the forma daemon
│   ├── db.rs         # SQLite schema, migrations, all database operations
│   ├── daemon.rs     # Axum HTTP server, route handlers
│   ├── types.rs      # Spec, Section, Ref, Event, Status, RequiredSection, slugify
│   └── output.rs     # Human-readable formatting (non-JSON output)
├── tests/
│   ├── integration.rs  # CLI-level integration tests via std::process::Command
│   └── cli_client.rs   # Client API integration tests against a test daemon
└── Cargo.toml
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (4, derive + env) | CLI argument parsing with env var support |
| `axum` (0.8) | Daemon HTTP server |
| `tokio` (1, full) | Async runtime for daemon |
| `reqwest` (0.12, blocking + json) | CLI HTTP client |
| `rusqlite` (0.35, bundled) | SQLite with bundled SQLite |
| `serde` (1, derive) | Serialization |
| `serde_json` (1) | JSON handling |
| `uuid` (1, v7) | UUIDv7 ID generation |
| `chrono` (0.4, serde) | Timestamp generation |
| `sha2` (0.10) | SHA-256 for port/path derivation |
| `tracing` (0.1) | Structured logging |
| `tracing-subscriber` (0.3) | Log output formatting |

Dev dependencies:

| Crate | Purpose |
|-------|---------|
| `tempfile` (3) | Temporary directories for test isolation |
| `portpicker` (0.1) | Random port selection for test daemons |
| `proptest` (1) | Property-based testing |

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Spec not found | `not_found` error, exit 1 |
| Spec already exists | `already_exists` error, exit 1 |
| Section not found | `not_found` error, exit 1 |
| Remove required section | `required_section` error, exit 1 |
| Ref cycle detected | `cycle_detected` error, exit 1 |
| Ref target doesn't exist | `not_found` error, exit 1 |
| Daemon unreachable (local) | Auto-start daemon, retry |
| Daemon unreachable (remote) | Error, exit 1 |
| Pensa daemon unreachable (during `fm check`) | Skip pensa validation, warn |
| Empty stdin on `--body-stdin` | Accept empty body (valid for clearing a section) |
| Invalid stem (spaces, uppercase) | Reject at CLI level with validation error |

## Testing

Forma should be end-to-end testable from the command line. Tests start a daemon on a random port, run `fm` commands against it via `FM_DAEMON`, and assert on stdout/stderr/exit codes.

Key scenarios:

- **Spec CRUD lifecycle**: create → show → update → delete
- **Section lifecycle**: create spec → section add → section set → section get → section list → section move → section remove
- **Required sections scaffolded**: create spec → section list → verify 5 required sections exist
- **Required sections protected**: create spec → section remove on required slug → verify error
- **Ref lifecycle**: create two specs → ref add → ref list → ref remove
- **Ref cycle detection**: A refs B → B refs C → C refs A → verify `cycle_detected` error
- **Ref tree**: create chain A → B → C → verify tree output shows depth
- **Status transitions**: create (draft) → update stable → update proven
- **Search**: create specs with known content → search → verify matches
- **Export/import round-trip**: create specs with sections and refs → export → delete db → import → verify all data intact
- **Markdown generation**: create spec with sections and refs → export → verify generated markdown structure
- **README generation**: create multiple specs → export → verify README table
- **fm check**: create spec with empty required section → check → verify warning. Create ref to non-existent spec → check → verify error
- **fm doctor**: create orphaned data → doctor --fix → verify cleaned up
- **History**: create spec → update → section set → verify history shows all events
- **JSON output**: verify `--json` output matches documented shapes for each command
- **Pensa integration**: start both daemons → `pn create --spec valid-stem` succeeds → `pn create --spec nonexistent` fails
- **Slug generation**: verify "Error Handling" → `error-handling`, "NDJSON Stream Formatting" → `ndjson-stream-formatting`

## Storage Model

Dual-layer storage:

- **`~/.local/share/forma/<project-hash>/db.sqlite`** — the working database, stored outside the workspace to keep binary files out of git. Lives on the host, owned by the forma daemon. Rebuilt from JSONL on clone. The `<project-hash>` is a 16-hex-char hash of the canonical project directory path (SHA-256, first 8 bytes as hex).
- **`.forma/*.jsonl`** — the git-committed exports. Separate files per entity: `specs.jsonl`, `sections.jsonl`, `refs.jsonl`. Events are not exported (derivable from history, avoids monotonic file growth). Human-readable, diffs cleanly. JSONL files are never read at runtime — they capture a snapshot at commit time via `fm export` and are only used to rebuild SQLite on clone or post-merge via `fm import`.

Generated artifacts (also committed, produced by `fm export`):

- **`.forma/specs/*.md`** — generated markdown for each spec. Human-readable, reviewable in PRs, browsable on GitHub. Never edited directly.
- **`.forma/README.md`** — generated spec index table. Replaces the old manual `specs/README.md`.

Transient files (gitignored):

- **`.forma/daemon.port`** — written by the daemon on startup, contains the port number.
- **`.forma/daemon.project`** — written by the daemon on startup, contains the canonical project directory path. Used by the CLI to detect stale daemons.
- **`.forma/daemon.url`** — optional override for daemon address. If present, `fm` treats the daemon as remote (no auto-start).

Git sync is automated via prek (git hooks):

- **Pre-commit hook**: runs `fm export` to write SQLite → JSONL + markdown and stage the files.
- **Post-merge/post-checkout/post-rewrite hooks**: run `fm import` to rebuild JSONL → SQLite.

## Runtime Architecture

Forma uses a client/daemon model. The daemon runs on the host, owns the SQLite database, and handles all reads and writes. The `fm` CLI is a thin client that connects to the daemon over HTTP.

### Why a daemon?

SQLite needs serialized write access. Multiple agents may update different specs concurrently during parallel `sgf build` loops. The daemon keeps all reads and writes behind a single process, preventing concurrent SQLite writers.

### Daemon (`fm daemon`)

- Listens on a per-project derived port. Port derivation: SHA-256 of `"forma:" + canonical_project_path`, bytes 8-9 mapped to range [10000, 59999]. The `"forma:"` prefix ensures forma and pensa derive different ports for the same project.
- Owns the database directly via `rusqlite`.
- Sets pragmas on every connection: `busy_timeout=5000`, `foreign_keys=ON`.
- All mutation is serialized through the daemon — no concurrent SQLite writers.
- Runs in the foreground (daemonization is the caller's responsibility — `sgf` backgrounds it).
- Stops on SIGTERM.
- The daemon needs to know the project root (where `.forma/` lives). It accepts a `--project-dir` flag, defaulting to the current working directory.

### CLI client

- Every `fm` command (create, list, show, etc.) sends an HTTP request to the daemon.
- The CLI discovers the daemon address via env vars and port discovery. Resolution order: (1) if `FM_DAEMON_HOST` is set and non-empty, use `http://<host>:<port>` with port from discovery; (2) if `FM_DAEMON` is set, use it as the full URL; (3) if `.forma/daemon.url` exists and contains a non-empty URL, use it; (4) otherwise use `http://localhost:<port>`. Port discovery checks `.forma/daemon.port` (written by the daemon on startup), falling back to SHA-256 derivation of the project directory.
- If the daemon is unreachable and a remote host is configured — `FM_DAEMON_HOST` is set to something other than empty/`localhost`/`127.0.0.1`/`::1`, `FM_DAEMON` is explicitly set, or `.forma/daemon.url` exists with content — the CLI prints an error and exits (exit code 1). It never auto-starts a daemon when a remote daemon address is configured. Otherwise (local host), the CLI auto-starts it (spawning `fm daemon` in the background with the current working directory as `--project-dir`), waits up to 5 seconds for it to become ready, then proceeds. If the daemon still isn't reachable after 5 seconds, the command continues anyway (the HTTP call will fail with a clear error). The `daemon` and `where` subcommands skip auto-start.
- **Stale daemon detection**: before checking reachability, the CLI reads `.forma/daemon.project` (if it exists) and compares the path inside to the current working directory. If they differ, the daemon was started for a different project directory. The CLI removes `.forma/daemon.port` and `.forma/daemon.project`, then proceeds to start a fresh daemon.

### Technology choices

- **Daemon HTTP server**: `axum` (tokio-based).
- **CLI HTTP client**: `reqwest` (blocking mode — the CLI doesn't need async).
- **SQLite**: `rusqlite` with bundled SQLite (the `bundled` feature).

## Daemon Lifecycle

### `/shutdown` Endpoint (Internal)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/shutdown` | Triggers graceful daemon shutdown |

The `/shutdown` endpoint is internal-only — used by test fixtures and tooling, not by the `fm` CLI. It triggers a `tokio::sync::Notify` that participates in the daemon's `tokio::select\!` shutdown signal alongside SIGTERM, Ctrl+C, and the project-dir watchdog.

No request body. Returns `200 OK` immediately. The daemon completes in-flight requests then exits.

### Project Directory Watchdog

The daemon monitors the existence of its `--project-dir` on a fixed 5-second interval. If the directory does not exist for 3 consecutive checks (15 seconds total), the daemon shuts down gracefully. This prevents the daemon from running indefinitely after the project directory is deleted (e.g., temp dirs in tests, renamed projects).

The watchdog requires 3 consecutive failures before triggering shutdown to tolerate transient filesystem issues (NFS flakes, permission blips, momentary unmounts). A single successful check resets the failure counter to zero.

The 5-second interval is hardcoded — not configurable.

### `FormaState`

```rust
struct FormaState {
    db: Mutex<Db>,
    project_dir: PathBuf,
    shutdown: Notify,
}
```

`FormaState` is the shared state passed to all Axum route handlers via `State<AppState>` (where `AppState = Arc<FormaState>`). `project_dir` is the `--project-dir` value passed at daemon startup, stored as a `PathBuf`. `shutdown` is a `tokio::sync::Notify` used by the `/shutdown` endpoint to signal the main `tokio::select\!` loop.

### Shutdown Conditions

The daemon's main loop uses `tokio::select\!` to await any of four conditions:

1. **Ctrl+C** (`tokio::signal::ctrl_c()`)
2. **SIGTERM** (`tokio::signal::unix::signal(SignalKind::terminate())`)
3. **`/shutdown` endpoint** (`state.shutdown.notified()`)
4. **Project directory watchdog** (3 consecutive failures at 5s interval)

Whichever fires first triggers graceful shutdown.

## Schema

### Specs table

```sql
CREATE TABLE specs (
    stem       TEXT PRIMARY KEY,
    src        TEXT,
    purpose    TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'stable', 'proven')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

**`stem`** — the unique identifier for the spec. Lowercase, alphanumeric + hyphens. Examples: `auth`, `ralph`, `claude-wrapper`. Used as the primary key and in all CLI commands.

**`src`** — optional relative path to the source directory (e.g., `crates/auth/`, `frontend/src/auth/`). Null for specs not tied to a specific directory.

**`purpose`** — one-line description of what the spec covers.

**`status`** — lifecycle state:
- **`draft`** — spec is being written, not yet implementable.
- **`stable`** — spec is approved and implementation can proceed.
- **`proven`** — spec is fully reflected in the codebase (implementation complete, tests passing).

### Sections table

```sql
CREATE TABLE sections (
    id         TEXT PRIMARY KEY,
    spec_stem  TEXT NOT NULL REFERENCES specs(stem),
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('required', 'custom')),
    body       TEXT NOT NULL DEFAULT '',
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(spec_stem, slug)
);
```

**`id`** — Format: `fm-` prefix + 8 hex chars from UUIDv7. Example: `fm-a1b2c3d4`.

**`slug`** — URL-safe identifier derived from the display name. Lowercase, spaces to hyphens, non-alphanumeric characters stripped. Examples: `overview`, `error-handling`, `ndjson-stream-formatting`. Used in CLI commands and HTTP routes.

**`name`** — human-readable display name. Examples: `Overview`, `Error Handling`, `NDJSON Stream Formatting`.

**`kind`** — `required` for the five standard sections, `custom` for everything else.

**`position`** — integer ordering within the spec. Required sections are scaffolded at positions 0-4. Custom sections are inserted at the end by default, or after a specified section via `--after`.

### Required sections

Five required sections are created automatically when a spec is created via `fm create`:

| Position | Name | Slug | Purpose |
|----------|------|------|---------|
| 0 | Overview | `overview` | What this crate/module does |
| 1 | Architecture | `architecture` | Directory structure, module layout |
| 2 | Dependencies | `dependencies` | Crate/package dependencies |
| 3 | Error Handling | `error-handling` | Error types, failure modes, recovery |
| 4 | Testing | `testing` | Test strategy, key scenarios |

Required sections cannot be removed via `fm section remove`. Their bodies start empty — `fm check` flags empty required sections as warnings.

### Refs table

```sql
CREATE TABLE refs (
    from_stem TEXT NOT NULL REFERENCES specs(stem),
    to_stem   TEXT NOT NULL REFERENCES specs(stem),
    PRIMARY KEY (from_stem, to_stem),
    CHECK (from_stem \!= to_stem)
);
```

Models directional cross-references between specs. `fm ref add auth ralph` means the auth spec references the ralph spec. Used to auto-generate the "Related Specifications" section in exported markdown.

### Events table (audit log)

```sql
CREATE TABLE events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    spec_stem  TEXT NOT NULL REFERENCES specs(stem),
    event_type TEXT NOT NULL,
    actor      TEXT,
    detail     TEXT,
    created_at TEXT NOT NULL
);
```

Every mutation (create, update, delete, section add/set/remove/move, ref add/remove) gets logged. Powers `fm history`. Events are not exported to JSONL.

Known event types: `created`, `updated`, `deleted`, `section_added`, `section_updated`, `section_removed`, `section_moved`, `ref_added`, `ref_removed`.

### Timestamps

All timestamps are ISO 8601 UTC strings (`2026-03-14T14:30:00Z`). Generated by the daemon, not the client.

## CLI Commands

The binary is named `fm`. All commands support `--json` for structured agent consumption.

### Global flags

- `--actor <name>` — who is running this command, for audit trail. Resolution order: `--actor` flag > `FM_ACTOR` env var > `git config user.name` > `$USER`.
- `--json` — structured JSON output to stdout.

### Working with specs

```
fm create <stem> [--src <path>] --purpose "<text>"
fm show <stem>
fm list [--status <status>]
fm update <stem> [--status <status>] [--src <path>] [--purpose "<text>"]
fm delete <stem> [--force]
fm search "<query>"
fm count [--by-status]
fm status
fm history <stem>
```

**`fm create`** creates a new spec with the given stem, crate path, and purpose. Automatically scaffolds the five required sections with empty bodies. Fails if the stem already exists.

**`fm show`** returns the full spec: metadata, all sections (in position order), and refs. Without `--json`, renders as markdown. With `--json`, returns structured data.

**`fm list`** returns all specs. Filterable by status.

**`fm update`** modifies spec metadata fields. At least one field must be provided.

**`fm delete`** removes a spec and all its sections, refs, and events. Requires `--force` if the spec has sections with non-empty bodies (safety check against accidental deletion of authored content).

**`fm search`** does case-insensitive substring match across spec stems, purposes, and section bodies.

**`fm count`** without flags returns `{"count": N}` for all specs. With `--by-status` returns a breakdown.

**`fm status`** returns a summary: count of specs by status.

**`fm history`** returns the event log for a spec, newest first.

### Working with sections

```
fm section add <stem> "<name>" --body-stdin [--after "<slug>"]
fm section set <stem> "<slug>" --body-stdin
fm section get <stem> "<slug>"
fm section list <stem>
fm section remove <stem> "<slug>"
fm section move <stem> "<slug>" --after "<slug>"
```

**`fm section add`** creates a new custom section. The slug is auto-generated from the name. Body is read from stdin via `--body-stdin`. Without `--after`, the section is appended at the end. With `--after`, it is inserted after the specified section (other positions are renumbered).

**`fm section set`** replaces the body of an existing section. Body is read from stdin.

**`fm section get`** returns a single section. Without `--json`, prints the body. With `--json`, returns `{name, slug, kind, body}`.

**`fm section list`** returns all sections for a spec, ordered by position.

**`fm section remove`** deletes a custom section. Refuses to delete required sections (exits with error).

**`fm section move`** changes a section's position to be after the specified target section. All other positions are renumbered.

### Working with refs

```
fm ref add <stem> <target-stem>
fm ref remove <stem> <target-stem>
fm ref list <stem>
fm ref tree <stem> [--direction up|down]
fm ref cycles
```

**`fm ref add`** creates a directional cross-reference. Fails if either spec doesn't exist, or if the ref already exists, or if adding it would create a cycle.

**`fm ref remove`** deletes a cross-reference.

**`fm ref list`** returns specs that the given spec references.

**`fm ref tree`** with `--direction down` (default) shows what the spec references, recursively. `--direction up` shows what references the spec, recursively.

**`fm ref cycles`** scans for cycles and reports them. Should return `[]` in a healthy database.

### Data and maintenance

```
fm export
fm import
fm check
fm doctor [--fix]
fm where
```

**`fm export`** — the daemon dumps SQLite → JSONL files in `.forma/`, generates `.forma/specs/*.md` and `.forma/README.md`. The CLI then runs `git add .forma/` to stage everything.

**`fm import`** — rebuilds SQLite from the committed JSONL files. Drops and recreates tables, then inserts from JSONL. Used after clone or post-merge.

**`fm check`** — validation report (see Validation section below).

**`fm doctor [--fix]`** — health checks:
- JSONL/SQLite sync drift (count mismatch)
- Orphaned refs (referencing non-existent specs)
- Orphaned sections (referencing non-existent specs)
With `--fix`: removes orphaned refs and sections.

**`fm where`** — prints both the JSONL directory (`.forma/`) and the DB directory (`~/.local/share/forma/<hash>/`).

### Daemon

```
fm daemon [--port <port>] [--project-dir <path>]
fm daemon status
```

**`fm daemon`** starts the daemon in the foreground on the specified port (default: per-project derived via SHA-256). The `--project-dir` flag tells the daemon where `.forma/` lives (default: current working directory). The daemon creates `.forma/` and the DB file if they don't exist, runs migrations, and starts serving.

**`fm daemon status`** checks if the daemon is running and reachable. Prints the daemon URL and project directory if connected. Exits 0 if reachable, 1 if not.

## Validation

`fm check` runs all validation rules and reports findings. Returns exit code 0 if clean, 1 if any errors found.

### Rules

| Check | Severity | Description |
|-------|----------|-------------|
| Required sections present | error | Every spec has all 5 required sections |
| Required sections non-empty | warn | Required sections have non-empty bodies |
| Src paths exist | error | Every spec's `src` path (if set) exists on disk |
| Ref targets exist | error | All `ref` targets point to existing specs |
| No ref cycles | error | No circular reference chains |
| Pensa spec references valid | warn | All pensa issues with `--spec` values reference existing forma specs |
| No duplicate slugs | error | No duplicate section slugs within a spec |

### Pensa integration

`fm check` calls the pensa daemon (`GET /issues?status=open&status=in_progress`) to retrieve all active issues. For each issue with a non-null `spec` field, it verifies that a forma spec with that stem exists. Mismatches are reported as warnings (not errors) since they don't break forma's own integrity.

If the pensa daemon is unreachable, `fm check` skips pensa validation with a warning.

## Pensa Tight Coupling

When `pn create --spec <stem>` or `pn update <id> --spec <stem>` is called, pensa validates the spec stem against forma before accepting the mutation.

### Validation flow

1. Pensa daemon receives a request with a `spec` field value.
2. Pensa daemon calls forma daemon: `GET /specs/:stem`.
3. If forma returns 200 → proceed with the pensa operation.
4. If forma returns 404 → pensa rejects with error: `spec '<stem>' not found in forma`.
5. If forma daemon is unreachable → pensa rejects with error: `forma daemon not running, cannot validate --spec`.

### Discovery

Pensa discovers the forma daemon using the same port derivation logic (SHA-256 of `"forma:" + project_path`). The project path is already known to the pensa daemon (it receives `--project-dir` on startup). No additional configuration is needed.

### No-spec operations

`--spec` remains optional on all pensa commands. Operations without `--spec` do not contact the forma daemon.

## JSON Output

Following the pensa pattern: no envelope, direct data to stdout.

### Routing

- Success data → stdout
- Errors → stderr
- Both as JSON when `--json` is active

### Exit codes

- `0` — success
- `1` — error

### Error shape (stderr)

```json
{"error": "spec not found: auth", "code": "not_found"}
```

Known error codes: `not_found`, `already_exists`, `cycle_detected`, `required_section`, `validation_failed`.

### Per-command output shapes (stdout)

| Command | Shape |
|---------|-------|
| `create`, `update` | Single spec object |
| `show` | Single spec detail object (spec fields + `sections`, `refs` arrays) |
| `delete` | `{"status": "deleted", "stem": "..."}` |
| `list`, `search` | Array of spec objects |
| `count` | `{"count": N}` or `{"total": N, "groups": [...]}` when grouped |
| `status` | Summary object (counts by status) |
| `history` | Array of event objects |
| `section add`, `section set` | Single section object |
| `section get` | Single section object |
| `section list` | Array of section objects |
| `section remove` | `{"status": "removed", "spec": "...", "slug": "..."}` |
| `section move` | Single section object (with updated position) |
| `ref add` | `{"status": "added", "from": "...", "to": "..."}` |
| `ref remove` | `{"status": "removed", "from": "...", "to": "..."}` |
| `ref list` | Array of spec objects (the referenced specs) |
| `ref tree` | Flat array of tree nodes: `{"stem", "purpose", "status", "depth"}` |
| `ref cycles` | Array of arrays (each inner array is one cycle of stems) |
| `check` | `{"ok": bool, "errors": [...], "warnings": [...]}` |
| `doctor` | Report object (findings array + fixes applied) |
| `export`, `import` | `{"status": "ok", "specs": N, "sections": N, "refs": N}` |

### Spec object fields

`stem`, `src` (nullable), `purpose`, `status`, `created_at`, `updated_at`.

### Spec detail object

Spec object fields plus `sections` (array of section objects, ordered by position) and `refs` (array of spec objects for referenced specs).

### Section object fields

`id`, `name`, `slug`, `kind`, `body`, `position`, `spec_stem`, `created_at`, `updated_at`. The `id`, `spec_stem`, `created_at`, and `updated_at` fields are included when the section is returned as part of a spec detail object or from `section get`/`section add`/`section set`.

### Event object fields

`id`, `spec_stem`, `event_type`, `actor`, `detail`, `created_at`.

## Human-Readable Output

When `--json` is not set, commands produce human-readable output suitable for terminal use.

**`fm show`** renders the spec as clean markdown — the same format as the exported `.forma/specs/*.md` files.

**`fm list`** renders a compact table: `stem  status  purpose`.

**`fm section list`** renders: `position  slug  kind  (body length)`.

**`fm history`** renders: `timestamp  event_type  by actor: detail`.

Other commands use similar compact formats. Specific formatting is left to implementation.

## HTTP API

The daemon exposes a REST API. The CLI translates subcommands into HTTP requests.

### Endpoint mapping

| CLI command | Method | Path |
|-------------|--------|------|
| `create` | POST | `/specs` |
| `show` | GET | `/specs/:stem` |
| `list` | GET | `/specs` |
| `update` | PATCH | `/specs/:stem` |
| `delete` | DELETE | `/specs/:stem?force=true` |
| `search` | GET | `/specs/search?q=...` |
| `count` | GET | `/specs/count` |
| `status` | GET | `/status` |
| `history` | GET | `/specs/:stem/history` |
| `section add` | POST | `/specs/:stem/sections` |
| `section set` | PUT | `/specs/:stem/sections/:slug` |
| `section get` | GET | `/specs/:stem/sections/:slug` |
| `section list` | GET | `/specs/:stem/sections` |
| `section remove` | DELETE | `/specs/:stem/sections/:slug` |
| `section move` | PATCH | `/specs/:stem/sections/:slug/move` |
| `ref add` | POST | `/specs/:stem/refs` |
| `ref remove` | DELETE | `/specs/:stem/refs/:target` |
| `ref list` | GET | `/specs/:stem/refs` |
| `ref tree` | GET | `/specs/:stem/refs/tree?direction=up|down` |
| `ref cycles` | GET | `/refs/cycles` |
| `export` | POST | `/export` |
| `import` | POST | `/import` |
| `check` | GET | `/check` |
| `doctor` | POST | `/doctor?fix=true` |
| *(internal)* | POST | `/shutdown` |
| `where` | — | *(client-only, no daemon request)* |

All endpoints accept and return JSON. Query parameters map to CLI filter flags.

### Actor resolution

Extract from request header `x-forma-actor`. Fallback to `"unknown"`.

### Error handling

`AppError` wraps `FormaError`, maps to HTTP status codes:

| Error | HTTP Status |
|-------|-------------|
| `not_found` | 404 |
| `already_exists` | 409 |
| `cycle_detected` | 409 |
| `required_section` | 400 |
| `validation_failed` | 400 |
| Internal errors | 500 |


## JSONL Format

Each JSONL file contains one JSON object per line. Objects use the same field names as the JSON output.

### `specs.jsonl`

One line per spec. All fields included. Sorted by `stem` for stable diffs.

### `sections.jsonl`

One line per section. All fields included. Sorted by `spec_stem` then `position`.

### `refs.jsonl`

One line per ref: `{"from_stem": "...", "to_stem": "..."}`. Sorted by `from_stem` then `to_stem`.

## Export Markdown Generation

`fm export` generates `.forma/specs/<stem>.md` for each spec using a deterministic template:

```markdown
# <stem> Specification

<purpose>

| Field | Value |
|-------|-------|
| Src | `<src>` |
| Status | <status> |

## <Section 1 Name>

<section 1 body>

## <Section 2 Name>

<section 2 body>

...

## Related Specifications

- [<ref-stem>](<ref-stem>.md) — <ref-purpose>
```

Sections are rendered in position order. The "Related Specifications" section is auto-generated from the refs data — it is not a stored section. If a spec has no refs, the "Related Specifications" section is omitted.

### Stale file cleanup

After writing all spec markdown files, `fm export` removes any `.md` files from `.forma/specs/` whose filename stem does not match an active spec in the database. This ensures deleted specs do not leave orphaned markdown behind.

### README generation

`fm export` also generates `.forma/README.md`:

```markdown
# Specifications

| Spec | Code | Status | Purpose |
|------|------|--------|---------|
| [auth](specs/auth.md) | `crates/auth/` | stable | Authentication and session management |
```

Sorted by stem. Links are relative to `.forma/`.

## Database Initialization

When the daemon starts:

1. Create `.forma/` directory if it doesn't exist. (Note: `sgf init` also creates this directory during scaffolding. Both operations are idempotent.)
2. Create `~/.local/share/forma/<project-hash>/` directory if it doesn't exist.
3. Open (or create) `~/.local/share/forma/<project-hash>/db.sqlite`.
4. Set pragmas: `busy_timeout=5000`, `foreign_keys=ON`.
5. Run migrations — create tables if they don't exist.
6. If JSONL files exist but the database is empty, automatically import from JSONL (handles fresh clone scenario).
