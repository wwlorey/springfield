# Springfield (`sgf`) Implementation Plan

Implementation plan for the `sgf` CLI binary, based on [`specs/springfield.md`](specs/springfield.md).

The `sgf` crate is the CLI entry point for Springfield — it handles project scaffolding, prompt assembly, loop orchestration, recovery, and daemon lifecycle. It delegates iteration execution to ralph and persistent memory to pensa.

---

## Phase 0: Toolchain & Environment Setup

Install all tools referenced in [`AGENTS.md`](AGENTS.md) and verify the workspace builds.

- Install Rust toolchain (stable, 1.85+ required for `edition = "2024"` — see [`Cargo.toml:7`](Cargo.toml))
- Install `cargo-geiger` for unsafe code detection: `cargo install cargo-geiger` (referenced in [`AGENTS.md`](AGENTS.md) under "Detect unsafe code usage")
- Confirm existing workspace compiles: `cargo build --workspace`
- Confirm existing tests pass: `cargo test --workspace`
- Confirm linting passes: `cargo clippy --workspace -- -D warnings`
- Confirm formatting: `cargo fmt --all --check`

---

## Phase 1: Crate Scaffolding & CLI Skeleton

Create the `crates/sgf/` directory structure, `Cargo.toml`, and full clap CLI skeleton with all subcommands defined.

**Source**: [`specs/springfield.md:19-31`](specs/springfield.md) (CLI Commands), [`specs/springfield.md:33-39`](specs/springfield.md) (Common Flags)

### Files to create

- `crates/sgf/Cargo.toml` — `name = "sgf"`, `[[bin]] name = "sgf"`, workspace edition/version/license
  - Dependencies: `clap` (4, derive+env), `serde` (1, derive), `serde_json` (1), `chrono` (0.4) for loop ID timestamps, `signal-hook` (0.4) for interrupt handling
  - Dev-dependencies: `tempfile` (3), `assert_cmd` (2), `predicates` (3), `portpicker` (0.1)
- `crates/sgf/src/main.rs` — clap skeleton with all subcommands:
  - `init` — no args ([`specs/springfield.md:43-45`](specs/springfield.md))
  - `spec` — no args ([`specs/springfield.md:396-414`](specs/springfield.md))
  - `build <spec>` + common flags ([`specs/springfield.md:416-421`](specs/springfield.md))
  - `verify` + common flags ([`specs/springfield.md:426-435`](specs/springfield.md))
  - `test-plan` + common flags ([`specs/springfield.md:437-445`](specs/springfield.md))
  - `test <spec>` + common flags ([`specs/springfield.md:447-451`](specs/springfield.md))
  - `issues log` — no flags ([`specs/springfield.md:453-461`](specs/springfield.md))
  - `issues plan` + common flags ([`specs/springfield.md:463-469`](specs/springfield.md))
  - `status` — no args (future work placeholder, [`specs/springfield.md:28`](specs/springfield.md))
  - `logs <loop-id>` — one positional arg ([`specs/springfield.md:329-332`](specs/springfield.md))
  - `template build` — no args ([`specs/springfield.md:836-845`](specs/springfield.md))
- `crates/sgf/src/lib.rs` — module declarations

### Common flags (all loop commands)

Per [`specs/springfield.md:33-39`](specs/springfield.md):

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | `false` | AFK mode |
| `--no-push` | `false` | Disable auto-push |
| `N` (positional) | `30` | Number of iterations |

### Verification

- `cargo build -p sgf` compiles
- `cargo clippy -p sgf -- -D warnings` passes
- `sgf --help` shows all subcommands
- `sgf init --help`, `sgf build --help`, etc. show expected flags
- `cargo test --workspace` (existing tests still pass)

---

## Prerequisite: Ralph `--loop-id` flag

sgf passes `--loop-id` to ralph ([`specs/springfield.md:286`](specs/springfield.md)). Ralph's spec defines this flag ([`specs/ralph.md:90-91`](specs/ralph.md)) but the current implementation is missing it. This is tracked separately in the ralph implementation plan and must be completed before Phase 7 (loop orchestration).

---

## Phase 2: `sgf init` — Directory & File Scaffolding

Implement the core `sgf init` command that creates the full project structure.

**Source**: [`specs/springfield.md:43-184`](specs/springfield.md) (sgf init)

### Module to create

- `crates/sgf/src/init.rs` — all scaffolding logic

### Directories to create

Per [`specs/springfield.md:49-71`](specs/springfield.md):

- `.pensa/` — empty dir (daemon creates `db.sqlite` on start)
- `.sgf/` — config root
- `.sgf/logs/` — empty, gitignored
- `.sgf/run/` — empty, gitignored
- `.sgf/prompts/` — prompt templates
- `.sgf/prompts/.assembled/` — empty, gitignored
- `specs/` — spec files

### Files to create (skip if exists — idempotent)

Per [`specs/springfield.md:49-71`](specs/springfield.md):

- `.sgf/backpressure.md` — full backpressure template (content at [`specs/springfield.md:662-754`](specs/springfield.md))
- `.sgf/prompts/spec.md` — spec prompt (content at [`specs/springfield.md:483-509`](specs/springfield.md))
- `.sgf/prompts/build.md` — build prompt (content at [`specs/springfield.md:513-537`](specs/springfield.md))
- `.sgf/prompts/verify.md` — verify prompt (content at [`specs/springfield.md:541-559`](specs/springfield.md))
- `.sgf/prompts/test-plan.md` — test-plan prompt (content at [`specs/springfield.md:563-579`](specs/springfield.md))
- `.sgf/prompts/test.md` — test prompt (content at [`specs/springfield.md:583-607`](specs/springfield.md))
- `.sgf/prompts/issues.md` — issues prompt (content at [`specs/springfield.md:611-627`](specs/springfield.md))
- `.sgf/prompts/issues-plan.md` — issues-plan prompt (content at [`specs/springfield.md:631-654`](specs/springfield.md))
- `memento.md` — skeleton reference doc (content at [`specs/springfield.md:153-165`](specs/springfield.md))
- `CLAUDE.md` — entry point (content at [`specs/springfield.md:169-171`](specs/springfield.md): `Read memento.md and AGENTS.md before starting work.`)
- `specs/README.md` — empty spec index (content at [`specs/springfield.md:176-180`](specs/springfield.md))

### Embedding strategy

All prompt template content and backpressure template should be embedded in the binary. Two options:
1. **`include_str!` from files** — store templates in `crates/sgf/templates/` and embed at compile time
2. **Const strings** — inline in Rust source

Use `include_str!` for larger files (backpressure template, prompt templates) to keep source readable. Store templates in `crates/sgf/templates/`.

### Verification

- `sgf init` in a temp dir creates all expected files and directories
- File contents match spec exactly
- `sgf init` is idempotent — running twice does not duplicate content or overwrite existing files

---

## Phase 3: `sgf init` — Config Merging

Implement the merge logic for `.gitignore`, `.claude/settings.json`, and `.pre-commit-config.yaml`.

**Source**: [`specs/springfield.md:75-149`](specs/springfield.md)

### `.gitignore` (idempotent append)

Per [`specs/springfield.md:118-149`](specs/springfield.md):

- If `.gitignore` doesn't exist, create it with all entries
- If it exists, append only entries not already present (line-by-line dedup)
- All entries from the spec are always added regardless of directory contents
- Entries: `.pensa/db.sqlite`, `.sgf/logs/`, `.sgf/run/`, `.sgf/prompts/.assembled/`, `.ralph-complete`, `.ralph-ding`, `/target`, `node_modules/`, `.svelte-kit/`, `.env`, `.env.local`, `.env.*.local`, `.DS_Store`

### `.claude/settings.json` (deny rules merge)

Per [`specs/springfield.md:75-90`](specs/springfield.md):

- If `.claude/settings.json` doesn't exist, create `.claude/` dir and write the full JSON
- If it exists, parse existing JSON, merge deny rules into `permissions.deny` array without duplicating entries or removing existing rules
- Deny rules: `Edit .sgf/**`, `Write .sgf/**`, `Bash rm .sgf/**`, `Bash mv .sgf/**`

### `.pre-commit-config.yaml` (hook append)

Per [`specs/springfield.md:94-114`](specs/springfield.md):

- If `.pre-commit-config.yaml` doesn't exist, create it with the full YAML content
- If it exists, append pensa hooks without duplicating them (check for hook IDs `pensa-export`, `pensa-import`)
- Hook IDs: `pensa-export` (pre-commit stage, `pn export`), `pensa-import` (post-merge/post-checkout/post-rewrite, `pn import`)
- **Note**: YAML parsing — use `serde_yaml` for reliable merge, or do simple string matching for hook ID presence and raw append. `serde_yaml` is recommended for correctness.
- Add `serde_yaml` dependency to `Cargo.toml`

### Verification

- `.gitignore` created from scratch with correct content
- `.gitignore` append: existing entries not duplicated, new entries added
- `.claude/settings.json` created from scratch
- `.claude/settings.json` merge: existing deny rules preserved, sgf rules added without duplication
- `.pre-commit-config.yaml` created from scratch
- `.pre-commit-config.yaml` merge: existing hooks preserved, pensa hooks added
- Full idempotence: `sgf init` run twice produces identical results

---

## Phase 4: Prompt Assembly

Implement the prompt assembly engine that reads templates, substitutes variables, validates, and writes assembled prompts.

**Source**: [`specs/springfield.md:245-268`](specs/springfield.md) (Prompt Assembly)

### Module to create

- `crates/sgf/src/prompt.rs` — prompt assembly logic

### Assembly process

Per [`specs/springfield.md:249-267`](specs/springfield.md):

1. Read template from `.sgf/prompts/<stage>.md`
2. Substitute `{{var}}` tokens with values
3. Validate — scan for unresolved `{{...}}` tokens, fail with error before launching
4. Write assembled prompt to `.sgf/prompts/.assembled/<stage>.md`
5. Return the assembled file path

### Template variables

Per [`specs/springfield.md:259-265`](specs/springfield.md):

| Variable | Stages | Value |
|----------|--------|-------|
| `spec` | `build`, `test` | Spec stem from positional arg |

Other stages have no variables — templates are passed through unchanged but still written to `.assembled/`.

### Implementation

- `pub fn assemble(stage: &str, vars: &HashMap<String, String>) -> Result<PathBuf>`
- Read `.sgf/prompts/<stage>.md`
- Replace all `{{key}}` with `vars[key]`
- Regex scan for remaining `{{...}}` — if any found, return error with the unresolved token names
- Write to `.sgf/prompts/.assembled/<stage>.md` (create `.assembled/` dir if needed)
- Return the path to the assembled file

### Verification

- Assembly with `{{spec}}` substitution produces correct output
- Assembly without variables passes through unchanged
- Unresolved `{{...}}` token causes error with descriptive message
- Missing template file causes error
- `.assembled/` dir is created automatically

---

## Phase 5: Loop ID, PID Files & Logging

Implement loop ID generation, PID file management, and log teeing.

**Source**: [`specs/springfield.md:310-332`](specs/springfield.md) (Loop ID, Logging)

### Module to create

- `crates/sgf/src/loop_mgmt.rs` — loop ID generation, PID file lifecycle, log teeing

### Loop ID format

Per [`specs/springfield.md:312-318`](specs/springfield.md):

Pattern: `<stage>[-<spec>]-<YYYYMMDDTHHmmss>`

Examples:
- `build-auth-20260226T143000` (build loop for auth spec)
- `verify-20260226T150000` (verify loop, no spec filter)
- `issues-plan-20260226T160000` (issues plan loop)

### PID files

Per [`specs/springfield.md:339-341`](specs/springfield.md):

- Write `.sgf/run/<loop-id>.pid` on launch (containing process ID)
- Remove on clean exit
- `.sgf/run/` is gitignored

### Log teeing

Per [`specs/springfield.md:325-328`](specs/springfield.md):

- Tee ralph's stdout to both terminal and `.sgf/logs/<loop-id>.log`
- In AFK mode: pipe ralph stdout through sgf, write each line to both stdout and log file
- In interactive mode: stdout is inherited (terminal passthrough), logging is not possible without PTY trickery — log file will be empty or skipped
- `.sgf/logs/` is gitignored

### `sgf logs <loop-id>`

Per [`specs/springfield.md:329-332`](specs/springfield.md):

- Run `tail -f .sgf/logs/<loop-id>.log`
- If log file does not exist, print error and exit 1

### Verification

- Loop ID generation produces correct format for each stage
- PID file written on launch, removed on exit
- Log file populated in AFK mode
- `sgf logs` tails existing log file
- `sgf logs` exits 1 for nonexistent log

---

## Phase 6: Daemon Lifecycle & Pre-launch Recovery

Implement automatic pensa daemon startup and pre-launch cleanup of dirty state from crashed iterations.

**Source**: [`specs/springfield.md:357-367`](specs/springfield.md) (Daemon Lifecycle), [`specs/springfield.md:335-354`](specs/springfield.md) (Recovery)

### Module to create

- `crates/sgf/src/recovery.rs` — daemon lifecycle + pre-launch recovery

### Daemon lifecycle

Per [`specs/springfield.md:360-366`](specs/springfield.md):

1. Check if daemon is reachable: run `pn daemon status`, check exit code
2. If not reachable, start it: `pn daemon --project-dir <project-root> &` (backgrounded)
3. Wait for readiness: poll `pn daemon status` with short timeout (e.g., 5 seconds, 100ms intervals)
4. If timeout expires, exit with error

### Pre-launch recovery

Per [`specs/springfield.md:345-354`](specs/springfield.md):

Before launching ralph, scan all PID files in `.sgf/run/`:

1. **Any PID alive** (verified via `kill -0 <pid>`) → another loop is running. Skip cleanup and launch normally.
2. **All PIDs stale** (process dead) → no loops running. Remove stale PID files, then recover:
   - `git checkout -- .` — discard modifications to tracked files
   - `git clean -fd` — remove untracked files (respects `.gitignore`)
   - `pn doctor --fix` — release stale claims and repair integrity

### Implementation notes

- PID aliveness check: `unsafe { libc::kill(pid, 0) }` returns 0 if process exists (or use `nix` crate, but `libc` is already a dependency of ralph and keeps deps minimal)
- Or: use `std::process::Command::new("kill").args(["-0", &pid_str])` for safe alternative
- Recovery must run BEFORE daemon startup (daemon might be stale too)
- Actually, recovery order: scan PIDs → recover if stale → start daemon → launch ralph

### Verification

- Daemon auto-starts when not running (requires `pn` binary available)
- Daemon startup skipped when already running
- Stale PID files detected and removed
- Recovery runs git checkout + git clean + pn doctor when all PIDs stale
- Recovery skipped when a live PID exists

---

## Phase 7: Loop Orchestration Core

Implement the core loop orchestration that launches ralph with the correct flags, manages the process lifecycle, tees logs, and handles exit codes.

**Source**: [`specs/springfield.md:270-302`](specs/springfield.md) (sgf-to-ralph Contract)

### Module to create

- `crates/sgf/src/orchestrate.rs` — ralph invocation, flag translation, exit code handling

### sgf-to-ralph flag translation

Per [`specs/springfield.md:281-291`](specs/springfield.md):

| Flag | Source |
|------|--------|
| `-a` / `--afk` | From sgf command's `-a` flag |
| `--loop-id` | sgf-generated loop ID |
| `--template` | Hardcoded: `ralph-sandbox:latest` ([`specs/springfield.md:860`](specs/springfield.md)) |
| `--auto-push` | `true` unless `--no-push` passed to sgf |
| `--max-iterations` | Hardcoded: `30` ([`specs/springfield.md:859`](specs/springfield.md)) |
| `ITERATIONS` | Positional arg or default `30` |
| `PROMPT` | Assembled prompt file path from Phase 4 |

### Ralph invocation

Build `std::process::Command` for `ralph`:
- Locate ralph binary (check `PATH`, or use `SGF_RALPH_BINARY` env override for testing)
- Translate sgf flags → ralph flags per table above
- In AFK mode: pipe stdout for tee to log file
- In interactive mode: inherit all stdio

### Exit code handling

Per [`specs/springfield.md:293-302`](specs/springfield.md):

| Code | Meaning | sgf response |
|------|---------|--------------|
| `0` | Complete | Log success, clean up PID file |
| `1` | Error | Log error, clean up PID file, alert developer |
| `2` | Iterations exhausted | Clean up PID file, developer decides |
| `130` | Interrupted | Log interruption, clean up PID file |

### Signal forwarding

- Register SIGINT/SIGTERM handlers in sgf (via `signal-hook`)
- When interrupted, kill ralph child process and clean up PID file
- Exit with code 130

### Testing override

- Add `SGF_RALPH_BINARY` env var to override ralph binary path (similar to ralph's `RALPH_COMMAND` pattern)
- This enables integration testing without Docker by pointing to a mock script

### Verification

- Ralph is invoked with correct flags for each combination of sgf flags
- AFK mode tees output to log file
- Interactive mode passes through terminal
- Exit codes are handled correctly
- PID file is always cleaned up (normal exit, error, interrupt)
- `SGF_RALPH_BINARY` override works

---

## Phase 8: Workflow Commands

Wire all workflow commands through the orchestration core.

**Source**: [`specs/springfield.md:370-469`](specs/springfield.md) (Workflow Stages)

### Looped stages (use standard loop orchestration)

Each command: assemble prompt → ensure daemon → recovery → launch ralph → handle exit.

#### `sgf build <spec>` ([`specs/springfield.md:416-421`](specs/springfield.md))

- Requires spec stem positional arg
- Prompt template: `.sgf/prompts/build.md` with `{{spec}}` substitution
- Loop ID: `build-<spec>-<timestamp>`
- Supports `-a`, `--no-push`, `N`

#### `sgf test <spec>` ([`specs/springfield.md:447-451`](specs/springfield.md))

- Requires spec stem positional arg
- Prompt template: `.sgf/prompts/test.md` with `{{spec}}` substitution
- Loop ID: `test-<spec>-<timestamp>`
- Supports `-a`, `--no-push`, `N`

#### `sgf verify` ([`specs/springfield.md:426-435`](specs/springfield.md))

- No spec filter
- Prompt template: `.sgf/prompts/verify.md` (no variables)
- Loop ID: `verify-<timestamp>`
- Supports `-a`, `--no-push`, `N`

#### `sgf test-plan` ([`specs/springfield.md:437-445`](specs/springfield.md))

- No spec filter
- Prompt template: `.sgf/prompts/test-plan.md` (no variables)
- Loop ID: `test-plan-<timestamp>`
- Supports `-a`, `--no-push`, `N`

#### `sgf issues plan` ([`specs/springfield.md:463-469`](specs/springfield.md))

- No spec filter
- Prompt template: `.sgf/prompts/issues-plan.md` (no variables)
- Loop ID: `issues-plan-<timestamp>`
- Supports `-a`, `--no-push`, `N`

### Interactive stages (1 iteration, no AFK)

#### `sgf spec` ([`specs/springfield.md:396-414`](specs/springfield.md))

- Runs via ralph with 1 iteration in interactive mode
- Prompt template: `.sgf/prompts/spec.md` (no variables)
- Loop ID: `spec-<timestamp>`
- No `-a`, `--no-push`, or `N` flags
- No sentinel detection needed (1 iteration)

#### `sgf issues log` ([`specs/springfield.md:453-461`](specs/springfield.md))

- Runs via ralph with 1 iteration in interactive mode
- Prompt template: `.sgf/prompts/issues.md` (no variables)
- Loop ID: `issues-log-<timestamp>`
- No `-a`, `--no-push`, or `N` flags

### Utility commands

#### `sgf logs <loop-id>` ([`specs/springfield.md:329-332`](specs/springfield.md))

- Run `tail -f .sgf/logs/<loop-id>.log`
- Exit 1 if log file not found

#### `sgf status` ([`specs/springfield.md:28`](specs/springfield.md))

- Placeholder/stub — print "Not yet implemented" and exit 0
- Future work ([`specs/springfield.md:897`](specs/springfield.md))

### Verification

- Each command invokes ralph with the correct flags
- Spec-requiring commands fail gracefully without spec arg
- Interactive commands run with 1 iteration, no AFK
- `sgf logs` tails correct file
- `sgf status` prints placeholder message

---

## Phase 9: Docker Template Build

Implement `sgf template build` to build the `ralph-sandbox:latest` Docker image.

**Source**: [`specs/springfield.md:758-845`](specs/springfield.md) (Docker Sandbox Template)

### Dockerfile

Per [`specs/springfield.md:768`](specs/springfield.md):

- Source lives at `.docker/sandbox-templates/ralph/Dockerfile` in the Springfield repo
- Embedded in sgf binary at compile time via `include_str!`
- Create the file at `.docker/sandbox-templates/ralph/Dockerfile` with content from [`specs/springfield.md:771-832`](specs/springfield.md)

### `sgf template build` implementation

Per [`specs/springfield.md:838-845`](specs/springfield.md):

1. Locate the `pn` binary via `which pn`
2. Create a temporary build context directory
3. Write the embedded Dockerfile to the temp dir
4. Copy the `pn` binary into the build context
5. Run `docker build -t ralph-sandbox:latest .` in the temp dir
6. Clean up the temporary directory

### Verification

- `sgf template build` exits 0 when Docker is available and `pn` is on PATH
- Errors gracefully when `pn` not found
- Errors gracefully when Docker not available
- Built image name is `ralph-sandbox:latest`

### Note

- Docker must be available for E2E testing of this command
- If Docker is unavailable in the test environment, this test should be gated behind `#[ignore]` or a feature flag

---

## Phase 10: Documentation

### `crates/sgf/README.md` (new)

- Project overview and purpose
- Architecture summary (scaffolding, prompt assembly, loop orchestration, recovery, daemon lifecycle)
- Quick start / usage examples
- Command reference (brief, with link to spec for details)
- Relationship to ralph and pensa
- Testing instructions

### Root `README.md` update ([`README.md`](README.md))

- Add sgf to the architecture diagram
- Update component descriptions to reflect sgf's role
- Ensure all three crates are mentioned

### `AGENTS.md` update ([`AGENTS.md`](AGENTS.md))

- Add `cargo build -p sgf` and `cargo test -p sgf` examples
- Add sgf-specific notes if needed

### `specs/README.md` update ([`specs/README.md`](specs/README.md))

- Already lists `springfield.md` → `crates/springfield/` — update code path to `crates/sgf/` if needed (verify current listing)

### Verification

- All README files are accurate and reference correct paths
- Links work correctly
- AGENTS.md examples are up to date

---

## Phase 11: Integration Tests

End-to-end tests that verify sgf commands work correctly from the command line.

**Test infrastructure**: Following the patterns from pensa ([`crates/pensa/tests/integration.rs`](crates/pensa/tests/integration.rs)) and ralph ([`crates/ralph/tests/integration.rs`](crates/ralph/tests/integration.rs)).

### Test file

- `crates/sgf/tests/integration.rs`

### Test harness

- `setup_test_dir()` → `TempDir` with git init + initial commit (matching ralph's pattern)
- `sgf_cmd(dir)` → `Command` for sgf binary with `current_dir` set
- Mock ralph script: create a shell script that emits expected output and optionally creates `.ralph-complete`, pointed to via `SGF_RALPH_BINARY` env var
- For daemon tests: use `portpicker` for random port, `pn daemon` with temp project dir

### Test scenarios

#### `sgf init` tests

1. **`init_creates_all_files`** — run `sgf init` in empty dir, verify all directories and files exist:
   - `.pensa/`, `.sgf/`, `.sgf/logs/`, `.sgf/run/`, `.sgf/prompts/`, `.sgf/prompts/.assembled/`
   - `.sgf/backpressure.md`, all prompt templates, `memento.md`, `CLAUDE.md`, `specs/README.md`
   - `.claude/settings.json`, `.pre-commit-config.yaml`, `.gitignore`

2. **`init_file_contents`** — verify critical file contents match spec:
   - `CLAUDE.md` contains `Read memento.md and AGENTS.md`
   - `memento.md` contains `## Stack` and `## References`
   - `.claude/settings.json` contains all 4 deny rules
   - `.gitignore` contains Springfield entries

3. **`init_idempotent`** — run `sgf init` twice, verify:
   - No duplicate lines in `.gitignore`
   - No duplicate deny rules in `.claude/settings.json`
   - No duplicate hooks in `.pre-commit-config.yaml`
   - Prompt templates not overwritten (add marker to first run, verify it persists)

4. **`init_merges_existing_gitignore`** — create `.gitignore` with custom entries, run `sgf init`, verify custom entries preserved and sgf entries added

5. **`init_merges_existing_settings_json`** — create `.claude/settings.json` with custom deny rules, run `sgf init`, verify custom rules preserved and sgf rules added

#### Prompt assembly tests

6. **`prompt_assembly_substitutes_spec`** — create `.sgf/prompts/build.md` with `{{spec}}`, run assembly, verify output has spec value substituted

7. **`prompt_assembly_validates_unresolved`** — create template with `{{unknown}}`, verify assembly fails with descriptive error

8. **`prompt_assembly_passthrough`** — create template without variables, verify assembled output matches input exactly

#### Loop orchestration tests (mocked ralph)

9. **`build_invokes_ralph_with_correct_flags`** — run `sgf build auth -a` with mock ralph, verify mock ralph received correct flags (`--afk`, `--loop-id`, `--template ralph-sandbox:latest`, `--auto-push true`, `--max-iterations 30`)

10. **`build_creates_pid_file`** — run `sgf build auth` with mock ralph, verify `.sgf/run/<loop-id>.pid` exists during execution

11. **`build_cleans_pid_file_on_exit`** — run `sgf build auth` with mock ralph that exits 0, verify PID file removed

12. **`afk_tees_output_to_log`** — run `sgf build auth -a` with mock ralph that prints output, verify `.sgf/logs/<loop-id>.log` contains output

13. **`spec_runs_one_interactive_iteration`** — run `sgf spec` with mock ralph, verify ralph invoked with `1` iteration and no `--afk` flag

14. **`issues_log_runs_one_interactive_iteration`** — similar to above for `sgf issues log`

#### Recovery tests

15. **`recovery_cleans_stale_state`** — create stale PID file (with dead PID), dirty git state, run `sgf build auth` with mock ralph, verify recovery ran (git state clean before ralph starts)

16. **`recovery_skips_when_live_pid`** — create PID file with own PID (alive), run `sgf build auth`, verify recovery did NOT run

#### Utility tests

17. **`logs_tails_existing_file`** — create `.sgf/logs/test-loop.log`, run `sgf logs test-loop` with timeout, verify exit 0

18. **`logs_exits_1_for_missing`** — run `sgf logs nonexistent`, verify exit 1

19. **`status_prints_placeholder`** — run `sgf status`, verify it outputs something and exits 0

20. **`help_flag`** — run `sgf --help`, verify output contains command listing

#### Docker template test (gated)

21. **`template_build_requires_pn`** — run `sgf template build` without `pn` on PATH, verify descriptive error (gate behind `#[ignore]` if Docker unavailable)

### Additional tools required

- **`SGF_RALPH_BINARY` env var** in sgf binary — overrides `ralph` binary path for testing (required for mock ralph pattern)
- Tests run with `cargo test -p sgf` — no Docker required except for `#[ignore]`-gated tests
- All tests should be parallelizable (unique temp dirs, no shared state)

### Verification

- `cargo test -p sgf` — all non-ignored tests pass
- `cargo test --workspace` — all tests across all crates pass
- `cargo clippy --workspace -- -D warnings` — no warnings
- `cargo fmt --all --check` — formatting clean
