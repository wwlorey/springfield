# Ralph Implementation Plan

Gap analysis between `specs/ralph.md` (specification) and `crates/ralph/` (implementation). Each item cites the spec section and the source file + line that needs to change.

---

## Phase 1: Tooling Setup

- Install/verify Rust toolchain and workspace tools referenced in `AGENTS.md`:
  - `cargo build -p ralph` — verify the crate builds cleanly
  - `cargo test -p ralph` — verify existing tests pass
  - `cargo clippy -p ralph -- -D warnings` — verify lint-clean
  - `cargo fmt --all` — verify formatting
  - `cargo geiger` — verify no unsafe code beyond the existing PTY/setsid blocks (which are inherently `unsafe`)

---

## Phase 2: Fix Exit Code for Max Iterations Exhausted

**Spec reference:** `specs/ralph.md` lines 96–103 (Exit Codes table) and line 339 ("exit 2")

> | Code | Meaning |
> |------|---------|
> | `2`  | Iterations exhausted without completion |

**Current code:** `crates/ralph/src/main.rs:175` — uses `std::process::exit(1)` after the loop ends without completion.

**Changes:**
- [x] `crates/ralph/src/main.rs:175` — change `std::process::exit(1)` to `std::process::exit(2)`
- [x] `crates/ralph/tests/integration.rs:262` — update `afk_exhausts_iterations_without_promise` test: change `assert_eq!(output.status.code(), Some(1), ...)` to `assert_eq!(output.status.code(), Some(2), ...)`

**Status:** Complete. All 32 tests pass (19 unit + 13 integration), clippy clean, fmt clean.

---

## Phase 3: Add `--loop-id` CLI Flag

**Spec reference:** `specs/ralph.md` lines 66, 90, 115, 323, 327

> | `--loop-id` | — | — | Loop identifier (sgf-generated, included in banner output) |

**Current code:** `crates/ralph/src/main.rs` `Cli` struct (lines 52–80) — no `loop_id` field.

**Changes:**
- [x] `crates/ralph/src/main.rs` — add `loop_id: Option<String>` field to the `Cli` struct with `#[arg(long)]`
- [x] `crates/ralph/src/main.rs:117` — pass `loop_id` to `print_banner()`
- [x] `crates/ralph/src/main.rs:178–227` — update `print_banner()` signature and body to accept and display `loop_id` (e.g., `Loop ID:    build-auth-20260226T143000`)
- [x] `crates/ralph/src/main.rs:123–127` — update iteration banner to include loop ID when provided (spec line 327: "Print iteration banner (includes loop ID if provided)")

**Status:** Complete. All 35 tests pass (19 unit + 16 integration), clippy clean, fmt clean. Loop ID is displayed in startup banner as `Loop ID:     <id>` and in iteration banner as `Iteration N of M [<id>]`. Both are conditionally shown only when `--loop-id` is provided.

---

## Phase 4: Fix Default Docker Template

**Spec reference:** `specs/ralph.md` line 91 and `specs/springfield.md` line 762

> `--template` default value: `ralph-sandbox:latest`

**Current code:** `crates/ralph/src/main.rs:66` — `default_value = "tauri-sandbox:latest"`

**Changes:**
- [x] `crates/ralph/src/main.rs:66` — change `default_value = "tauri-sandbox:latest"` to `default_value = "ralph-sandbox:latest"`

**Status:** Complete. Single-line change to the `#[arg]` default value. All 36 tests pass (19 unit + 17 integration), clippy clean, fmt clean.

---

## Phase 5: Add Sentinel Files to `.gitignore`

**Spec reference:** `specs/ralph.md` lines 232–234 and 357

> "`.ralph-ding` must be listed in `.gitignore` to prevent accidental commits of the sentinel file."
> "The file [`.ralph-complete`] is gitignored."

Also from `specs/springfield.md` lines 126–129, the `sgf init` gitignore entries include both `.ralph-complete` and `.ralph-ding`.

**Current `.gitignore`:** Contains only `/target` and `.pensa/db.sqlite` — missing both sentinel entries.

**Changes:**
- [x] `.gitignore` — add `.ralph-complete` and `.ralph-ding` entries

**Status:** Complete. Added both sentinel file entries to `.gitignore`. All 98 tests pass across workspace, clippy clean, fmt clean.

---

## Phase 6: Documentation — `crates/ralph/README.md`

**Spec reference:** `specs/ralph.md` lines 1–3 (overview), 57–116 (CLI interface), 96–103 (exit codes)

The spec describes ralph as a standalone tool with a full CLI interface. A `README.md` should exist for the crate explaining:

**Changes:**
- [x] Create `crates/ralph/README.md` covering:
  - Purpose: iterative Claude Code runner via Docker sandbox
  - CLI usage synopsis and examples (drawn from spec lines 107–116)
  - Modes: interactive (default) vs AFK (`--afk`)
  - Exit codes table (spec lines 96–103)
  - NDJSON formatting overview (spec lines 236–301)
  - Testing: how to run tests (`cargo test -p ralph`)
  - Relationship to `sgf` (invoked by `sgf build`, `sgf test`, etc.)

**Status:** Complete. README covers all spec'd sections: purpose, CLI synopsis with arguments/flags/options/examples, modes (interactive + AFK), exit codes, NDJSON formatting table, testing commands, and sgf relationship. All 105 workspace tests pass, clippy clean, fmt clean.

---

## Phase 7: Integration Tests

**Spec reference:** `specs/ralph.md` lines 389–428 (Testing section)

Existing tests cover most spec'd cases. The following tests are missing or incorrect and need to be added/fixed:

### 7a. Fix existing test for exit code 2

- [x] `crates/ralph/tests/integration.rs` — `afk_exhausts_iterations_without_promise` (addressed in Phase 2, listed here for completeness)

### 7b. Add test: loop ID displayed in startup banner

**Spec reference:** `specs/ralph.md` line 323

> "The startup banner includes mode, prompt source, iteration count, sandbox template, and loop ID (if provided via `--loop-id`)"

- [x] `crates/ralph/tests/integration.rs` — add `loop_id_in_startup_banner` test:
  - Run ralph with `--loop-id build-auth-20260226T143000 --afk --command <mock> 1 prompt.md`
  - Assert stdout contains `build-auth-20260226T143000`

### 7c. Add test: loop ID displayed in iteration banner

**Spec reference:** `specs/ralph.md` line 327

- [x] `crates/ralph/tests/integration.rs` — add `loop_id_in_iteration_banner` test:
  - Same setup as 7b
  - Assert the iteration banner line includes the loop ID

### 7d. Add test: loop ID absent when not provided

- [x] `crates/ralph/tests/integration.rs` — add `no_loop_id_when_not_provided` test:
  - Run ralph WITHOUT `--loop-id`
  - Assert stdout does NOT contain "Loop ID" label

### 7e. Add test: correct Docker template default

- [x] `crates/ralph/tests/integration.rs` — add `default_template_in_banner` test:
  - Run ralph with default settings
  - Assert stdout banner contains `ralph-sandbox:latest`

### 7f. Verify all integration tests pass end-to-end

- [x] Run full test suite: `cargo test -p ralph` — 36 tests pass (19 unit + 17 integration)
- [x] Run full workspace checks:
  - `cargo build --workspace` — clean
  - `cargo clippy --workspace -- -D warnings` — clean
  - `cargo fmt -p ralph -- --check` — clean (note: `crates/sgf/` has pre-existing fmt issues unrelated to this change)

---

## Summary of All Files to Change

| File | Action | Phase |
|------|--------|-------|
| `crates/ralph/src/main.rs` | Edit: exit code 2, add `--loop-id`, fix template default, update banners | 2, 3, 4 |
| `crates/ralph/tests/integration.rs` | Edit: fix exit code assertion, add 4 new integration tests | 2, 7 |
| `.gitignore` | Edit: add `.ralph-complete` and `.ralph-ding` | 5 |
| `crates/ralph/README.md` | Create: crate-level documentation | 6 |
