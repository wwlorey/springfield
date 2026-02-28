# Pensa Implementation Plan

Implementation plan for the `pn` CLI and daemon, based on [`specs/pensa.md`](specs/pensa.md).

---

## Phase 0: Toolchain & Environment Setup ✅

Install all tools referenced in [`AGENTS.md`](AGENTS.md) and verify the workspace builds.

- ✅ Install Rust toolchain (stable) and confirm `rustup show` reports a valid toolchain
- ✅ Confirm existing workspace compiles: `cargo build --workspace`
- ✅ Confirm existing tests pass: `cargo test --workspace`
- ✅ Confirm linting passes: `cargo clippy --workspace -- -D warnings`
- ✅ Confirm formatting: `cargo fmt --all --check`
- **Note:** The workspace declares `edition = "2024"` in [`Cargo.toml:7`](Cargo.toml). This requires Rust 1.85+.

---

## Phase 1: Crate Scaffolding ✅

Create the `crates/pensa/` directory structure and `Cargo.toml`.

- ✅ `crates/pensa/Cargo.toml` — `name = "pensa"`, `[[bin]] name = "pn"`, all deps declared
- ✅ `crates/pensa/src/main.rs` — clap skeleton (daemon, where subcommands)
- ✅ `crates/pensa/src/lib.rs` — module declarations
- ✅ `crates/pensa/src/types.rs` — Issue, Comment, Event, Dep, DepTreeNode, enums
- ✅ `crates/pensa/src/id.rs` — UUIDv7-based ID generation with 2 unit tests
- ✅ `crates/pensa/src/error.rs` — PensaError enum, ErrorResponse

### Lessons learned

- **UUIDv7 first 8 hex chars are NOT unique within the same millisecond.** The first 8 hex chars are the top 32 bits of the 48-bit timestamp — two IDs generated in rapid succession share them. Use the last 8 hex chars (random_b portion) instead.

---

## Phase 2: Shared Types & ID Generation ✅ (completed as part of Phase 1)

Completed inline with Phase 1 — see above.

---

## Phase 3: DB — Schema & Open ✅

Create `crates/pensa/src/db.rs` with the `Db` struct, connection management, and schema migrations.

- ✅ `Db` struct wrapping `rusqlite::Connection` + `pensa_dir: PathBuf`
- ✅ `Db::open(path: &Path) -> Result<Db>` — creates `.pensa/` dir, opens `db.sqlite`, sets pragmas, runs migrations
- ✅ Schema matches spec exactly (all four tables: issues, deps, comments, events)
- ✅ Auto-import from JSONL deferred to Phase 8 (TODO comment in place)
- ✅ `now() -> String` helper — ISO 8601 UTC timestamp
- ✅ Wired `pub mod db` in `lib.rs`
- ✅ Tests: `open_creates_tables`, `open_is_idempotent`, `foreign_keys_enforced`
- ✅ Verified: build + test (5 pass) + clippy + fmt

---

## Phase 4: DB — Issue CRUD ✅

Implement `create_issue`, `get_issue`, `update_issue`.

- ✅ `create_issue(&self, params: &CreateIssueParams) -> Result<Issue>` — uses `CreateIssueParams` struct to satisfy clippy's max-args limit
- ✅ `get_issue(&self, id) -> Result<IssueDetail>` — fetches issue + deps (as issue objects via JOIN) + comments
- ✅ `get_issue_only(&self, id) -> Result<Issue>` — internal helper, returns just the issue (no deps/comments)
- ✅ `update_issue(&self, id, &UpdateFields, actor) -> Result<Issue>` — dynamic SET via `rusqlite::types::Value` + `params_from_iter`
- ✅ Helper functions: `issue_from_row`, `comment_from_row`, `parse_dt` for SQLite → Rust type conversion
- ✅ New types: `IssueDetail` (with `#[serde(flatten)]`), `UpdateFields` (with `Default`), `CreateIssueParams`
- ✅ Enum conversion: `as_str()` + `FromStr` impls for `IssueType`, `Status`, `Priority`
- ✅ Tests: `create_and_get`, `get_nonexistent`, `update_fields`, `update_logs_event` (4 new, 9 total)
- ✅ Verified: build + test (9 pass) + clippy + fmt

### Design decisions

- **`CreateIssueParams` struct** instead of 10 positional args — clippy `too_many_arguments` lint enforces max 7.
- **Dynamic UPDATE via `rusqlite::types::Value`** — column names are hardcoded strings (safe from injection), values are parameterized via `params_from_iter(Vec<Value>)`. Empty assignee string maps to `Value::Null` to support clearing.
- **`issue_from_row` / `comment_from_row` as `pub(crate)` free functions** — reusable across future phases without coupling to `Db`.
- **`parse_dt` uses `unwrap()`** — DB only stores timestamps from our `now()` function (guaranteed RFC 3339), so parse failure is an invariant violation.

---

## Phase 5: DB — Issue Lifecycle ✅

Implement `claim_issue`, `release_issue`, `close_issue` (with `fixes` auto-close), `reopen_issue`, `delete_issue`.

- ✅ `claim_issue(id, actor)` — atomic UPDATE with WHERE status='open', returns AlreadyClaimed with holder on conflict
- ✅ `release_issue(id, actor)` — sets status='open', clears assignee, logs "released" event
- ✅ `close_issue(id, reason, force, actor)` — with InvalidStatusTransition guard, auto-closes linked bug via `fixes` field
- ✅ `reopen_issue(id, reason, actor)` — clears closed_at/close_reason, logs "reopened" event
- ✅ `delete_issue(id, force)` — checks dependents + comments, cascading delete with force
- ✅ Tests: `claim_sets_in_progress`, `double_claim_fails`, `release_clears`, `close_reopen_cycle`, `fixes_auto_close`, `delete_requires_force`, `force_delete_cascades` (7 new, 16 total)
- ✅ Verified: build + test (16 pass) + clippy + fmt

---

## Phase 6: DB — Queries ✅

Implement `list_issues`, `ready_issues`, `blocked_issues`, `search_issues`, `count_issues`, `project_status`, `issue_history`.

- ✅ `ListFilters` struct: optional status, priority, assignee, issue_type, spec, sort (`String`), limit (`usize`)
- ✅ New types: `CountResult`, `GroupedCountResult`, `CountGroup`, `StatusEntry`
- ✅ `list_issues(filters)` — dynamic WHERE + ORDER BY + LIMIT, default sort priority ASC then created_at ASC
- ✅ `ready_issues(filters)` — WHERE status='open' AND issue_type IN ('task','test','chore') AND no non-closed deps
- ✅ `blocked_issues()` — DISTINCT join on deps + blocker issues where blocker.status != 'closed'
- ✅ `search_issues(query)` — case-insensitive LIKE on title + description
- ✅ `count_issues(group_by_fields)` — returns `{"count": N}` (no grouping) or `{"total": N, "groups": [...]}` (grouped)
- ✅ `project_status()` — SUM/CASE pivot by issue_type → open/in_progress/closed counts
- ✅ `issue_history(id)` — SELECT from events ORDER BY created_at DESC, id DESC
- ✅ Tests: `list_with_filters`, `ready_excludes_bugs`, `ready_excludes_blocked`, `blocked_returns_blocked`, `search_case_insensitive`, `count_basic`, `history_newest_first` (7 new, 23 total)
- ✅ Verified: build + test (23 pass) + clippy + fmt

---

## Phase 7: DB — Dependencies ✅

Implement `add_dep` (with cycle detection), `remove_dep`, `list_deps`, `dep_tree`, `detect_cycles`.

- ✅ `add_dep(child_id, parent_id, actor)` — validates both issues exist, calls `has_cycle`, INSERTs dep + "dep_added" event
- ✅ `remove_dep(child_id, parent_id, actor)` — DELETE from deps, returns NotFound if no such dep, logs "dep_removed" event
- ✅ `list_deps(id)` — returns issue objects that this issue depends on (via JOIN)
- ✅ `dep_tree(id, direction)` — recursive CTE traversal, direction=down (what this blocks) or up (what blocks this), returns flat `DepTreeNode` array
- ✅ `detect_cycles()` — full DFS graph scan, returns empty in healthy DB
- ✅ Private `has_cycle(child_id, parent_id)` — BFS from `parent_id`, checks if `child_id` is reachable
- ✅ Tests: `add_and_list_deps`, `cycle_detection_rejects`, `dep_tree_down`, `dep_tree_up`, `remove_dep_works`, `detect_cycles_empty` (6 new, 29 total)
- ✅ Verified: build + test (29 pass) + clippy + fmt

---

## Phase 8: DB — Comments, Export/Import, Doctor ✅

Implement comments, JSONL export/import (with auto-import in `Db::open`), and doctor.

- ✅ `add_comment(issue_id, actor, text)` — generates comment ID, INSERTs into comments + "commented" event, returns `Comment`
- ✅ `list_comments(issue_id)` — SELECT from comments WHERE issue_id ORDER BY created_at
- ✅ `export_jsonl()` — writes `issues.jsonl` (sorted by created_at), `deps.jsonl` (sorted by issue_id+depends_on_id), `comments.jsonl` (sorted by created_at); returns `ExportImportResult`
- ✅ `import_jsonl()` — DELETEs all rows from all tables, reads and inserts from JSONL files; returns `ExportImportResult`
- ✅ `Db::open()` auto-import — if issue count is 0 and `issues.jsonl` exists, calls `import_jsonl()`
- ✅ `doctor(fix: bool)` — checks stale claims (in_progress issues), orphaned deps, JSONL/SQLite drift; with fix=true releases all claims and removes orphaned deps
- ✅ New types: `ExportImportResult`, `DoctorFinding`, `DoctorReport`
- ✅ Tests: `add_and_list_comments`, `export_import_roundtrip`, `jsonl_sorted`, `auto_import_on_open`, `doctor_detects_stale`, `doctor_fix_releases` (6 new, 35 total)
- ✅ Verified: build + test (35 pass) + clippy + fmt

### Design decisions

- **`import_jsonl` uses DELETE instead of DROP/recreate** — avoids having to re-run schema migrations; DELETE is sufficient since we're rebuilding all data from JSONL. Foreign key constraints are temporarily sidestepped by deleting in the correct order (events → comments → deps → issues).
- **`export_jsonl` uses `list_issues` with default filters** — reuses existing query logic rather than duplicating SQL. Results are then sorted by `created_at` in Rust for the JSONL output.
- **Doctor stale claim check is simple** — reports all `in_progress` issues as stale. The spec mentions "no recent activity threshold" but doesn't define a specific threshold, so any in_progress issue is reported. This can be refined later if a specific staleness duration is defined.

---

## Phase 9: Daemon — Skeleton & Issue Endpoints ✅

Create the axum server with app state, error mapping, startup/shutdown, and all issue endpoints.

- ✅ `crates/pensa/src/daemon.rs` created with full axum server
- ✅ App state: `Arc<Mutex<Db>>` (all mutation serialized)
- ✅ `pub async fn start(port: u16, project_dir: PathBuf)` — opens DB, builds router, binds to `0.0.0.0:{port}`
- ✅ Graceful shutdown via `tokio::signal` (SIGTERM + SIGINT)
- ✅ `AppError` type wrapping `PensaError` → HTTP status mapping: NotFound→404, AlreadyClaimed/CycleDetected/InvalidStatusTransition/DeleteRequiresForce→409, Internal→500
- ✅ Actor resolution: `X-Pensa-Actor` header or JSON body `actor` field, fallback to `"unknown"`
- ✅ Issue endpoints:
  - `POST /issues` → `create_issue` (body: title, issue_type, priority, description, spec, fixes, assignee, deps)
  - `GET /issues/{id}` → `get_issue` (returns IssueDetail with deps + comments)
  - `PATCH /issues/{id}` → `update_issue` (supports claim/unclaim flags + field updates)
  - `DELETE /issues/{id}?force=bool` → `delete_issue`
  - `POST /issues/{id}/close` → `close_issue` (body: reason, force)
  - `POST /issues/{id}/reopen` → `reopen_issue` (body: reason)
  - `POST /issues/{id}/release` → `release_issue`
- ✅ Wired `pub mod daemon` in `lib.rs`
- ✅ Updated `main.rs` — `pn daemon` now starts the real axum server via `tokio::runtime::Runtime`
- ✅ Verified: build + test (35 pass) + clippy + fmt
- ✅ Smoke tested: all 7 issue endpoints exercised via curl against a live daemon

### Lessons learned

- **Axum 0.8 uses `{id}` path syntax** (not `:id`) — the route parameter syntax changed from earlier versions.

---

## Phase 10: Daemon — Query Endpoints ✅

Add all query/read endpoints.

- ✅ `GET /issues` → `list_issues` (query params: status, priority, assignee, type, spec, sort, limit)
- ✅ `GET /issues/ready` → `ready_issues` (query params: limit, priority, assignee, type, spec)
- ✅ `GET /issues/blocked` → `blocked_issues`
- ✅ `GET /issues/search` → `search_issues` (query param: `q`)
- ✅ `GET /issues/count` → `count_issues` (query params: by_status, by_priority, by_issue_type, by_assignee)
- ✅ `GET /status` → `project_status`
- ✅ `GET /issues/{id}/history` → `issue_history`
- ✅ Routes registered before `{id}` routes to avoid path parameter capture conflicts
- ✅ Consolidated `GET /issues/{id}`, `PATCH /issues/{id}`, `DELETE /issues/{id}` into single `.route()` with method chaining
- ✅ Removed unused `delete` and `patch` routing imports (method chaining handles it)
- ✅ Verified: build + test (35 pass) + clippy + fmt
- ✅ Smoke tested: all 7 query endpoints exercised via curl against a live daemon

### Design decisions

- **Method chaining on routes** — `get(get_issue).patch(update_issue).delete(delete_issue)` on a single `.route("/issues/{id}", ...)` call instead of three separate route registrations. This is more idiomatic for axum 0.8.
- **`#[serde(rename = "type")]` on query params** — The `type` query parameter is a Rust reserved word, so the struct field is `issue_type` with a serde rename.

---

## Phase 11: Daemon — Dep, Comment & Data Endpoints ✅

Add dependency, comment, and data management endpoints.

- ✅ Dependency endpoints:
  - `POST /deps` → `add_dep` (body: issue_id, depends_on_id, actor) — returns `{"status": "added", "issue_id": "...", "depends_on_id": "..."}`
  - `DELETE /deps` → `remove_dep` (query params: issue_id, depends_on_id) — returns `{"status": "removed", ...}`
  - `GET /issues/{id}/deps` → `list_deps` — returns array of issue objects
  - `GET /issues/{id}/deps/tree` → `dep_tree` (query param: direction, default `down`) — returns array of DepTreeNode
  - `GET /deps/cycles` → `detect_cycles` — returns array of arrays
- ✅ Comment endpoints:
  - `POST /issues/{id}/comments` → `add_comment` (body: text, actor) — returns 201 + comment object
  - `GET /issues/{id}/comments` → `list_comments` — returns array of comment objects
- ✅ Data endpoints:
  - `POST /export` → `export_jsonl` — returns ExportImportResult
  - `POST /import` → `import_jsonl` — returns ExportImportResult
  - `POST /doctor` → `doctor` (query param: `fix`) — returns DoctorReport
- ✅ Verified: build + test (35 pass) + clippy + fmt

### Design decisions

- **Method chaining on `/deps` route** — `post(add_dep).delete(remove_dep)` on a single `.route("/deps", ...)` call, consistent with the Phase 10 pattern for `/issues/{id}`.
- **`add_comment` returns 201** — mirrors `create_issue` convention; creating a new resource returns `StatusCode::CREATED`.
- **`remove_dep` uses query params** — the spec maps `DELETE /deps` with query params for `issue_id` and `depends_on_id`, matching the CLI's `pn dep remove <child> <parent>` semantics.

---

## Phase 12: CLI — Client, Clap & Issue Commands ✅

Create the HTTP client, full clap CLI definition, actor resolution, output formatting, and wire all issue commands.

- ✅ `crates/pensa/src/client.rs` — `Client` struct wrapping `reqwest::blocking::Client` + `base_url: String`
  - `Client::new()`: discovers daemon URL from `PN_DAEMON` env var, default `http://localhost:7533`
  - `check_reachable()`: GET `/status` to verify daemon is up
  - `parse_error()`: maps daemon error responses back to `PensaError` variants
  - One method per CLI command (27 methods total), translating args → HTTP request → parse response or error
- ✅ `crates/pensa/src/output.rs` — `OutputMode` enum (`Json`, `Human`) + print functions
  - Print functions for every output shape: issue, issue_detail, issue_list, events, dep_status, dep_tree, cycles, comment, comment_list, count, status, doctor, export_import, deleted, error
  - JSON mode: pretty-printed to stdout, errors as JSON to stderr
  - Human mode: compact, scannable one-liner format
- ✅ `crates/pensa/src/main.rs` — full clap derive CLI with all subcommands
  - Global flags: `--actor <name>` (with `PN_ACTOR` env), `--json`
  - All subcommands wired: create, show, update, close, reopen, release, delete, list, ready, blocked, search, count, status, history, dep (add/remove/list/tree/cycles), comment (add/list), export, import, doctor, daemon (start/status), where
  - Actor resolution: `--actor` flag → `PN_ACTOR` env → `git config user.name` → `$USER`
  - `pn daemon status` checks reachability via client
  - `pn where` prints `.pensa/` path (client-only, no daemon)
  - `pn export` runs `git add .pensa/*.jsonl` after daemon export
- ✅ Wired `pub mod client`, `pub mod output` in `lib.rs`
- ✅ Added `Deserialize` to `ErrorResponse` for client-side error parsing
- ✅ Verified: build + test (35 pass) + clippy + fmt

### Design decisions

- **Client methods return `serde_json::Value`** — the client is a thin HTTP wrapper; it doesn't deserialize into typed structs. This keeps the client simple and avoids duplicating type definitions. The output module formats directly from `Value`.
- **`create_issue` takes `&CreateIssueParams`** — reuses the existing params struct from `types.rs` to satisfy clippy's max-args lint (same pattern as `db.rs`).
- **`parse_error` maps daemon JSON errors back to `PensaError`** — preserves error codes like `not_found`, `already_claimed`, `cycle_detected` so the CLI can format them correctly and exit with code 1.

---

## Phase 13: CLI — Query, Dep, Comment & Data Commands ✅

All commands were already wired in Phase 12 (client methods, output formatters, and main.rs match arms). Phase 13 verified completeness.

- ✅ Query commands: `list`, `ready`, `blocked`, `search`, `count`, `status`, `history`
- ✅ Dep commands: `dep add`, `dep remove`, `dep list`, `dep tree`, `dep cycles`
- ✅ Comment commands: `comment add`, `comment list`
- ✅ Data commands: `export` (with `git add`), `import`, `doctor [--fix]`, `where`
- ✅ Daemon commands: `pn daemon [--port] [--project-dir]`, `pn daemon status`
- ✅ Verified: build + test (35 pass) + clippy + fmt

---

## Phase 14: Documentation ✅

- ✅ Created `crates/pensa/README.md` — project overview, architecture summary (client/daemon, SQLite + JSONL), quick start, command reference (with link to spec), environment variables, storage layout, testing instructions
- ✅ Root `README.md` already accurate — pensa is in the architecture diagram and description matches implementation
- ✅ Updated `.gitignore` — added `.pensa/db.sqlite`
- ✅ Updated `AGENTS.md` — replaced `buddy-<crate>` references with current crate names (`pensa`, `ralph`), added pensa-specific examples
- ✅ Verified: build + test (35 pensa + 19 ralph unit + 13 ralph integration) + clippy + fmt

---

## Phase 15: Integration Tests — Core Scenarios ✅

End-to-end tests that start a real daemon, run `pn` commands against it, and assert on stdout/stderr/exit codes.

- ✅ Created `crates/pensa/tests/integration.rs` with full test harness
- ✅ **Test harness:**
  - `start_daemon()` → spawns daemon on random port (via `portpicker`) with `tempfile::TempDir`, polls `/status` until ready
  - `DaemonGuard` with `Drop` impl kills daemon child process on teardown
  - `pn(guard)` → builds `Command` with `PN_DAEMON=http://localhost:{port}` and `PN_ACTOR=test-agent`
  - Helper functions: `run_json`, `run_ok_json`, `run_fail`, `extract_id`
- ✅ **Test: CRUD lifecycle** — create → show → update → close → reopen → close
- ✅ **Test: Claim semantics** — create → claim (agent-1) → double-claim fails (agent-2) → release → claim (agent-2)
- ✅ **Test: `fixes` auto-close** — create bug → create task with `--fixes` → close task → verify bug auto-closed
- ✅ **Test: Delete with force** — create with comment → delete fails without force → force delete succeeds → show returns not_found
- ✅ **Test: Daemon status (reachable)** — `pn daemon status` returns exit 0 when daemon is running
- ✅ **Test: Daemon status (unreachable)** — `pn daemon status` returns exit 1 when daemon is not running
- ✅ **Test: `pn where`** — prints `.pensa/` path, exits 0, works without running daemon
- ✅ Added dev-dependencies: `portpicker`, `reqwest` (blocking), `serde_json`
- ✅ Verified: build + test (35 unit + 7 integration + 19 ralph unit + 13 ralph integration) + clippy + fmt

### Design decisions

- **`portpicker` for random port** — avoids port conflicts when tests run in parallel. More reliable than manual port range scanning.
- **`DaemonGuard` with `Drop`** — RAII pattern ensures daemon is killed even on test failure/panic, preventing orphaned processes.
- **`reqwest::blocking` for readiness polling** — simpler than spawning `pn daemon status` in a loop; directly polls the HTTP endpoint.
- **`run_json` / `run_ok_json` / `run_fail` helpers** — DRY pattern for the common test flow: run command, parse JSON, assert success/failure.

---

## Phase 16: Integration Tests — Advanced Scenarios ✅

- ✅ **Test: `deps_and_ready`** — create A, B → dep B→A → verify ready excludes B, blocked includes B → close A → B becomes ready → dep list B includes A
- ✅ **Test: `cycle_detection`** — create A, B, C chain → adding cycle dep fails with `cycle_detected` → `dep cycles` returns `[]`
- ✅ **Test: `ready_excludes_bugs`** — create bug + task → ready excludes bug, includes task
- ✅ **Test: `export_import_roundtrip`** — create issues with deps + comments → export → import → verify all data intact (issue count, deps, comments)
- ✅ **Test: `doctor_detects_and_fixes`** — create + claim tasks → doctor reports stale claims → doctor --fix releases them → verify all open
- ✅ **Test: `concurrent_claims`** — spawn two simultaneous claim processes → exactly one succeeds, one fails
- ✅ **Test: `search_issues`** — create issues with distinct titles → search by substring, case-insensitive, partial match
- ✅ **Test: `comments_add_and_list`** — add multiple comments → list returns them in order with correct fields
- ✅ **Test: `history_events`** — create/update/close → history returns events newest-first, at least 3 events
- ✅ **Test: `json_output_shapes`** — comprehensive shape validation for all commands: create, show (deps+comments arrays), list, ready, blocked, search (arrays), count (with/without grouping), status (array), history, comment add/list, dep add/remove/list/tree/cycles, export, import, doctor, update, close, reopen, release
- ✅ **Test: `human_readable_output`** — run list without --json → output contains title text, is NOT valid JSON
- ✅ **Test: `dep_tree_structure`** — A→B→C chain → tree down from A includes B+C with depth fields → tree up from C includes B+A
- ✅ Verified: build + test (35 unit + 19 integration + 19 ralph unit + 13 ralph integration) + clippy + fmt

### Design decisions

- **`project_status` returns an array, not an object** — the daemon's `/status` endpoint returns `Vec<StatusEntry>` (one entry per issue_type), not a summary object. The `json_output_shapes` test was corrected to assert `is_array()`.
- **`dep_tree_structure` as separate test** — covers tree traversal in both directions (up/down) and validates the `depth` field on tree nodes, which isn't covered by `json_output_shapes`.
- **`concurrent_claims` uses `spawn` + `wait_with_output`** — spawns both processes before waiting for either, maximizing the chance of true concurrency hitting the daemon's mutex serialization.
