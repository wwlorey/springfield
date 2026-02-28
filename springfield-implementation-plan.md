# Springfield (`sgf`) Implementation Plan

Implementation plan for the `sgf` CLI binary, based on [`specs/springfield.md`](specs/springfield.md).

The `sgf` crate is the CLI entry point for Springfield — it handles project scaffolding, prompt assembly, loop orchestration, recovery, and daemon lifecycle. It delegates iteration execution to ralph and persistent memory to pensa.

---

## Progress

| Phase | Status | Notes |
|-------|--------|-------|
| 0 | ✅ Complete | Workspace builds, tests, lint, format all pass |
| 1 | ✅ Complete | Full CLI skeleton with all subcommands |
| 2 | ✅ Complete | `sgf init` directory/file scaffolding with 26 unit tests |
| 3 | ✅ Complete | Config merging (.gitignore, settings.json, pre-commit) |
| 4 | ✅ Complete | Prompt assembly with `{{var}}` substitution and validation |
| 5 | ✅ Complete | Loop ID, PID files, log teeing, `sgf logs` |
| 6 | ✅ Complete | Pre-launch recovery and daemon lifecycle |
| 7 | ✅ Complete | Loop orchestration core with 12 unit tests |
| 8 | ✅ Complete | Workflow commands wired through orchestrate core, 7 new tests |
| 9 | ✅ Complete | Docker template build with embedded Dockerfile, 7 unit tests |
| 10 | Pending | Documentation |
| 11 | Pending | Integration tests |

### Lessons Learned

- **Test ordering matters for git-based tests**: PID files and other untracked artifacts must be created AFTER `git init` + `git add .` + `git commit` to avoid being accidentally tracked. Otherwise `git checkout -- .` restores them during recovery tests.
- **Pre-existing formatting issues**: The pensa proptest file had formatting issues that needed `cargo fmt` before clean checks. Always run `cargo fmt --all` early.
- **Module wiring**: Phase 5 (`loop_mgmt.rs`) existed on disk but wasn't declared in `lib.rs` — the `libc` dependency was also missing from `Cargo.toml`. Files must be wired into the module tree and all deps declared.

---

## Phase 0: Toolchain & Environment Setup

Install all tools referenced in [`AGENTS.md`](AGENTS.md) and verify the workspace builds.

- [x] Install Rust toolchain (stable, 1.85+ required for `edition = "2024"` — see [`Cargo.toml:7`](Cargo.toml))
- [ ] Install `cargo-geiger`: `cargo install cargo-geiger` ([`AGENTS.md`](AGENTS.md) "Detect unsafe code usage")
- [x] `cargo build --workspace` compiles
- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes

---

## Phase 1: Crate Scaffolding & CLI Skeleton

Create the `crates/sgf/` directory structure, `Cargo.toml`, and full clap CLI skeleton with all subcommands defined.

**Source**: [`specs/springfield.md:19-31`](specs/springfield.md) (CLI Commands), [`specs/springfield.md:33-39`](specs/springfield.md) (Common Flags)

- [x] Create `crates/sgf/Cargo.toml` with `name = "sgf"`, `[[bin]] name = "sgf"`, workspace edition/version/license
- [x] Add dependencies: `clap` (4, derive+env), `serde` (1, derive), `serde_json` (1), `chrono` (0.4), `signal-hook` (0.4)
- [x] Add dev-dependencies: `tempfile` (3), `assert_cmd` (2), `predicates` (3), `portpicker` (0.1)
- [x] Create `crates/sgf/src/lib.rs` with module declarations
- [x] Create `crates/sgf/src/main.rs` with clap `#[derive(Parser)]` skeleton
- [x] Define `init` subcommand — no args ([`specs/springfield.md:43-45`](specs/springfield.md))
- [x] Define `spec` subcommand — no args ([`specs/springfield.md:396-414`](specs/springfield.md))
- [x] Define `build <spec>` subcommand + common flags `-a`/`--afk`, `--no-push`, `N` ([`specs/springfield.md:416-421`](specs/springfield.md))
- [x] Define `verify` subcommand + common flags ([`specs/springfield.md:426-435`](specs/springfield.md))
- [x] Define `test-plan` subcommand + common flags ([`specs/springfield.md:437-445`](specs/springfield.md))
- [x] Define `test <spec>` subcommand + common flags ([`specs/springfield.md:447-451`](specs/springfield.md))
- [x] Define `issues log` subcommand — no flags ([`specs/springfield.md:453-461`](specs/springfield.md))
- [x] Define `issues plan` subcommand + common flags ([`specs/springfield.md:463-469`](specs/springfield.md))
- [x] Define `status` subcommand — no args, future work placeholder ([`specs/springfield.md:28`](specs/springfield.md))
- [x] Define `logs <loop-id>` subcommand — one positional arg ([`specs/springfield.md:329-332`](specs/springfield.md))
- [x] Define `template build` subcommand — no args ([`specs/springfield.md:836-845`](specs/springfield.md))
- [x] `cargo build -p sgf` compiles
- [x] `cargo clippy -p sgf -- -D warnings` passes
- [x] `sgf --help` shows all subcommands
- [x] `sgf build --help` shows `-a`, `--no-push`, `<spec>`, `[N]`
- [x] `cargo test --workspace` still passes (no regressions)

---

## Prerequisite: Ralph `--loop-id` flag

sgf passes `--loop-id` to ralph ([`specs/springfield.md:286`](specs/springfield.md)). Ralph's spec defines this flag ([`specs/ralph.md:90-91`](specs/ralph.md)) but the current implementation is missing it. This is tracked separately in the ralph implementation plan and must be completed before Phase 7 (loop orchestration).

---

## Phase 2: `sgf init` — Directory & File Scaffolding

Implement the core `sgf init` command that creates the full project structure.

**Source**: [`specs/springfield.md:43-184`](specs/springfield.md) (sgf init)

- [x] Create `crates/sgf/src/init.rs` module
- [x] Wire `pub mod init` in `lib.rs`
- [x] Wire `sgf init` in `main.rs` to call `init::run()`

### Template files

- [x] Create `crates/sgf/templates/` directory for `include_str!` embedding
- [x] Create `crates/sgf/templates/backpressure.md` with content from [`specs/springfield.md:662-754`](specs/springfield.md)
- [x] Create `crates/sgf/templates/spec.md` with content from [`specs/springfield.md:483-509`](specs/springfield.md)
- [x] Create `crates/sgf/templates/build.md` with content from [`specs/springfield.md:513-537`](specs/springfield.md)
- [x] Create `crates/sgf/templates/verify.md` with content from [`specs/springfield.md:541-559`](specs/springfield.md)
- [x] Create `crates/sgf/templates/test-plan.md` with content from [`specs/springfield.md:563-579`](specs/springfield.md)
- [x] Create `crates/sgf/templates/test.md` with content from [`specs/springfield.md:583-607`](specs/springfield.md)
- [x] Create `crates/sgf/templates/issues.md` with content from [`specs/springfield.md:611-627`](specs/springfield.md)
- [x] Create `crates/sgf/templates/issues-plan.md` with content from [`specs/springfield.md:631-654`](specs/springfield.md)

### Directory creation

Per [`specs/springfield.md:49-71`](specs/springfield.md):

- [x] Create `.pensa/` directory
- [x] Create `.sgf/` directory
- [x] Create `.sgf/logs/` directory
- [x] Create `.sgf/run/` directory
- [x] Create `.sgf/prompts/` directory
- [x] Create `.sgf/prompts/.assembled/` directory
- [x] Create `specs/` directory

### File creation (skip if exists)

Per [`specs/springfield.md:49-71`](specs/springfield.md):

- [x] Write `.sgf/backpressure.md` (from embedded template)
- [x] Write `.sgf/prompts/spec.md` (from embedded template)
- [x] Write `.sgf/prompts/build.md` (from embedded template)
- [x] Write `.sgf/prompts/verify.md` (from embedded template)
- [x] Write `.sgf/prompts/test-plan.md` (from embedded template)
- [x] Write `.sgf/prompts/test.md` (from embedded template)
- [x] Write `.sgf/prompts/issues.md` (from embedded template)
- [x] Write `.sgf/prompts/issues-plan.md` (from embedded template)
- [x] Write `memento.md` with skeleton content from [`specs/springfield.md:153-165`](specs/springfield.md)
- [x] Write `CLAUDE.md` with `Read memento.md and AGENTS.md before starting work.` ([`specs/springfield.md:169-171`](specs/springfield.md))
- [x] Write `specs/README.md` with empty spec index table ([`specs/springfield.md:176-180`](specs/springfield.md))

### Verification

- [x] `sgf init` in a temp dir creates all 7 directories
- [x] `sgf init` in a temp dir creates all 11 files
- [x] File contents match spec exactly (spot-check `CLAUDE.md`, `memento.md`, one prompt template)
- [x] Existing files are NOT overwritten on second run
- [x] `cargo build -p sgf` compiles
- [x] `cargo clippy -p sgf -- -D warnings` passes

---

## Phase 3: `sgf init` — Config Merging

Implement the merge logic for `.gitignore`, `.claude/settings.json`, and `.pre-commit-config.yaml`.

**Source**: [`specs/springfield.md:75-149`](specs/springfield.md)

### `.gitignore` (idempotent append)

Per [`specs/springfield.md:118-149`](specs/springfield.md):

- [x] Create `.gitignore` from scratch when it doesn't exist with all entries
- [x] Append missing entries to existing `.gitignore` without duplicating existing lines
- [x] Include all entries: `.pensa/db.sqlite`, `.sgf/logs/`, `.sgf/run/`, `.sgf/prompts/.assembled/`, `.ralph-complete`, `.ralph-ding`, `/target`, `node_modules/`, `.svelte-kit/`, `.env`, `.env.local`, `.env.*.local`, `.DS_Store`
- [x] Include `# Springfield` section header when creating from scratch

### `.claude/settings.json` (deny rules merge)

Per [`specs/springfield.md:75-90`](specs/springfield.md):

- [x] Create `.claude/` directory if it doesn't exist
- [x] Create `.claude/settings.json` from scratch with all 4 deny rules when it doesn't exist
- [x] Parse existing `.claude/settings.json` and merge deny rules without duplicating or removing existing rules
- [x] Deny rules: `Edit .sgf/**`, `Write .sgf/**`, `Bash rm .sgf/**`, `Bash mv .sgf/**`

### `.pre-commit-config.yaml` (hook append)

Per [`specs/springfield.md:94-114`](specs/springfield.md):

- [x] Add `serde_yaml` dependency to `Cargo.toml`
- [x] Create `.pre-commit-config.yaml` from scratch with full YAML content when it doesn't exist
- [x] Append pensa hooks to existing file without duplicating (check for hook IDs `pensa-export`, `pensa-import`)
- [x] `pensa-export` hook: pre-commit stage, `pn export`, `always_run: true`
- [x] `pensa-import` hook: post-merge/post-checkout/post-rewrite stages, `pn import`, `always_run: true`

### Verification

- [x] `.gitignore` created from scratch contains all entries
- [x] `.gitignore` with existing custom entries: custom preserved, sgf entries added, no duplicates
- [x] `.gitignore` idempotent: running twice produces identical file
- [x] `.claude/settings.json` created from scratch contains all 4 deny rules
- [x] `.claude/settings.json` with existing custom deny rules: custom preserved, sgf rules added, no duplicates
- [x] `.pre-commit-config.yaml` created from scratch contains both hooks
- [x] `.pre-commit-config.yaml` with existing hooks: existing preserved, pensa hooks added, no duplicates
- [x] Full `sgf init` run twice produces identical results across all config files

---

## Phase 4: Prompt Assembly

Implement the prompt assembly engine that reads templates, substitutes variables, validates, and writes assembled prompts.

**Source**: [`specs/springfield.md:245-268`](specs/springfield.md) (Prompt Assembly)

- [x] Create `crates/sgf/src/prompt.rs` module
- [x] Wire `pub mod prompt` in `lib.rs`
- [x] Implement `pub fn assemble(root: &Path, stage: &str, vars: &HashMap<String, String>) -> Result<PathBuf>`
- [x] Read template from `.sgf/prompts/<stage>.md`
- [x] Replace all `{{key}}` with corresponding value from `vars`
- [x] Validate: string scan for remaining `{{...}}` tokens, return error listing unresolved names
- [x] Create `.sgf/prompts/.assembled/` directory if it doesn't exist
- [x] Write assembled prompt to `.sgf/prompts/.assembled/<stage>.md`
- [x] Return the path to the assembled file
- [x] Error on missing template file with descriptive message

### Verification

- [x] Template with `{{spec}}` produces correct substitution (e.g., `pn ready --spec auth --json`)
- [x] Template without variables passes through unchanged
- [x] Template with unresolved `{{unknown}}` returns error naming the unresolved token
- [x] Missing template file returns error with file path
- [x] `.assembled/` dir created automatically if absent
- [x] `cargo test -p sgf` passes (unit tests for assembly)
- [x] `cargo clippy -p sgf -- -D warnings` passes

### Notes

- Signature takes `root: &Path` as first arg (project root) rather than assuming cwd — consistent with `init::run()` and testable.
- Used simple string scanning (`find("{{")` / `find("}}"))` instead of regex — no extra dependency needed, the token format is simple enough.

---

## Phase 5: Loop ID, PID Files & Logging

Implement loop ID generation, PID file management, and log teeing.

**Source**: [`specs/springfield.md:310-332`](specs/springfield.md) (Loop ID, Logging)

- [x] Create `crates/sgf/src/loop_mgmt.rs` module
- [x] Wire `pub mod loop_mgmt` in `lib.rs`

### Loop ID generation

Per [`specs/springfield.md:312-318`](specs/springfield.md):

- [x] Implement `pub fn generate_loop_id(stage: &str, spec: Option<&str>) -> String`
- [x] Format: `<stage>-<spec>-<YYYYMMDDTHHmmss>` when spec provided
- [x] Format: `<stage>-<YYYYMMDDTHHmmss>` when no spec
- [x] Verify: `build` + `auth` → `build-auth-20260226T143000` pattern
- [x] Verify: `verify` + no spec → `verify-20260226T150000` pattern
- [x] Verify: `issues-plan` + no spec → `issues-plan-20260226T160000` pattern

### PID files

Per [`specs/springfield.md:339-341`](specs/springfield.md):

- [x] Implement `pub fn write_pid_file(root: &Path, loop_id: &str) -> Result<PathBuf>` — writes `std::process::id()` to `.sgf/run/<loop-id>.pid`
- [x] Implement `pub fn remove_pid_file(root: &Path, loop_id: &str)` — removes `.sgf/run/<loop-id>.pid`
- [x] Implement `pub fn list_pid_files(root: &Path) -> Vec<(String, u32)>` — reads all `.pid` files in `.sgf/run/`, returns `(loop_id, pid)` pairs
- [x] Implement `pub fn is_pid_alive(pid: u32) -> bool` — check via `kill -0`

### Log teeing

Per [`specs/springfield.md:325-328`](specs/springfield.md):

- [x] Implement log file creation at `.sgf/logs/<loop-id>.log`
- [x] In AFK mode: read ralph stdout line-by-line, write each line to both terminal stdout and log file
- [x] In interactive mode: inherit stdio (no log teeing possible)

### `sgf logs` command

Per [`specs/springfield.md:329-332`](specs/springfield.md):

- [x] Wire `sgf logs <loop-id>` in `main.rs`
- [x] Run `tail -f .sgf/logs/<loop-id>.log` via `std::process::Command`
- [x] Exit 1 with error message if log file does not exist

### Verification

- [x] Loop ID format correct for each stage variant
- [x] PID file written contains current process ID
- [x] PID file removed on cleanup
- [x] `is_pid_alive` returns true for own PID, false for dead PID
- [x] `sgf logs` exits 1 for nonexistent log file
- [x] `cargo test -p sgf` passes
- [x] `cargo clippy -p sgf -- -D warnings` passes

### Notes

- All functions take `root: &Path` (project root) rather than assuming cwd — consistent with `init::run()` and `prompt::assemble()`, and testable with `TempDir`.
- Used `libc::kill(pid, 0)` for `is_pid_alive` — standard Unix process existence check, added `libc` dependency.
- `tee_output` takes a generic `io::Read` for testability — tests pass byte slices, production will pass ralph's piped stdout.
- `list_pid_files` uses chained `let` guards (Rust edition 2024 `let` chains) to collapse nested `if let` per clippy preference.

---

## Phase 6: Daemon Lifecycle & Pre-launch Recovery

Implement automatic pensa daemon startup and pre-launch cleanup of dirty state from crashed iterations.

**Source**: [`specs/springfield.md:357-367`](specs/springfield.md) (Daemon Lifecycle), [`specs/springfield.md:335-354`](specs/springfield.md) (Recovery)

- [x] Create `crates/sgf/src/recovery.rs` module
- [x] Wire `pub mod recovery` in `lib.rs`

### Daemon lifecycle

Per [`specs/springfield.md:360-366`](specs/springfield.md):

- [x] Implement `pub fn ensure_daemon(project_dir: &Path) -> Result<()>`
- [x] Check daemon reachability: run `pn daemon status`, check exit code 0
- [x] If not reachable: spawn `pn daemon --project-dir <project-root>` backgrounded
- [x] Poll `pn daemon status` every 100ms, timeout after 5 seconds
- [x] Return error if timeout expires

### Pre-launch recovery

Per [`specs/springfield.md:345-354`](specs/springfield.md):

- [x] Implement `pub fn pre_launch_recovery(root: &Path) -> Result<()>`
- [x] Scan all PID files in `.sgf/run/` via `list_pid_files()`
- [x] If no PID files exist: skip recovery, return Ok
- [x] Check each PID for aliveness via `is_pid_alive()`
- [x] If any PID is alive: skip recovery (another loop is running), return Ok
- [x] If all PIDs are stale: remove stale PID files
- [x] Run `git checkout -- .` to discard tracked file modifications
- [x] Run `git clean -fd` to remove untracked files (respects `.gitignore`)
- [x] Run `pn doctor --fix` to release stale claims

### Execution order

- [x] Recovery runs BEFORE daemon startup: `pre_launch_recovery()` → `ensure_daemon()` → launch ralph

### Verification

- [x] `ensure_daemon` starts daemon when not running (requires `pn` on PATH)
- [x] `ensure_daemon` skips startup when daemon already running
- [x] `ensure_daemon` returns error on timeout
- [x] `pre_launch_recovery` skips when no PID files exist
- [x] `pre_launch_recovery` skips when a live PID exists
- [x] `pre_launch_recovery` cleans up when all PIDs are stale: removes PID files, runs git checkout, git clean, pn doctor
- [x] `cargo test -p sgf` passes
- [x] `cargo clippy -p sgf -- -D warnings` passes

### Notes

- Recovery warns on `pn doctor --fix` failure rather than returning error — allows recovery to complete even when `pn` is not installed.
- `ensure_daemon` spawns with null stdio to avoid interfering with sgf's terminal output.
- `daemon_is_reachable` uses `is_ok_and()` for clean one-liner status check.

---

## Phase 7: Loop Orchestration Core

Implement the core loop orchestration that launches ralph with the correct flags, manages the process lifecycle, tees logs, and handles exit codes.

**Source**: [`specs/springfield.md:270-302`](specs/springfield.md) (sgf-to-ralph Contract)

- [x] Create `crates/sgf/src/orchestrate.rs` module
- [x] Wire `pub mod orchestrate` in `lib.rs`

### Ralph binary resolution

- [x] Check `SGF_RALPH_BINARY` env var first (for testing)
- [x] Fall back to `ralph` on `PATH`

### Flag translation

Per [`specs/springfield.md:281-291`](specs/springfield.md):

- [x] Pass `-a` / `--afk` when sgf's `-a` flag is set
- [x] Pass `--loop-id <id>` with sgf-generated loop ID
- [x] Pass `--template ralph-sandbox:latest` (hardcoded, [`specs/springfield.md:860`](specs/springfield.md))
- [x] Pass `--auto-push true` unless `--no-push` was passed to sgf
- [x] Pass `--max-iterations 30` (hardcoded, [`specs/springfield.md:859`](specs/springfield.md))
- [x] Pass iterations as positional arg (default `30`)
- [x] Pass assembled prompt file path as positional arg (from Phase 4)

### Process lifecycle

- [x] Spawn ralph via `std::process::Command`
- [x] AFK mode: pipe stdout, tee to terminal + `.sgf/logs/<loop-id>.log`
- [x] Interactive mode: inherit stdin/stdout/stderr
- [x] Write PID file before launching ralph
- [x] Remove PID file after ralph exits (success or failure)
- [x] Remove PID file on signal interrupt

### Exit code handling

Per [`specs/springfield.md:293-302`](specs/springfield.md):

- [x] Exit 0 from ralph: print success, clean up PID file
- [x] Exit 1 from ralph: print error, clean up PID file, exit 1
- [x] Exit 2 from ralph: print "iterations exhausted", clean up PID file, exit 2
- [x] Exit 130 from ralph: print "interrupted", clean up PID file, exit 130

### Signal handling

- [x] Register SIGINT/SIGTERM handlers via `signal-hook`
- [x] On interrupt: kill ralph child process, clean up PID file, exit 130

### Verification

- [x] Mock ralph receives correct flags for `sgf build auth -a --no-push 10`
- [x] Mock ralph receives `--auto-push true` when `--no-push` not passed
- [x] Mock ralph receives `--template ralph-sandbox:latest`
- [x] Mock ralph receives `--max-iterations 30`
- [x] PID file exists while ralph is running
- [x] PID file removed after ralph exits
- [x] AFK mode creates log file with ralph's output
- [x] `SGF_RALPH_BINARY` env var overrides ralph binary path
- [x] `cargo test -p sgf` passes
- [x] `cargo clippy -p sgf -- -D warnings` passes

### Notes

- `LoopConfig` uses `ralph_binary: Option<String>` for direct binary override (avoids `unsafe` env var mutation in Rust 2024 edition tests) with fallback to `SGF_RALPH_BINARY` env var, then `ralph` on PATH.
- `skip_preflight: bool` on `LoopConfig` allows unit tests to bypass recovery/daemon startup (those are already tested independently in `recovery.rs`).
- Signal handling uses `signal-hook::flag` for safe atomic bool flip — poll loop checks the flag and sends SIGTERM to the ralph child process.
- AFK mode tees stdout in a background thread via `loop_mgmt::tee_output`, while the main thread polls `try_wait()` + interrupt flag.
- Interactive mode inherits all stdio directly (no log teeing possible, per spec).

---

## Phase 8: Workflow Commands

Wire all workflow commands through the orchestration core.

**Source**: [`specs/springfield.md:370-469`](specs/springfield.md) (Workflow Stages)

### Looped stages

Each: assemble prompt → `pre_launch_recovery()` → `ensure_daemon()` → launch ralph → handle exit.

- [x] `sgf build <spec>`: template `build.md`, `{{spec}}` substitution, loop ID `build-<spec>-<ts>`, supports `-a`/`--no-push`/`N` ([`specs/springfield.md:416-421`](specs/springfield.md))
- [x] `sgf test <spec>`: template `test.md`, `{{spec}}` substitution, loop ID `test-<spec>-<ts>`, supports `-a`/`--no-push`/`N` ([`specs/springfield.md:447-451`](specs/springfield.md))
- [x] `sgf verify`: template `verify.md`, no variables, loop ID `verify-<ts>`, supports `-a`/`--no-push`/`N` ([`specs/springfield.md:426-435`](specs/springfield.md))
- [x] `sgf test-plan`: template `test-plan.md`, no variables, loop ID `test-plan-<ts>`, supports `-a`/`--no-push`/`N` ([`specs/springfield.md:437-445`](specs/springfield.md))
- [x] `sgf issues plan`: template `issues-plan.md`, no variables, loop ID `issues-plan-<ts>`, supports `-a`/`--no-push`/`N` ([`specs/springfield.md:463-469`](specs/springfield.md))

### Interactive stages (1 iteration, no AFK)

- [x] `sgf spec`: template `spec.md`, no variables, loop ID `spec-<ts>`, hardcoded 1 iteration, interactive mode ([`specs/springfield.md:396-414`](specs/springfield.md))
- [x] `sgf issues log`: template `issues.md`, no variables, loop ID `issues-log-<ts>`, hardcoded 1 iteration, interactive mode ([`specs/springfield.md:453-461`](specs/springfield.md))

### Utility commands

- [x] `sgf logs <loop-id>`: run `tail -f .sgf/logs/<loop-id>.log`, exit 1 if missing ([`specs/springfield.md:329-332`](specs/springfield.md))
- [x] `sgf status`: print "Not yet implemented", exit 0 ([`specs/springfield.md:28`](specs/springfield.md), [`specs/springfield.md:897`](specs/springfield.md))

### Verification

- [x] `sgf build auth` assembles prompt with `{{spec}}` = `auth` and invokes ralph
- [x] `sgf verify` assembles prompt without variables and invokes ralph
- [x] `sgf spec` invokes ralph with 1 iteration, no `--afk`
- [x] `sgf issues log` invokes ralph with 1 iteration, no `--afk`
- [x] `sgf build` without `<spec>` arg shows clap error
- [x] `sgf test` without `<spec>` arg shows clap error
- [x] `sgf logs nonexistent` exits 1
- [x] `sgf status` exits 0 with placeholder message
- [x] `cargo test -p sgf` passes
- [x] `cargo clippy -p sgf -- -D warnings` passes

### Notes

- Added `prompt_template: Option<String>` to `LoopConfig` to decouple the loop ID stage name from the prompt template name. Only `sgf issues log` needs this — its loop ID uses `issues-log` but its template is `issues.md`.
- All workflow commands delegate to a shared `run_loop()` helper in `main.rs` that constructs `LoopConfig` and calls `orchestrate::run()`.
- Interactive stages (`spec`, `issues log`) hardcode `iterations: 1`, `afk: false`, `no_push: false` — they bypass the `LoopOpts` clap struct entirely.
- `sgf logs` and `sgf status` were already wired in Phase 5/7; no changes needed.

---

## Phase 9: Docker Template Build

Implement `sgf template build` to build the `ralph-sandbox:latest` Docker image.

**Source**: [`specs/springfield.md:758-845`](specs/springfield.md) (Docker Sandbox Template)

### Dockerfile

- [x] Create `.docker/sandbox-templates/ralph/Dockerfile` with content from [`specs/springfield.md:771-832`](specs/springfield.md)
- [x] Embed Dockerfile in sgf binary via `include_str!("../../../.docker/sandbox-templates/ralph/Dockerfile")`

### `sgf template build` implementation

Per [`specs/springfield.md:838-845`](specs/springfield.md):

- [x] Create `crates/sgf/src/template.rs` module
- [x] Wire `pub mod template` in `lib.rs`
- [x] Wire `sgf template build` in `main.rs`
- [x] Locate `pn` binary via `which pn` (or `Command::new("which").arg("pn")`)
- [x] Exit with descriptive error if `pn` not found
- [x] Create temporary build context directory via `tempfile::tempdir()`
- [x] Write embedded Dockerfile to temp dir
- [x] Copy `pn` binary into temp build context
- [x] Run `docker build -t ralph-sandbox:latest .` in the temp dir
- [x] Exit with error message if docker build fails
- [x] Temp directory cleaned up automatically via `TempDir` drop

### Verification

- [x] `sgf template build` with `pn` not on PATH: exits 1 with "pn not found" message
- [x] `sgf template build` with `pn` on PATH and Docker available: exits 0, image built
- [x] `docker image inspect ralph-sandbox:latest` succeeds after build
- [x] `cargo build -p sgf` compiles (include_str! resolves)
- [x] `cargo clippy -p sgf -- -D warnings` passes

**Note**: Docker-dependent tests should be gated behind `#[ignore]`.

### Notes

- Moved `tempfile` from `[dev-dependencies]` to `[dependencies]` — it's now used at runtime for the build context temporary directory.
- The `locate_pn()` function validates existence after `which` to catch edge cases where `which` returns a stale path.
- Docker build and `pn`-on-PATH tests are verified at the unit level (Dockerfile embedding, build context creation) without requiring Docker to be running.

---

## Phase 10: Documentation

- [ ] Create `crates/sgf/README.md`:
  - [ ] Project overview and purpose
  - [ ] Architecture summary (scaffolding, prompt assembly, loop orchestration, recovery, daemon lifecycle)
  - [ ] Quick start / usage examples for each command
  - [ ] Command reference table (with link to spec for details)
  - [ ] Relationship to ralph and pensa
  - [ ] Testing instructions (`cargo test -p sgf`)
- [ ] Update root [`README.md`](README.md):
  - [ ] Add sgf to architecture diagram / component listing
  - [ ] Ensure all three crates (sgf, ralph, pensa) are mentioned
- [ ] Update [`AGENTS.md`](AGENTS.md):
  - [ ] Add `cargo build -p sgf` and `cargo test -p sgf` examples
- [ ] Update [`specs/README.md`](specs/README.md):
  - [ ] Verify `springfield.md` code path points to `crates/sgf/` (not `crates/springfield/`)

### Verification

- [ ] All README files reference correct file paths
- [ ] AGENTS.md build/test examples include sgf
- [ ] specs/README.md code column is accurate

---

## Phase 11: Integration Tests

End-to-end tests that verify sgf commands work correctly from the command line.

**Test infrastructure**: Following patterns from pensa ([`crates/pensa/tests/integration.rs`](crates/pensa/tests/integration.rs)) and ralph ([`crates/ralph/tests/integration.rs`](crates/ralph/tests/integration.rs)).

- [ ] Create `crates/sgf/tests/integration.rs`

### Test harness

- [ ] `setup_test_dir()` → `TempDir` with git init + initial commit
- [ ] `sgf_cmd(dir)` → `Command` for sgf binary with `current_dir` set
- [ ] Mock ralph script helper: shell script that prints args to a file, optionally prints output, exits with configurable code
- [ ] Mock ralph pointed to via `SGF_RALPH_BINARY` env var

### `sgf init` tests

- [ ] **`init_creates_all_files`** — run `sgf init`, verify all directories exist (`.pensa/`, `.sgf/`, `.sgf/logs/`, `.sgf/run/`, `.sgf/prompts/`, `.sgf/prompts/.assembled/`, `specs/`)
- [ ] **`init_creates_all_files`** (cont.) — verify all files exist (`.sgf/backpressure.md`, 7 prompt templates, `memento.md`, `CLAUDE.md`, `specs/README.md`, `.claude/settings.json`, `.pre-commit-config.yaml`, `.gitignore`)
- [ ] **`init_file_contents`** — `CLAUDE.md` contains `Read memento.md and AGENTS.md`
- [ ] **`init_file_contents`** (cont.) — `memento.md` contains `## Stack` and `## References`
- [ ] **`init_file_contents`** (cont.) — `.claude/settings.json` contains all 4 deny rules
- [ ] **`init_file_contents`** (cont.) — `.gitignore` contains `.pensa/db.sqlite` and `.sgf/logs/`
- [ ] **`init_idempotent`** — run `sgf init` twice: no duplicate `.gitignore` lines, no duplicate deny rules, no duplicate hooks
- [ ] **`init_idempotent`** (cont.) — modify a prompt template after first init, run init again, verify modification persists
- [ ] **`init_merges_existing_gitignore`** — create `.gitignore` with custom entries, `sgf init`, verify custom entries preserved + sgf entries added
- [ ] **`init_merges_existing_settings_json`** — create `.claude/settings.json` with custom deny rules, `sgf init`, verify custom rules preserved + sgf rules added

### Prompt assembly tests

- [ ] **`prompt_assembly_substitutes_spec`** — set up `.sgf/prompts/build.md` with `{{spec}}`, run assembly, verify `.assembled/build.md` has `spec` value substituted
- [ ] **`prompt_assembly_validates_unresolved`** — template with `{{unknown}}`, verify assembly fails with descriptive error
- [ ] **`prompt_assembly_passthrough`** — template without variables, verify assembled output matches input exactly

### Loop orchestration tests (mocked ralph)

- [ ] **`build_invokes_ralph_with_correct_flags`** — `sgf build auth -a` with mock ralph, verify mock received `--afk`, `--loop-id`, `--template ralph-sandbox:latest`, `--auto-push true`, `--max-iterations 30`, iterations `30`, prompt path
- [ ] **`build_creates_and_cleans_pid_file`** — run `sgf build auth` with mock ralph, verify PID file removed after exit
- [ ] **`afk_tees_output_to_log`** — `sgf build auth -a` with mock ralph that prints output, verify `.sgf/logs/<loop-id>.log` contains that output
- [ ] **`spec_runs_one_interactive_iteration`** — `sgf spec` with mock ralph, verify mock received `1` iteration and no `--afk`
- [ ] **`issues_log_runs_one_interactive_iteration`** — `sgf issues log` with mock ralph, verify mock received `1` iteration and no `--afk`

### Recovery tests

- [ ] **`recovery_cleans_stale_state`** — create stale PID file (dead PID), dirty git state, run `sgf build auth` with mock ralph, verify git state clean before ralph started
- [ ] **`recovery_skips_when_live_pid`** — create PID file with own PID (alive), run `sgf build auth`, verify recovery did NOT run (dirty state preserved)

### Utility tests

- [ ] **`logs_exits_1_for_missing`** — `sgf logs nonexistent`, verify exit 1 with error message
- [ ] **`status_prints_placeholder`** — `sgf status`, verify exit 0 with output
- [ ] **`help_flag`** — `sgf --help`, verify exit 0, output contains subcommand names

### Docker template test (gated)

- [ ] **`template_build_requires_pn`** — `sgf template build` without `pn` on PATH, verify exit 1 with descriptive error (gate behind `#[ignore]` if Docker unavailable)

### Final verification

- [ ] `cargo test -p sgf` — all non-ignored tests pass
- [ ] `cargo test --workspace` — all tests across all crates pass
- [ ] `cargo clippy --workspace -- -D warnings` — no warnings
- [ ] `cargo fmt --all --check` — formatting clean
