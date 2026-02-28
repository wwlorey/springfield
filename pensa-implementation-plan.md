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

## Phase 10: Daemon — Query Endpoints

Add all query/read endpoints.

- **Source:** [`specs/pensa.md:334-340`](specs/pensa.md)
- Query endpoints:
  - `GET /issues` → `list_issues` (query params: status, priority, assignee, type, spec, sort, limit) ([`specs/pensa.md:334`](specs/pensa.md))
  - `GET /issues/ready` → `ready_issues` (query params: limit, priority, assignee, type, spec) ([`specs/pensa.md:335`](specs/pensa.md))
  - `GET /issues/blocked` → `blocked_issues` ([`specs/pensa.md:336`](specs/pensa.md))
  - `GET /issues/search` → `search_issues` (query param: `q`) ([`specs/pensa.md:337`](specs/pensa.md))
  - `GET /issues/count` → `count_issues` (query params: by_status, by_priority, by_issue_type, by_assignee) ([`specs/pensa.md:338`](specs/pensa.md))
  - `GET /status` → `project_status` ([`specs/pensa.md:339`](specs/pensa.md))
  - `GET /issues/:id/history` → `issue_history` ([`specs/pensa.md:340`](specs/pensa.md))
- **Important routing note:** `/issues/ready`, `/issues/blocked`, `/issues/search`, `/issues/count` must be registered before `/issues/:id` to avoid the path parameter capturing literal segment names.
- **Verify:** build + clippy + fmt

---

## Phase 11: Daemon — Dep, Comment & Data Endpoints

Add dependency, comment, and data management endpoints.

- **Source:** [`specs/pensa.md:341-350`](specs/pensa.md)
- Dependency endpoints:
  - `POST /deps` → `add_dep` (body: issue_id, depends_on_id) ([`specs/pensa.md:341`](specs/pensa.md))
  - `DELETE /deps` → `remove_dep` (query params: issue_id, depends_on_id) ([`specs/pensa.md:342`](specs/pensa.md))
  - `GET /issues/:id/deps` → `list_deps` ([`specs/pensa.md:343`](specs/pensa.md))
  - `GET /issues/:id/deps/tree` → `dep_tree` (query param: direction — `up` or `down`, default `down`) ([`specs/pensa.md:344`](specs/pensa.md))
  - `GET /deps/cycles` → `detect_cycles` ([`specs/pensa.md:345`](specs/pensa.md))
- Comment endpoints:
  - `POST /issues/:id/comments` → `add_comment` (body: text; actor from header) ([`specs/pensa.md:346`](specs/pensa.md))
  - `GET /issues/:id/comments` → `list_comments` ([`specs/pensa.md:347`](specs/pensa.md))
- Data endpoints:
  - `POST /export` → `export_jsonl` ([`specs/pensa.md:348`](specs/pensa.md))
  - `POST /import` → `import_jsonl` ([`specs/pensa.md:349`](specs/pensa.md))
  - `POST /doctor` → `doctor` (query param: `fix`) ([`specs/pensa.md:350`](specs/pensa.md))
- **Verify:** build + clippy + fmt

---

## Phase 12: CLI — Client, Clap & Issue Commands

Create the HTTP client, full clap CLI definition, actor resolution, output formatting, and wire all issue commands.

- **Source:** [`specs/pensa.md:44-48`](specs/pensa.md) — CLI client; [`specs/pensa.md:145-257`](specs/pensa.md) — CLI Commands; [`specs/pensa.md:262-315`](specs/pensa.md) — Output
- Create `crates/pensa/src/client.rs`:
  - `Client` struct wrapping `reqwest::blocking::Client` + `daemon_url: String`
  - `Client::new()`: discover daemon URL from `PN_DAEMON` env var, default `http://localhost:7533` ([`specs/pensa.md:46`](specs/pensa.md))
  - If daemon is unreachable, fail with clear error + exit 1 ([`specs/pensa.md:47`](specs/pensa.md))
  - One method per CLI command, translating args → HTTP request → parse response or error
- Create `crates/pensa/src/output.rs`:
  - `OutputMode` enum: `Json`, `Human`
  - Print functions for each output shape: single issue, issue list, error, dep status, comment, event list, count, status report, doctor report, export/import result
  - Human-readable: compact, scannable, `git log --oneline` density ([`specs/pensa.md:313-315`](specs/pensa.md))
  - JSON: direct data to stdout, errors to stderr ([`specs/pensa.md:262-270`](specs/pensa.md))
  - Null arrays always `[]`, never `null` ([`specs/pensa.md:285-286`](specs/pensa.md))
  - Exit codes: 0 success, 1 error ([`specs/pensa.md:273-275`](specs/pensa.md))
- Update `crates/pensa/src/main.rs`:
  - Full clap derive CLI with all subcommands ([`specs/pensa.md:145-257`](specs/pensa.md)):
    - Global flags: `--actor <name>`, `--json` ([`specs/pensa.md:147`](specs/pensa.md), [`specs/pensa.md:151`](specs/pensa.md))
    - Issue subcommands: `create`, `update`, `close`, `reopen`, `release`, `delete`, `show`
    - Query subcommands: `list`, `ready`, `blocked`, `search`, `count`, `status`, `history`
    - Dep subcommands: `dep add`, `dep remove`, `dep list`, `dep tree`, `dep cycles`
    - Comment subcommands: `comment add`, `comment list`
    - Data subcommands: `export`, `import`, `doctor`
    - Other: `daemon` (start / status), `where`
  - Actor resolution order: `--actor` flag → `PN_ACTOR` env var → `git config user.name` → `$USER` ([`specs/pensa.md:151`](specs/pensa.md))
  - `pn daemon` starts the daemon directly (not via HTTP) ([`specs/pensa.md:227-233`](specs/pensa.md))
  - `pn daemon status` checks reachability, prints info, exits 0/1 ([`specs/pensa.md:233`](specs/pensa.md))
  - `pn where` prints `.pensa/` path, no daemon request ([`specs/pensa.md:257`](specs/pensa.md), [`specs/pensa.md:351`](specs/pensa.md))
- Wire issue commands through client: `create`, `show`, `update`, `delete`, `close`, `reopen`, `release`
- Wire modules: add `pub mod client`, `pub mod output` to `lib.rs`
- **Verify:** build + clippy + fmt

---

## Phase 13: CLI — Query, Dep, Comment & Data Commands

Wire all remaining commands through the client.

- Query commands: `list`, `ready`, `blocked`, `search`, `count`, `status`, `history`
- Dep commands: `dep add`, `dep remove`, `dep list`, `dep tree`, `dep cycles`
- Comment commands: `comment add`, `comment list`
- Data commands:
  - `export`: POST to daemon, then run `git add .pensa/*.jsonl` locally ([`specs/pensa.md:246`](specs/pensa.md))
  - `import`: POST to daemon ([`specs/pensa.md:248`](specs/pensa.md))
  - `doctor [--fix]`: POST to daemon (with fix query param) ([`specs/pensa.md:250-255`](specs/pensa.md))
  - `where`: print `.pensa/` path, no daemon request (client-only) ([`specs/pensa.md:257`](specs/pensa.md))
- Daemon commands (wired in Phase 12 but finalized here if needed):
  - `pn daemon [--port <port>] [--project-dir <path>]` — start daemon in foreground
  - `pn daemon status` — check reachability, exit 0/1
- **Verify:** `cargo build -p pensa && cargo clippy -p pensa -- -D warnings && cargo fmt --all --check && cargo build --workspace`

---

## Phase 14: Documentation

- Create `crates/pensa/README.md`:
  - Project overview — what pensa is and why it exists
  - Architecture summary (client/daemon, SQLite + JSONL)
  - Quick start: starting the daemon, basic CLI usage
  - Full command reference (or link to [`specs/pensa.md`](specs/pensa.md))
  - Environment variables: `PN_DAEMON`, `PN_ACTOR`
  - Storage layout: `.pensa/db.sqlite`, `.pensa/*.jsonl`
  - Testing instructions: how to run the integration tests
- Update root [`README.md`](README.md):
  - Add pensa to the architecture diagram (`crates/pensa/` line)
  - Ensure the pensa description is accurate
- Update [`.gitignore`](.gitignore):
  - Add `.pensa/db.sqlite` (the working database is gitignored — [`specs/pensa.md:15`](specs/pensa.md))
- Update [`AGENTS.md`](AGENTS.md):
  - Add pensa-specific build/test commands: `cargo build -p pensa`, `cargo test -p pensa`
  - Replace outdated `buddy-<crate>` references with current crate names

---

## Phase 15: Integration Tests — Core Scenarios

End-to-end tests that start a real daemon, run `pn` commands against it, and assert on stdout/stderr/exit codes. Following the pattern in [`crates/ralph/tests/integration.rs`](crates/ralph/tests/integration.rs).

- **Source:** [`specs/pensa.md:387-402`](specs/pensa.md) — Testing Strategy
- **Tools required:** No additional tools beyond `cargo test`. Tests use the built `pn` binary via `env!("CARGO_BIN_EXE_pn")` and `std::process::Command` (same pattern as ralph).
- Create `crates/pensa/tests/integration.rs`:
  - **Test harness:**
    - `start_daemon()` → spawn `pn daemon --port <random> --project-dir <tempdir>`, wait for readiness via `pn daemon status`, return port + child handle
    - `pn_cmd(port)` → build `Command` with `PN_DAEMON=http://localhost:{port}` and `PN_ACTOR=test-agent`
    - Teardown: kill daemon child process after each test
  - **Test: CRUD lifecycle** ([`specs/pensa.md:393`](specs/pensa.md)):
    - `pn create "login crash" -t bug -p p0 --json` → assert exit 0, stdout contains issue with status=open, priority=p0, issue_type=bug
    - `pn show <id> --json` → assert fields match
    - `pn update <id> --priority p1 --json` → assert priority changed
    - `pn close <id> --reason "fixed" --json` → assert status=closed
    - `pn reopen <id> --json` → assert status=open
    - `pn close <id> --json` → assert status=closed again
  - **Test: Claim semantics** ([`specs/pensa.md:394`](specs/pensa.md)):
    - Create a task → `pn update <id> --claim` with actor=agent-1 → assert success, status=in_progress, assignee=agent-1
    - `pn update <id> --claim` with actor=agent-2 → assert exit 1, stderr contains `already_claimed`
    - `pn release <id>` → assert status=open, assignee cleared
    - `pn update <id> --claim` with actor=agent-2 → assert success
  - **Test: `fixes` auto-close** ([`specs/pensa.md:397`](specs/pensa.md)):
    - Create bug → create task with `--fixes <bug-id>` → close the task → `pn show <bug-id> --json` → assert bug is also closed, close_reason contains "fixed by"
  - **Test: Delete with force:**
    - Create issue with comments → `pn delete <id>` (no force) → assert exit 1
    - `pn delete <id> --force` → assert exit 0
    - `pn show <id>` → assert exit 1, not_found
  - **Test: Daemon status:**
    - `pn daemon status` when daemon is running → assert exit 0
    - `pn daemon status` when daemon is not running → assert exit 1
  - **Test: `pn where`:**
    - Assert it prints the `.pensa/` directory path and exits 0
    - Assert it works without a running daemon (client-only command)
- **Verify:** `cargo test -p pensa -- --test integration`

---

## Phase 16: Integration Tests — Advanced Scenarios

- **Test: Dependencies and ready** ([`specs/pensa.md:395`](specs/pensa.md)):
  - Create task-A and task-B → `pn dep add B A` (B depends on A)
  - `pn ready --json` → assert B is absent (blocked), A is present
  - `pn blocked --json` → assert B is listed
  - `pn close A` → `pn ready --json` → assert B now appears
  - `pn dep list B --json` → verify dep structure
- **Test: Cycle detection** ([`specs/pensa.md:396`](specs/pensa.md)):
  - Create A, B, C → `pn dep add B A` → `pn dep add C B` → `pn dep add A C` → assert exit 1, stderr contains `cycle_detected`
  - `pn dep cycles --json` → assert `[]` (the cycle was rejected)
- **Test: `ready` excludes bugs** ([`specs/pensa.md:398`](specs/pensa.md)):
  - Create a bug (open) → `pn ready --json` → assert the bug is not in the result
  - Create a task (open) → `pn ready --json` → assert the task is in the result
- **Test: Export/import round-trip** ([`specs/pensa.md:399`](specs/pensa.md)):
  - Create several issues with deps and comments → `pn export` → verify `.pensa/*.jsonl` files exist
  - Delete `db.sqlite` → `pn import` → `pn list --json` → assert all data intact, matches pre-export state
  - Verify JSONL files are sorted (issues by created_at, deps by issue_id then depends_on_id, comments by created_at — [`specs/pensa.md:363-371`](specs/pensa.md))
- **Test: Doctor** ([`specs/pensa.md:400`](specs/pensa.md)):
  - Create issues → claim them (set in_progress) → `pn doctor --json` → assert stale claims reported
  - `pn doctor --fix --json` → assert fixes applied
  - `pn list --status open --json` → assert all previously claimed issues are now open
- **Test: Concurrent claims** ([`specs/pensa.md:401`](specs/pensa.md)):
  - Create a task → spawn two `pn update <id> --claim` processes simultaneously → assert exactly one succeeds, one fails with `already_claimed`
- **Test: Search:**
  - Create issues with distinct titles → `pn search "login" --json` → assert only matching issues returned
  - Verify case-insensitive: search for "LOGIN" matches "login crash"
- **Test: Comments:**
  - Create issue → `pn comment add <id> "observation" --json` → assert comment returned
  - `pn comment list <id> --json` → assert array with the comment
- **Test: History:**
  - Create issue → update it → close it → `pn history <id> --json` → assert events in newest-first order, covering create/update/close
- **Test: JSON output shapes** ([`specs/pensa.md:402`](specs/pensa.md)):
  - For each command, run with `--json` and validate output matches documented shapes ([`specs/pensa.md:290-305`](specs/pensa.md)):
    - `create` returns single issue object
    - `list` returns array
    - `ready` returns array (and `[]` when nothing matches)
    - `count` returns `{"count": N}` or `{"total": N, "groups": [...]}`
    - `dep tree` returns flat array of `DepTreeNode` objects
    - etc.
- **Test: Human-readable output:**
  - Run `pn list` (without `--json`) → assert stdout contains formatted text, not JSON
- **Verify:** `cargo test -p pensa`
