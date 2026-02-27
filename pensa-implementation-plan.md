# Pensa Implementation Plan

Implementation plan for the `pn` CLI and daemon, based on [`specs/pensa.md`](specs/pensa.md).

---

## Phase 0: Toolchain & Environment Setup ✅

Install all tools referenced in [`AGENTS.md`](AGENTS.md) and verify the workspace builds.

- ✅ Install Rust toolchain (stable) and confirm `rustup show` reports a valid toolchain
- Install `cargo-geiger` for unsafe code detection: `cargo install cargo-geiger`
  - Referenced in `AGENTS.md`: "Detect unsafe code usage: `cargo geiger`"
- ✅ Confirm existing workspace compiles: `cargo build --workspace`
- ✅ Confirm existing tests pass: `cargo test --workspace`
- ✅ Confirm linting passes: `cargo clippy --workspace -- -D warnings`
- ✅ Confirm formatting: `cargo fmt --all --check`
- **Note:** The workspace declares `edition = "2024"` in [`Cargo.toml:7`](Cargo.toml). This requires Rust 1.85+. Verify the installed toolchain supports it; if not, either update the toolchain or change to `edition = "2021"`.

---

## Phase 1: Crate Scaffolding ✅

Create the `crates/pensa/` directory structure and `Cargo.toml`.

- ✅ Create `crates/pensa/Cargo.toml` following the pattern in [`crates/ralph/Cargo.toml`](crates/ralph/Cargo.toml):
  - `name = "pensa"`, inherit `version`, `edition`, `license` from workspace
  - Define `[[bin]] name = "pn"` pointing to `src/main.rs`
  - Dependencies (per [`specs/pensa.md:49-53`](specs/pensa.md) — Technology choices):
    - `axum` — daemon HTTP server
    - `tokio` with `full` features — async runtime for axum
    - `reqwest` with `blocking` feature — CLI HTTP client
    - `rusqlite` with `bundled` feature — SQLite with bundled driver
    - `clap` with `derive` and `env` features — CLI argument parsing
    - `serde` with `derive` feature — serialization
    - `serde_json` — JSON serialization
    - `uuid` with `v7` feature — UUIDv7 for ID generation ([`specs/pensa.md:81`](specs/pensa.md))
    - `tracing` and `tracing-subscriber` — structured logging per `AGENTS.md` code style
    - `chrono` with `serde` feature — ISO 8601 timestamp handling ([`specs/pensa.md:139-141`](specs/pensa.md))
  - Dev-dependencies:
    - `tempfile` — test isolation (following [`crates/ralph/Cargo.toml:19`](crates/ralph/Cargo.toml))
    - `assert_cmd` — CLI integration testing
    - `predicates` — assertion helpers for CLI output
    - `tokio-test` or `tokio` (with `test-util`) — async test support
- ✅ Create source files (not just stubs — includes Phase 2 types, id gen, error types):
  - `crates/pensa/src/main.rs` — binary entry point with clap CLI skeleton (daemon, where subcommands)
  - `crates/pensa/src/lib.rs` — module declarations
  - `crates/pensa/src/types.rs` — Issue, Comment, Event, Dep, DepTreeNode, enums
  - `crates/pensa/src/id.rs` — UUIDv7-based ID generation with tests
  - `crates/pensa/src/error.rs` — PensaError enum, ErrorResponse
- ✅ Verify the new crate compiles: `cargo build -p pensa`
- ✅ Verify the workspace still compiles: `cargo build --workspace`
- ✅ All tests pass: `cargo test --workspace` (34 tests)
- ✅ Clippy clean: `cargo clippy --workspace -- -D warnings`
- ✅ Formatting clean: `cargo fmt --all --check`

### Lessons learned

- **UUIDv7 first 8 hex chars are NOT unique within the same millisecond.** The first 8 hex chars are the top 32 bits of the 48-bit timestamp — two IDs generated in rapid succession share them. Use the last 8 hex chars (random_b portion) instead.

---

## Phase 2: Shared Types & ID Generation ✅ (completed as part of Phase 1)

Define the core domain types that both daemon and CLI share.

- **Source:** [`specs/pensa.md:59-141`](specs/pensa.md) — Schema section
- Create `crates/pensa/src/types.rs`:
  - `IssueType` enum: `Bug`, `Task`, `Test`, `Chore` — with serde rename to lowercase ([`specs/pensa.md:68`](specs/pensa.md))
  - `Status` enum: `Open`, `InProgress`, `Closed` — matching CHECK constraint ([`specs/pensa.md:69`](specs/pensa.md))
  - `Priority` enum: `P0`, `P1`, `P2`, `P3` — with `Ord` impl where P0 < P1 < P2 < P3 ([`specs/pensa.md:93`](specs/pensa.md))
  - `Issue` struct — all fields from the issues table, with `Option<T>` for nullable fields ([`specs/pensa.md:64-78`](specs/pensa.md), [`specs/pensa.md:309`](specs/pensa.md))
  - `Comment` struct — id, issue_id, actor, text, created_at ([`specs/pensa.md:113-119`](specs/pensa.md))
  - `Event` struct — id, issue_id, event_type, actor, detail, created_at ([`specs/pensa.md:127-134`](specs/pensa.md))
  - `Dep` struct — issue_id, depends_on_id ([`specs/pensa.md:100-106`](specs/pensa.md))
  - `DepTreeNode` struct — id, title, status, priority, issue_type, depth ([`specs/pensa.md:300`](specs/pensa.md))
  - Serde: derive `Serialize`, `Deserialize` on all types; use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields ([`specs/pensa.md:309`](specs/pensa.md) — "Absent optional fields are omitted")
- Create `crates/pensa/src/id.rs`:
  - `generate_id() -> String` — produce `pn-` + 8 hex chars from UUIDv7 ([`specs/pensa.md:81`](specs/pensa.md))
- Create `crates/pensa/src/error.rs`:
  - `PensaError` enum with variants for each error code: `NotFound`, `AlreadyClaimed`, `CycleDetected`, `InvalidStatusTransition` ([`specs/pensa.md:278-282`](specs/pensa.md))
  - `ErrorResponse` struct: `{ error: String, code: Option<String> }` ([`specs/pensa.md:279`](specs/pensa.md))

---

## Phase 3: Database Layer

Implement SQLite schema, migrations, and query functions that the daemon will call.

- **Source:** [`specs/pensa.md:62-141`](specs/pensa.md) — Schema; [`specs/pensa.md:375-384`](specs/pensa.md) — Database Initialization
- Create `crates/pensa/src/db.rs`:
  - `Db` struct wrapping `rusqlite::Connection`
  - `Db::open(path: &Path) -> Result<Db>`:
    - Create `.pensa/` directory if needed ([`specs/pensa.md:379`](specs/pensa.md))
    - Open or create `db.sqlite` ([`specs/pensa.md:380`](specs/pensa.md))
    - Set pragmas: `busy_timeout=5000`, `foreign_keys=ON` ([`specs/pensa.md:37`](specs/pensa.md), [`specs/pensa.md:381`](specs/pensa.md))
    - Run migrations — CREATE TABLE IF NOT EXISTS for all four tables ([`specs/pensa.md:382`](specs/pensa.md))
    - Auto-import from JSONL if DB is empty but JSONL files exist ([`specs/pensa.md:383`](specs/pensa.md))
  - Issue CRUD:
    - `create_issue(...)` — INSERT into issues + INSERT event ([`specs/pensa.md:155-156`](specs/pensa.md))
    - `get_issue(id)` — SELECT with deps and comments for `show` ([`specs/pensa.md:162`](specs/pensa.md))
    - `update_issue(id, ...)` — partial UPDATE + event logging ([`specs/pensa.md:157`](specs/pensa.md))
    - `claim_issue(id, actor)` — atomic UPDATE with `WHERE status = 'open'`, return `AlreadyClaimed` error if fails ([`specs/pensa.md:165-166`](specs/pensa.md))
    - `close_issue(id, reason, force)` — status transition + auto-close linked bug via `fixes` field ([`specs/pensa.md:158`](specs/pensa.md), [`specs/pensa.md:171`](specs/pensa.md))
    - `reopen_issue(id, reason)` ([`specs/pensa.md:159`](specs/pensa.md))
    - `release_issue(id)` — set status=open, clear assignee ([`specs/pensa.md:169`](specs/pensa.md))
    - `delete_issue(id, force)` — require force if dependents or comments exist ([`specs/pensa.md:173`](specs/pensa.md))
  - Query functions:
    - `list_issues(filters)` — with optional status/priority/assignee/type/spec filters, sort, limit ([`specs/pensa.md:178`](specs/pensa.md), [`specs/pensa.md:189`](specs/pensa.md))
    - `ready_issues(filters)` — open, unblocked, type in (task/test/chore), sorted priority→created_at ([`specs/pensa.md:179`](specs/pensa.md), [`specs/pensa.md:187`](specs/pensa.md))
    - `blocked_issues()` — issues with at least one open dependency ([`specs/pensa.md:192`](specs/pensa.md))
    - `search_issues(query)` — case-insensitive LIKE on title+description ([`specs/pensa.md:193`](specs/pensa.md))
    - `count_issues(group_by)` — with optional grouping ([`specs/pensa.md:195-196`](specs/pensa.md))
    - `project_status()` — open/in_progress/closed by type ([`specs/pensa.md:197`](specs/pensa.md))
    - `issue_history(id)` — events for issue, newest first ([`specs/pensa.md:199`](specs/pensa.md))
  - Dependency functions:
    - `add_dep(child, parent)` — with cycle detection before insert ([`specs/pensa.md:211`](specs/pensa.md))
    - `remove_dep(child, parent)` ([`specs/pensa.md:205`](specs/pensa.md))
    - `list_deps(id)` ([`specs/pensa.md:206`](specs/pensa.md))
    - `dep_tree(id, direction)` — recursive CTE, return flat `DepTreeNode` array ([`specs/pensa.md:207`](specs/pensa.md), [`specs/pensa.md:300`](specs/pensa.md))
    - `detect_cycles()` — scan and report ([`specs/pensa.md:215`](specs/pensa.md))
    - Private `has_cycle(child, parent)` — BFS/DFS reachability check used by `add_dep`
  - Comment functions:
    - `add_comment(issue_id, actor, text)` ([`specs/pensa.md:220`](specs/pensa.md))
    - `list_comments(issue_id)` ([`specs/pensa.md:221`](specs/pensa.md))
  - Export/import:
    - `export_jsonl(pensa_dir)` — dump issues, deps, comments to separate `.jsonl` files, sorted for stable diffs ([`specs/pensa.md:246`](specs/pensa.md), [`specs/pensa.md:357-371`](specs/pensa.md))
    - `import_jsonl(pensa_dir)` — drop and recreate tables, insert from JSONL ([`specs/pensa.md:248`](specs/pensa.md))
  - Doctor:
    - `doctor(fix: bool)` — detect stale claims, orphaned deps, JSONL/SQLite drift; optionally fix ([`specs/pensa.md:250-255`](specs/pensa.md))

---

## Phase 4: Daemon (HTTP Server)

Implement the axum-based daemon that owns the database.

- **Source:** [`specs/pensa.md:33-53`](specs/pensa.md) — Runtime Architecture; [`specs/pensa.md:319-354`](specs/pensa.md) — HTTP API
- Create `crates/pensa/src/daemon.rs`:
  - App state: `Arc<Mutex<Db>>` (all mutation serialized through the daemon — [`specs/pensa.md:38`](specs/pensa.md))
  - `start(port: u16, project_dir: PathBuf)`:
    - Open DB via `Db::open(project_dir.join(".pensa"))` ([`specs/pensa.md:41`](specs/pensa.md))
    - Build axum Router with all routes
    - Bind to `0.0.0.0:{port}` (default 7533 — [`specs/pensa.md:35`](specs/pensa.md))
    - Install SIGTERM handler for graceful shutdown ([`specs/pensa.md:40`](specs/pensa.md))
    - Run in foreground ([`specs/pensa.md:39`](specs/pensa.md))
  - Route handlers mapping CLI commands to HTTP endpoints ([`specs/pensa.md:325-351`](specs/pensa.md)):
    - `POST /issues` → create
    - `PATCH /issues/:id` → update (including claim/unclaim)
    - `POST /issues/:id/close` → close
    - `POST /issues/:id/reopen` → reopen
    - `POST /issues/:id/release` → release
    - `DELETE /issues/:id` → delete (query param `force`)
    - `GET /issues/:id` → show
    - `GET /issues` → list (query params for filters)
    - `GET /issues/ready` → ready (query params for filters)
    - `GET /issues/blocked` → blocked
    - `GET /issues/search` → search (query param `q`)
    - `GET /issues/count` → count (query params for grouping)
    - `GET /status` → status
    - `GET /issues/:id/history` → history
    - `POST /deps` → dep add
    - `DELETE /deps` → dep remove (query params)
    - `GET /issues/:id/deps` → dep list
    - `GET /issues/:id/deps/tree` → dep tree (query param `direction`)
    - `GET /deps/cycles` → dep cycles
    - `POST /issues/:id/comments` → comment add
    - `GET /issues/:id/comments` → comment list
    - `POST /export` → export
    - `POST /import` → import
    - `POST /doctor` → doctor (query param `fix`)
  - Error responses: map `PensaError` to appropriate HTTP status codes + JSON error body ([`specs/pensa.md:278-282`](specs/pensa.md))
  - All endpoints accept and return JSON ([`specs/pensa.md:353`](specs/pensa.md))
  - Actor passed via `X-Pensa-Actor` header or JSON body field

---

## Phase 5: CLI Client

Implement the `pn` binary that sends HTTP requests to the daemon.

- **Source:** [`specs/pensa.md:44-48`](specs/pensa.md) — CLI client; [`specs/pensa.md:145-257`](specs/pensa.md) — CLI Commands
- Create `crates/pensa/src/client.rs`:
  - `Client` struct wrapping `reqwest::blocking::Client` + daemon URL
  - Daemon URL discovery: `PN_DAEMON` env var → default `http://localhost:7533` ([`specs/pensa.md:46`](specs/pensa.md))
  - If daemon is unreachable, fail with clear error and non-zero exit ([`specs/pensa.md:47`](specs/pensa.md))
  - One method per CLI command, translating args into HTTP requests per the endpoint mapping
- Implement `crates/pensa/src/main.rs`:
  - Use clap derive API to define the CLI ([`specs/pensa.md:145-257`](specs/pensa.md)):
    - Global flag: `--actor <name>` ([`specs/pensa.md:151`](specs/pensa.md))
    - Global flag: `--json` for structured output ([`specs/pensa.md:147`](specs/pensa.md))
    - Subcommands: `create`, `update`, `close`, `reopen`, `release`, `delete`, `show`, `list`, `ready`, `blocked`, `search`, `count`, `status`, `history`, `dep`, `comment`, `daemon`, `export`, `import`, `doctor`, `where`
  - Actor resolution order: `--actor` flag → `PN_ACTOR` env var → `git config user.name` → `$USER` ([`specs/pensa.md:151`](specs/pensa.md))
  - `pn daemon` subcommand starts the daemon directly (not via HTTP client) ([`specs/pensa.md:227-233`](specs/pensa.md))
  - `pn daemon status` checks reachability, prints info, exits 0/1 ([`specs/pensa.md:233`](specs/pensa.md))
  - `pn where` is client-only — prints `.pensa/` path, no daemon request ([`specs/pensa.md:257`](specs/pensa.md), [`specs/pensa.md:351`](specs/pensa.md))
  - `pn export` sends POST to daemon, then runs `git add .pensa/*.jsonl` locally ([`specs/pensa.md:246`](specs/pensa.md))
- Create `crates/pensa/src/output.rs`:
  - Human-readable formatting when `--json` is not set ([`specs/pensa.md:313-315`](specs/pensa.md)):
    - Compact, scannable output similar to `git log --oneline` density
  - JSON output: direct data to stdout, errors to stderr ([`specs/pensa.md:262-270`](specs/pensa.md))
  - Exit codes: 0 success, 1 error ([`specs/pensa.md:273-275`](specs/pensa.md))
  - Null arrays always `[]`, never `null` ([`specs/pensa.md:285-286`](specs/pensa.md))

---

## Phase 6: Module Wiring & Build Verification

Connect all modules and verify the crate compiles and passes lint.

- Wire modules in `crates/pensa/src/lib.rs`: `pub mod types`, `pub mod id`, `pub mod error`, `pub mod db`, `pub mod daemon`, `pub mod client`, `pub mod output`
- Wire the `main.rs` entry point: clap parsing → dispatch to daemon start or client methods
- `cargo build -p pensa` — verify clean compile
- `cargo clippy -p pensa -- -D warnings` — zero warnings
- `cargo fmt --all --check` — formatting clean
- `cargo geiger` — review any unsafe code usage
- `cargo build --workspace` — full workspace still builds

---

## Phase 7: Documentation

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

## Phase 8: Integration Tests

End-to-end tests that start a real daemon on a random port, run `pn` commands against it, and assert on stdout/stderr/exit codes. Following the pattern established in [`crates/ralph/tests/integration.rs`](crates/ralph/tests/integration.rs).

- **Source:** [`specs/pensa.md:387-402`](specs/pensa.md) — Testing Strategy
- **Tools required:** No additional tools beyond `cargo test`. Tests use the built `pn` binary via `env!("CARGO_BIN_EXE_pn")` and `std::process::Command` (same pattern as ralph).
- Create `crates/pensa/tests/integration.rs`:
  - **Test harness:**
    - `start_daemon()` → spawn `pn daemon --port 0 --project-dir <tempdir>` (or a random high port), wait for readiness via `pn daemon status`, return the port and child process handle
    - `pn_cmd(port)` → build a `Command` with `PN_DAEMON=http://localhost:{port}` and `PN_ACTOR=test-agent`
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
  - **Test: Dependencies and ready** ([`specs/pensa.md:395`](specs/pensa.md)):
    - Create task-A and task-B → `pn dep add B A` (B depends on A)
    - `pn ready --json` → assert B is absent (blocked), A is present
    - `pn blocked --json` → assert B is listed
    - `pn close A` → `pn ready --json` → assert B now appears
    - `pn dep list B --json` → verify dep structure
  - **Test: Cycle detection** ([`specs/pensa.md:396`](specs/pensa.md)):
    - Create A, B, C → `pn dep add B A` → `pn dep add C B` → `pn dep add A C` → assert exit 1, stderr contains `cycle_detected`
    - `pn dep cycles --json` → assert `[]` (the cycle was rejected)
  - **Test: `fixes` auto-close** ([`specs/pensa.md:397`](specs/pensa.md)):
    - Create bug → create task with `--fixes <bug-id>` → close the task → `pn show <bug-id> --json` → assert bug is also closed, close_reason contains "fixed by"
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
  - **Test: JSON output shapes** ([`specs/pensa.md:402`](specs/pensa.md)):
    - For each command, run with `--json` and validate the output matches the documented shapes ([`specs/pensa.md:290-305`](specs/pensa.md)):
      - `create` returns single issue object
      - `list` returns array
      - `ready` returns array (and `[]` when nothing matches — [`specs/pensa.md:187`](specs/pensa.md))
      - `count` returns `{"count": N}` or `{"total": N, "groups": [...]}`
      - `dep tree` returns flat array of `DepTreeNode` objects
      - etc.
  - **Test: Human-readable output**:
    - Run `pn list` (without `--json`) → assert stdout contains formatted text, not JSON
  - **Test: Daemon status**:
    - `pn daemon status` when daemon is running → assert exit 0
    - `pn daemon status` when daemon is not running → assert exit 1
  - **Test: `pn where`**:
    - Assert it prints the `.pensa/` directory path and exits 0
    - Assert it works without a running daemon (client-only command)
  - **Test: Delete with force**:
    - Create issue with comments → `pn delete <id>` (no force) → assert exit 1
    - `pn delete <id> --force` → assert exit 0
    - `pn show <id>` → assert exit 1, not_found
  - **Test: Search**:
    - Create issues with distinct titles → `pn search "login" --json` → assert only matching issues returned
    - Verify case-insensitive: search for "LOGIN" matches "login crash"
  - **Test: Comments**:
    - Create issue → `pn comment add <id> "observation" --json` → assert comment returned
    - `pn comment list <id> --json` → assert array with the comment
  - **Test: History**:
    - Create issue → update it → close it → `pn history <id> --json` → assert events in newest-first order, covering create/update/close
