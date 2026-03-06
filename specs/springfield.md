# springfield Specification

CLI entry point for Springfield. All developer interaction goes through this binary. It handles project scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle. Delegates iteration execution to ralph and persistent memory to pensa.

## Overview

`sgf` provides:
- **Project scaffolding**: `sgf init` creates the full project structure (`.sgf/`, `.pensa/`, prompts, backpressure, specs index, Claude deny settings, git hooks)
- **Prompt delivery**: Validate prompt templates exist, pass raw paths to ralph or `$AGENT_CMD`, set `SGF_SPEC` env var
- **Loop orchestration**: Launch ralph with the correct flags, manage PID files, tee logs
- **Recovery**: Pre-launch cleanup of dirty state from crashed iterations
- **Daemon lifecycle**: Start the pensa daemon before launching loops
- **Workflow commands**: `spec`, `build`, `verify`, `test-plan`, `test`, `issues log`

---

## CLI Commands

```
sgf init                               — scaffold a new project
sgf spec                               — generate specs and implementation plan (interactive)
sgf build [spec] [-a] [--no-push] [N]  — run build loop
sgf verify [-a] [--no-push] [N]        — run verification loop
sgf test-plan [-a] [--no-push] [N]     — run test plan generation loop
sgf test [spec] [-a] [--no-push] [N]   — run test execution loop
sgf issues log                         — interactive session for logging bugs
sgf status                             — show project state (future work)
sgf logs <loop-id>                     — tail a running loop's output
sgf template build                     — rebuild Docker sandbox template
```

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | `false` | AFK mode: NDJSON stream parsing with formatted output |
| `--no-push` | `false` | Disable auto-push after commits |
| `N` (positional) | `30` | Number of iterations |

---

## sgf init

Scaffolds a new project. Accepts `--force` to overwrite template and skeleton files with built-in defaults.

### What it creates

```
.pensa/                                (directory only — daemon creates db.sqlite on start)
.sgf/
├── loom-specs-README.md               (reference format for specs/README.md)
├── logs/                              (empty, gitignored)
├── run/                               (empty, gitignored)
└── prompts/
    ├── build.md                       (prompt template — passed directly, no assembly)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    └── issues.md
BACKPRESSURE.md                        (build/test/lint/format commands for the project)
.claude/settings.json                  (deny rules for .sgf/**)
.pre-commit-config.yaml                (prek hooks for pensa sync)
.gitignore                             (Springfield entries + stack-specific entries)
CLAUDE.md                              (`ln -s` to AGENTS.md)
specs/
└── README.md                          (empty spec index)
```

### Claude deny settings

`sgf init` creates or updates `.claude/settings.json` with deny rules protecting `.sgf/` from agent modification:

```json
{
  "permissions": {
    "deny": [
      "Edit .sgf/**",
      "Write .sgf/**",
      "Bash rm .sgf/**",
      "Bash mv .sgf/**"
    ]
  }
}
```

If `.claude/settings.json` already exists, `sgf init` merges deny rules into the existing `permissions.deny` array without duplicating entries or removing existing rules.

### Prek hooks

[prek](https://github.com/j178/prek) is a Rust-based git hook manager that reads `.pre-commit-config.yaml`. It replaces the Python-based [pre-commit](https://pre-commit.com/) — same config format, no Python dependency. `sgf init` generates the config and runs `prek install` to wire the hooks into `.git/hooks/`.

`sgf init` creates `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: pensa export
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: pensa import
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
```

If `.pre-commit-config.yaml` already exists, `sgf init` appends the pensa hooks without duplicating them.

### Gitignore

`sgf init` creates `.gitignore` or appends entries to an existing one. Entries are added idempotently — existing lines are not duplicated.

#### Entries added

```gitignore
# Springfield
.pensa/db.sqlite
**/.pensa/daemon.port
**/.pensa/daemon.url
.sgf/logs/
.sgf/run/
.ralph-complete
.ralph-ding

# Rust
/target

# Node
node_modules/

# SvelteKit
.svelte-kit/

# Environment
.env
.env.local
.env.*.local

# macOS
.DS_Store
```

All entries are always added regardless of what exists in the directory. If an entry already exists anywhere in the file, it is not added again.

### CLAUDE.md

`ln -s` to AGENTS.md.

### specs/README.md

```markdown
# Specifications

| Spec | Code | Purpose |
|------|------|---------|
```

### Idempotence

`sgf init` is safe to re-run. It skips files that already exist (prompts, CLAUDE.md, specs/README.md) and only merges additive content (deny rules, git hooks, gitignore entries). It never overwrites existing content. `prek install` is always run to ensure hooks are wired into `.git/hooks/`.

### --force

`sgf init --force` overwrites all template and skeleton files with built-in defaults, **except `specs/README.md`** which is never overwritten. Use this to pick up upstream template changes after updating the `sgf` binary.

Safety checks:
- Fails if any target file has uncommitted changes or is untracked by git.
- Lists files to be overwritten and requires `y` confirmation before proceeding.

Config merges (`.gitignore`, `.claude/settings.json`, `.pre-commit-config.yaml`) are unaffected by `--force` — they always use additive merge logic.

---

## Per-Repo Project Structure

After `sgf init` and ongoing development, a project contains:

```
.pensa/
├── db.sqlite                  (gitignored — daemon-owned working database)
├── issues.jsonl               (committed — git-portable export)
├── deps.jsonl                 (committed)
└── comments.jsonl             (committed)
.sgf/
├── loom-specs-README.md       (committed — reference format for specs/README.md)
├── logs/                      (gitignored — AFK loop output)
│   └── <loop-id>.log
├── run/                       (gitignored — PID files for running loops)
│   └── <loop-id>.pid
└── prompts/
    ├── build.md               (committed — editable prompt templates, passed directly to ralph/$AGENT_CMD)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    └── issues.md
BACKPRESSURE.md                (committed — build/test/lint/format commands)
.pre-commit-config.yaml        (prek hooks for pensa sync)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (`ln -s` to AGENTS.md)
test-report.md                 (generated — overwritten each test run, committed)
verification-report.md         (generated — overwritten each verify run, committed)
specs/
├── README.md                  (agent-maintained spec index — loom-style tables)
└── *.md                       (prose specification files)
```

### File Purposes

**`specs/README.md`** — Agent-maintained spec index, matching the loom format (reference copy at `.sgf/loom-specs-README.md`). Categorized tables with `| Spec | Code | Purpose |` columns mapping each spec to its implementation location and a one-line summary. Agents update this file when adding or revising specs.

**`BACKPRESSURE.md`** — Build, test, lint, and format commands for the project. Generated by `sgf init` at the project root from a universal template (see [Backpressure Template](#backpressure-template)). Developer-editable after scaffolding. Lives at the project root (not inside `.sgf/`) so it is discoverable by ralph's `study` instruction and `$AGENT_CMD` wrappers. Not protected by Claude deny settings — developers and agents can edit it freely.

**`.sgf/loom-specs-README.md`** — Reference example showing how to format `specs/README.md`. Demonstrates categorized tables with `| Spec | Code | Purpose |` columns. Agents read this to learn the expected index format. Protected by Claude deny settings.

**`AGENTS.md`** — Hand-authored operational guidance. Contains code style preferences, runtime notes, and special instructions. Not generated by `sgf init` — the developer creates this when needed.

**`CLAUDE.md`** — Entry point for Claude Code. Symlinks to AGENTS.md. Auto-loaded by Claude Code at the start of every session.

**`.sgf/prompts/`** — Editable prompt templates for each workflow stage. Seeded by `sgf init` from Springfield's built-in templates. Once seeded, the project owns these files — edit them to evolve the prompts. To improve defaults for future projects, update the templates in the Springfield repo.

**`.sgf/` protection** — The entire `.sgf/` directory is protected from agent modification via Claude deny settings. `sgf init` scaffolds these rules. This is enforced at the framework level — agents cannot write to prompts or reference files regardless of prompt instructions. `BACKPRESSURE.md` is intentionally outside `.sgf/` and not protected — it is developer-authored content that agents may need to reference or suggest edits to.

**`PROMPT_FILES`** (env var) — Colon-separated list of files to include in `study` instructions. Default: `$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md`. Ralph reads this variable for automated stages and passes it as `--append-system-prompt 'study @<file>;...'`; `$AGENT_CMD` wrappers read it for interactive stages. sgf does not read `PROMPT_FILES`. `~` is expanded to `$HOME`; `./` paths are resolved relative to the project root. Missing files emit a warning to stderr. If `PROMPT_FILES` is not set, a warning is emitted and the default is used.

**`AGENT_CMD`** (env var) — Command used for interactive Claude sessions. Default: `claude`. Interactive stages call `$AGENT_CMD` directly with `--verbose @{prompt_path}`, inheriting stdio. The wrapper is responsible for system prompt injection (e.g., reading `PROMPT_FILES` and passing `--append-system-prompt` with `study` instructions). sgf does not inject system files for interactive stages.

**`SGF_SPEC`** (env var) — Spec stem for build/test stages. Set by sgf in ralph's environment (e.g., `SGF_SPEC=auth`). Ralph includes `./specs/${SGF_SPEC}.md` in its `study` instruction. Prompt templates reference this env var instead of `{{spec}}` template variables.

**`specs/`** — Prose specification files (one per topic of concern). Authored during the spec phase, consumed during builds. Indexed in `specs/README.md`.

---

## Prompt Delivery

sgf does not assemble, transform, or preprocess prompt templates. Templates in `.sgf/prompts/` are final prompts passed directly to ralph or `$AGENT_CMD`.

### What sgf Does

1. **Validate** — confirm `.sgf/prompts/<stage>.md` exists. Fail with a clear error if not found.
2. **Validate spec** — for stages given a spec (`build [spec]`, `test [spec]`), confirm `specs/<spec>.md` exists. Fail with a clear error (e.g., `spec not found: specs/auth.md`) if the file is missing. Skip this step when no spec is provided.
3. **Set environment** — when a spec is provided, set `SGF_SPEC=<stem>` in ralph's environment. When no spec is given, neither `SGF_SPEC` nor `--spec` are set.
4. **Pass the raw path** — give ralph or `$AGENT_CMD` the template path directly (no intermediate files).

### System Prompt Injection

**Automated stages (ralph):** Ralph owns system prompt injection. It reads `PROMPT_FILES`, resolves paths, builds a semicolon-separated `study @<file>` instruction, and passes it as `--append-system-prompt` to the Claude Code invocation inside the Docker sandbox. This ensures the agent actively reads and processes the files rather than receiving them as passive system context. When `SGF_SPEC` is set, ralph also includes `./specs/${SGF_SPEC}.md` in the study instruction.

**Interactive stages (`$AGENT_CMD`):** The user-configured `$AGENT_CMD` wrapper handles system prompt injection. sgf does not inject system files for interactive stages. Users must configure their wrapper to read `PROMPT_FILES` and pass `--append-system-prompt` with `study` instructions (or equivalent).

### Prompt Templates

Templates are plain markdown files with no variable substitution. The spec name is available to the agent via:
- The `SGF_SPEC` environment variable (readable by Claude Code)
- The spec file content actively read by the agent via ralph's `study` instruction

For example, `build.md` uses `$SGF_SPEC` directly:
```
Run `pn ready --spec $SGF_SPEC --json`.
```

---

## sgf-to-ralph Contract

### Invocation

```
[SGF_SPEC=<stem>] sgf → ralph [-a] [--loop-id ID] [--template T] [--auto-push BOOL] [--max-iterations N] [--spec STEM] ITERATIONS PROMPT
```

`sgf` translates its own flags and hardcoded defaults into ralph CLI flags. Ralph does not read config files — all configuration arrives via flags and environment variables.

### CLI Flags Passed to Ralph

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `-a` / `--afk` | bool | sgf command (e.g., `sgf build -a`) | AFK mode |
| `--loop-id` | string | sgf-generated | Unique loop identifier |
| `--template` | string | hardcoded: `ralph-sandbox:latest` | Docker sandbox template |
| `--auto-push` | bool | `true` unless `--no-push` passed to sgf | Auto-push after commits |
| `--max-iterations` | u32 | hardcoded: `30` | Safety limit |
| `--spec` | string | spec positional arg from sgf (optional) | Spec stem — ralph includes `./specs/<stem>.md` in its study instruction. Omitted when no spec is given. |
| `ITERATIONS` | u32 | positional arg or default `30` | Number of iterations |
| `PROMPT` | path | `.sgf/prompts/<stage>.md` | Raw prompt template file |

### Environment Variables Passed to Ralph

| Variable | Source | Description |
|----------|--------|-------------|
| `SGF_SPEC` | sgf | Spec stem (e.g., `auth`). Set only when a spec is provided to `build` or `test`. Not set when no spec is given. |

### Execution Model by Stage

| Stage | Execution | Description |
|-------|-----------|-------------|
| `spec` | `$AGENT_CMD` directly | Interactive interview; calls `$AGENT_CMD --verbose @{prompt_path}`, inheriting stdio |
| `build` | ralph (Docker) | Autonomous execution; sandbox provides isolation |
| `verify` | ralph (Docker) | Autonomous execution |
| `test-plan` | ralph (Docker) | Autonomous execution |
| `test` | ralph (Docker) | Autonomous execution |
| `issues log` | `$AGENT_CMD` directly | Interactive interview; calls `$AGENT_CMD --verbose @{prompt_path}`, inheriting stdio |

Interactive stages (`spec`, `issues log`) call `$AGENT_CMD` directly. No PID file, no log tee, no loop ID. The wrapper (configured by the user) handles system prompt injection via `PROMPT_FILES`. Automated stages (`build`, `verify`, `test-plan`, `test`) go through ralph, which handles system prompt injection via `--append-system-prompt` with `study` instructions and always runs in Docker sandboxes.

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|--------------|
| `0` | Sentinel found (`.ralph-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, stop sandbox, clean up |

On interrupt, sgf's behavior depends on the stage type:

**AFK loops** (`-a` flag): sgf spawns ralph in its own process group (`setsid()` via `pre_exec`) so that terminal SIGINT is delivered only to sgf, not to ralph. sgf independently implements double Ctrl+C semantics. SIGINT increments an `AtomicUsize` counter (`sigint_count`). First press: print "sgf: press Ctrl+C again to stop" and start a 2-second confirmation window. Second press within 2 seconds: send SIGTERM to the ralph child process, wait for it to exit, then run `docker sandbox stop claude`. If no second press arrives within 2 seconds, sgf resets its counter to 0. SIGTERM always triggers immediate shutdown (single signal). Signal handlers are registered just before spawning the ralph child — during pre-launch checks, daemon startup, and other phases before handler registration, default signal behavior applies (single SIGINT exits).

**Non-AFK loops and interactive stages**: A single SIGINT triggers immediate shutdown — send SIGTERM to the child process, wait for exit, stop sandbox, clean up.

This is belt-and-suspenders with ralph's own sandbox cleanup — even if ralph is hard-killed, sgf ensures no orphaned containers remain.

Claude Code crashes and push failures are handled within ralph as warnings — they do not produce distinct exit codes. Ralph logs the failure and continues to the next iteration without cleanup. The next iteration's agent inherits whatever state exists and proceeds via forward correction. Stale claims and dirty working trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery before the next run.

### Completion Sentinel

The agent creates a `.ralph-complete` file when `pn ready` returns no tasks. Ralph checks for this file after each iteration. If found, ralph deletes it, performs a final auto-push (if enabled), and exits with code `0`.

---

## Loop ID Format

`sgf` generates loop IDs with the pattern: `<stage>[-<spec>]-<YYYYMMDDTHHmmss>`

Examples:
- `build-auth-20260226T143000` (build loop for auth spec)
- `verify-20260226T150000` (verify loop, no spec filter)
- `issues-plan-20260226T160000` (issues plan loop)

Ralph includes the loop ID in log output. `sgf logs` uses the loop ID to locate log files.

---

## Logging

`sgf` tees ralph's stdout to both the terminal and `.sgf/logs/<loop-id>.log`. Ralph owns formatting — in AFK mode it emits human-readable one-liners (tool calls, text blocks); in interactive mode it passes through the terminal. `sgf` does not parse ralph's output.

The `.sgf/logs/` directory is gitignored.

### sgf logs

`sgf logs <loop-id>` runs `tail -f .sgf/logs/<loop-id>.log`. If the log file does not exist, print an error and exit 1.

---

## Recovery

Ralph does not perform iteration-start cleanup. Recovery is `sgf`'s responsibility, executed before launching ralph.

### PID Files

`sgf` writes `.sgf/run/<loop-id>.pid` on launch (containing its process ID) and removes it on clean exit. The `.sgf/run/` directory is gitignored.

### Pre-launch Cleanup

Before launching ralph, `sgf` scans all PID files in `.sgf/run/`:

- **Any PID alive** (verified via `kill -0`) → another loop is running. Skip cleanup and launch normally — the dirty tree or in-progress claims may belong to that loop.
- **All PIDs stale** (process dead) → no loops are running. Remove stale PID files, then recover:
  1. `git checkout -- .` — discard modifications to tracked files. **Failure is fatal** — loop launch is aborted.
  2. `git clean -fd` — remove untracked files (respects `.gitignore`, so `db.sqlite` and logs are safe). **Failure is fatal** — loop launch is aborted.
  3. `pn doctor --fix` — release stale claims and repair integrity (warning only — supplementary, not critical for state consistency)

**Principle**: Work is only preserved when committed. Uncommitted changes from crashed iterations are discarded — the agent that produced them is gone and cannot continue them. Git recovery failures are hard errors that prevent loop launch — proceeding with dirty state would violate the atomic iteration guarantee.

---

## Pre-launch Lifecycle

Before launching any loop, `sgf` runs pre-launch checks. The checks vary by stage:

**Sandboxed stages** (build, verify, test-plan, test):

1. **Recovery** — clean up stale state from crashed iterations (see Recovery)
2. **Daemon** — start the pensa daemon if not already running
3. **Template** — ensure the Docker sandbox template exists and is current
4. **Pensa export** — run `pn export` to sync SQLite → JSONL before the sandbox launches (the sandbox daemon auto-imports from JSONL on first start)

**Interactive stages** (spec, issues log):

1. **Recovery** — clean up stale state from crashed iterations (see Recovery)
2. **Daemon** — start the pensa daemon if not already running

Template pre-flight is skipped for interactive stages — no Docker image is needed. After pre-launch checks, `sgf` calls `$AGENT_CMD` directly with `--verbose @{prompt_path}`, inheriting stdio.

**`SGF_SKIP_PREFLIGHT`** (env var) — When set, skips daemon startup and template pre-flight while still running recovery. This allows two-tier control: the `--skip-preflight` CLI flag disables all pre-launch checks (including recovery), while `SGF_SKIP_PREFLIGHT` disables only the infrastructure checks (daemon + template). Used by integration tests to exercise recovery logic without requiring Docker or a running pensa daemon.

### Daemon

`sgf` starts the pensa daemon automatically before launching any loop (if not already running):

1. Check if the daemon is reachable (`pn daemon status`)
2. If not, start it: `pn daemon --project-dir <project-root> --port <derived> &` (backgrounded)
3. Wait for readiness (poll `pn daemon status` with exponential backoff: 50ms initial, doubling up to 800ms cap, 5s total deadline)
4. Proceed with loop launch

The daemon runs for the duration of the `sgf` session. It stops on SIGTERM or when `sgf` shuts down.

### Template Pre-flight

`sgf` checks the Docker sandbox template before launching any loop:

1. Resolve Docker context: use `SGF_DOCKER_CONTEXT` env var if set, otherwise capture current context via `docker context show`. Pass `--context <resolved>` on all subsequent docker commands.
2. Run `docker --context <CONTEXT> image inspect ralph-sandbox:latest` to check if the image exists
3. If the image does not exist, auto-build it (runs `sgf template build` logic internally). Print a heads-up before building — the first build takes several minutes.
4. If the image exists, check staleness by comparing Docker image labels against current values:
   - `pn_hash` — SHA-256 of the pensa crate source (Cargo.toml + src/*.rs)
   - `dockerfile_hash` — SHA-256 of the embedded Dockerfile content
5. If stale, auto-rebuild it (same as step 3). Print the reason (e.g., "rebuilding ralph-sandbox:latest (pensa source has changed)...").
6. If fresh, proceed silently.

Auto-build failure is a hard error — the loop cannot proceed without a template.

---

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready --spec <stem>` returns nothing (all tasks for a spec are done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., multiple `sgf build` instances) can run concurrently on the same branch. The pensa daemon serializes all database access, providing atomic claims via `pn update --claim` (fails with `already_claimed` if another agent got there first). `pn export` runs at commit time via the pre-commit hook. Concurrent sandboxes share the same git history via Mutagen file sync — push conflicts don't arise because each sandbox sees the other's commits within seconds. **Stop build loops before running `sgf spec`** to avoid task-supersession race conditions.

### Standard Loop Iteration

Build, Test, and Issues Plan stages share a common iteration pattern. Each iteration:

1. **Orient** — system files from `PROMPT_FILES` are actively read by the agent (via ralph's `study` instruction for automated stages, or by the wrapper for interactive stages).
2. **Query** — find work items via pensa (stage-specific query). If none, write `.ralph-complete` and exit.
3. **Choose & Claim** — pick a task from the results, then `pn update <id> --claim`. If the claim fails (`already_claimed`), re-query and pick another.
4. **Work** — stage-specific implementation
5. **Log issues** — if problems are discovered: `pn create "description" -t bug`
6. **Close/release** — close or release the work item
7. **Commit** — prefix the commit message with `[<task-id>]` (e.g., `[pn-a1b2c3d4] Implement login validation`). The pre-commit hook runs `pn export` automatically, syncing SQLite to JSONL. The prefix enables `git log --grep` for per-task history.

Each iteration gets fresh context. The pensa database persists state between iterations.

| Stage | Query | Work | Close |
|-------|-------|------|-------|
| Build | `pn ready [--spec <stem>] --json` | Implement the task (or plan the bug — see below); apply backpressure | `pn close <id> --reason "..."` (tasks) / `pn release <id>` (bugs) |
| Test | `pn ready -t test [--spec <stem>] --json` | Execute the test | `pn close <id> --reason "..."` |

#### Bug Planning in the Build Loop

`pn ready` now includes unplanned bugs (see pensa spec). When the build loop claims an item and it is a bug, the iteration becomes a **planning** iteration:

1. Study the codebase to understand the bug
2. Create fix task(s): `pn create -t task "fix: <description>" --fixes <bug-id> [--spec <stem>] [-p <priority>] [--dep <id>]`
3. Comment lessons learned on the bug: `pn comment add <bug-id> "..."`
4. Release the bug: `pn release <bug-id>` (the bug drops out of `pn ready` — it now has fix children)
5. Commit with `[<bug-id>]` prefix

The fix tasks appear in subsequent `pn ready` calls and are implemented as normal tasks. When all fix tasks for a bug are closed, pensa auto-closes the bug.

### 1. Spec (`sgf spec`)

Opens an interactive Claude Code session with the spec prompt. Calls `$AGENT_CMD` directly (no ralph, no Docker). The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates deliverables:

1. Write spec files to `specs/`
2. Update `specs/README.md` with new index entries (loom-style `| Spec | Code | Purpose |` rows)
3. Create implementation plan items via `pn create -t task --spec <stem>`, with dependencies and priorities
4. Commit and push

The interview and generation happen in a single session. The agent asks clarifying questions as needed, but the goal is always to produce specs and a plan. The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line.

Tasks linked to a spec *are* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Spec revision**: Run `sgf spec` again. **Stop any running build loops before revising specs.** When revising, the agent:
1. Reviews existing tasks for the spec: `pn list --spec <stem> --json`
2. Closes tasks that are no longer relevant: `pn close <id> --reason "superseded by revised spec"`
3. Creates new tasks for the delta: `pn create "..." -t task --spec <stem>`
4. Updates the spec file in `specs/`
5. Restart build loops after revision is committed

### 2. Build (`sgf build [spec]`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/build.md`. The spec stem is **optional** — `sgf build auth` builds tasks for the `auth` spec, while `sgf build` runs without a spec filter. When a spec is provided, sgf validates that `specs/auth.md` exists before launching (fails with a clear error if not found).

When a spec is given, sgf sets `SGF_SPEC=auth` in ralph's environment and passes `--spec auth` to ralph. Ralph includes `specs/auth.md` in its `study` instruction so the agent actively reads the full spec. When no spec is given, neither `SGF_SPEC` nor `--spec` are set. The build stage adds **backpressure** — after implementing the task, the agent runs build, test, and lint commands per `BACKPRESSURE.md`.

Run interactively first for a few supervised rounds, then switch to AFK mode (`-a`) for autonomous execution.

### 3. Verify (`sgf verify`)

Runs via ralph using `.sgf/prompts/verify.md`. Each iteration handles one spec:

1. Read the spec index from `specs/README.md`
2. Pick one unverified spec and investigate it against the codebase
3. Mark conformance: ✅ Matches spec, ⚠️ Partial match, ❌ Missing/different
4. Update `verification-report.md`
5. Log any gaps as pensa bugs: `pn create "..." -t bug`
6. Commit

When all specs have been verified, write `.ralph-complete`.

### 4. Test Plan (`sgf test-plan`)

Runs via ralph using `.sgf/prompts/test-plan.md`. The agent:

1. Studies specs and codebase
2. Generates a testing plan
3. Ensures tests are automatable (can be run by agents in loops)
4. Creates test items via `pn create -t test --spec <stem>`, with dependencies and priorities
5. Commits

### 5. Test (`sgf test [spec]`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/test.md`. The spec stem is **optional** — `sgf test auth` runs test items for the `auth` spec, while `sgf test` runs all test items regardless of spec. When a spec is provided, sgf validates that `specs/auth.md` exists before launching. Sets `SGF_SPEC` and `--spec` only when a spec is given.

After all test items are closed, a final iteration generates `test-report.md` — a summary of all test results, pass/fail status, and any bugs logged.

### 6. Issues Log (`sgf issues log`)

Calls `$AGENT_CMD` directly (no ralph, no Docker) using `.sgf/prompts/issues.md`. Each session handles one bug:

1. The developer describes a bug they've observed
2. The agent interviews them to capture details — steps to reproduce, expected vs actual behavior, relevant context
3. Logs the bug via `pn create -t bug`

One bug per session. The developer runs `sgf issues log` again for additional bugs — fresh context each time prevents accumulation across unrelated issues.

### 7. Inline Issue Logging

Issues are also logged by agents during any stage via `pn create`. The build loop logs bugs it discovers during implementation. The verify loop logs spec gaps. The test loop logs test failures. `sgf issues log` is for developer-reported bugs; inline logging is for agent-discovered bugs.

---

## Prompt Templates

Each workflow stage has a corresponding prompt template in `.sgf/prompts/`. These are the default contents that `sgf init` writes. The project owns these files after scaffolding — edit them freely.

| Template | Purpose |
|----------|---------|
| `spec.md` | Interactive spec discussion and implementation planning |
| `build.md` | Claim one pn issue, implement it, apply backpressure, commit |
| `verify.md` | Verify one spec against codebase, update verification report |
| `test-plan.md` | Generate test items from specs using pn |
| `test.md` | Claim one pn test item, execute it, apply backpressure |
| `issues.md` | Interactive bug reporting session |

The canonical prompt templates live in `.sgf/prompts/` — do not duplicate their contents here. See `crates/springfield/src/init.rs` for the default template text that `sgf init` writes.

---

## Backpressure Template

The following is the full content of `BACKPRESSURE.md` that `sgf init` writes to the project root. The developer deletes sections that don't apply to their project and edits commands as needed.

````markdown
# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning

This document defines backpressure for a variety of project types. Be sure to align your understanding of backpressure to the project type with which you're currently working.

## Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p <crate>` (e.g., `cargo build -p my-crate`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p <crate> <test_name>` (e.g., `cargo test -p my-crate test_login`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Detect unsafe code usage:** `cargo geiger`

### Long Running Tests

Some tests may be gated behind `#[ignore]` because they use expensive operations. These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

## Frontend

> Stack: TypeScript, Svelte 5, SvelteKit, Vitest, @testing-library/svelte, Playwright
>
> **Working directory:** adjust as needed (some projects may have frontend commands run from the frontend directory)

- **Build:** `pnpm run build`
- **Unit tests:** `pnpm run vitest run`
- **Unit tests (watch):** `pnpm run vitest`
- **Unit test single file:** `pnpm run vitest run <path>` (e.g., `pnpm run vitest run src/lib/components/Auth/LoginScreen.test.ts`)
- **Type check:** `pnpm run tsc --noEmit`
- **Svelte check:** `pnpm run svelte-check --tsconfig ./tsconfig.json`
- **Lint:** `pnpm run lint`
- **Lint fix:** `pnpm run lint:fix`
- **Format:** `pnpm run format`
- **Format check:** `pnpm run format:check`
- **Full check:** `pnpm run check`

### E2E Tests (Playwright)

- **E2E tests:** `pnpm run test:e2e`

### E2E Tests (Tauri, Linux Only)

E2E tests run on **Linux only** using WebdriverIO + WebKitWebDriver. macOS is not supported for E2E testing (no WebDriver access to WKWebView).

## Tauri

- **Build Tauri app:** `pnpm run tauri build`
- **Build Tauri app (debug):** `pnpm run tauri build --debug`
````

---

## Loom Specs README Template

The following is the full content of `.sgf/loom-specs-README.md` that `sgf init` writes. It serves as a reference example for how agents should format `specs/README.md` — categorized tables with `| Spec | Code | Purpose |` columns.

```markdown
# Loom Specifications

Design documentation for Loom, an AI-powered coding agent in Rust.

## Core Architecture

| Spec | Code | Purpose |
|------|------|---------|
| [architecture.md](./architecture.md) | [crates/](../crates/) | Crate structure, server-side LLM proxy design |
| [state-machine.md](./state-machine.md) | [loom-core](../crates/loom-core/) | Agent state machine for conversation flow |
| [tool-system.md](./tool-system.md) | [loom-tools](../crates/loom-tools/) | Tool registry and execution framework |
| [thread-system.md](./thread-system.md) | [loom-thread](../crates/loom-thread/) | Thread persistence and sync |
| [streaming.md](./streaming.md) | [loom-llm-service](../crates/loom-llm-service/) | SSE streaming for real-time LLM responses |
| [error-handling.md](./error-handling.md) | [loom-core](../crates/loom-core/) | Error types using `thiserror` |

## Observability Suite

Loom's integrated observability platform: analytics, crash tracking, cron monitoring, and session health.

| Spec | Code | Purpose |
|------|------|---------|
| [analytics-system.md](./analytics-system.md) | [loom-analytics-core](../crates/loom-analytics-core/), [loom-analytics](../crates/loom-analytics/), [loom-server-analytics](../crates/loom-server-analytics/) | Product analytics with PostHog-style identity resolution |
| [crash-system.md](./crash-system.md) | [loom-crash-core](../crates/loom-crash-core/), [loom-crash](../crates/loom-crash/), [loom-crash-symbolicate](../crates/loom-crash-symbolicate/), [loom-server-crash](../crates/loom-server-crash/) | Crash analytics with source maps, regression detection |
| [sessions-system.md](./sessions-system.md) | [loom-sessions-core](../crates/loom-sessions-core/), [loom-server-sessions](../crates/loom-server-sessions/) | Session analytics with release health and crash-free rate |

## LLM Integration

| Spec | Code | Purpose |
|------|------|---------|
| [llm-client.md](./llm-client.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/), [loom-llm-openai](../crates/loom-llm-openai/), [loom-server-llm-zai](../crates/loom-server-llm-zai/) | `LlmClient` trait for providers |
| [anthropic-oauth-pool.md](./anthropic-oauth-pool.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/) | Claude subscription pooling with failover |
| [claude-subscription-auth.md](./claude-subscription-auth.md) | [loom-llm-anthropic](../crates/loom-llm-anthropic/) | OAuth 2.0 PKCE for Claude Pro/Max |
```

---

## Docker Sandbox Template

All loops run inside Docker Desktop sandboxes. A single universal template is used for all project types.

### Template Name

`ralph-sandbox:latest`

### Dockerfile

The Dockerfile source lives in the Springfield repo at `.docker/sandbox-templates/ralph/Dockerfile`:

```dockerfile
FROM docker/sandbox-templates:claude-code

USER root

# System dependencies for Tauri development and general builds
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libwebkit2gtk-4.1-dev \
    webkit2gtk-driver \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    curl \
    wget \
    file \
    libxdo-dev \
    # Playwright browser dependencies (--with-deps fails on this Ubuntu version)
    libnss3 \
    libnspr4 \
    libatk1.0-0t64 \
    libatk-bridge2.0-0t64 \
    libcups2t64 \
    libdrm2 \
    libxcomposite1 \
    libxdamage1 \
    libxfixes3 \
    libxrandr2 \
    libgbm1 \
    libpango-1.0-0 \
    libcairo2 \
    libasound2t64 \
    libxshmfence1 \
    libglib2.0-0t64 \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
USER agent
ENV RUSTUP_HOME=/home/agent/.rustup
ENV CARGO_HOME=/home/agent/.cargo
ENV PATH="/home/agent/.cargo/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . "$CARGO_HOME/env" \
    && rustup default stable \
    && rustup component add rustfmt clippy

# Install Tauri CLI, cargo-geiger (unsafe code auditing), and prek (git hook manager)
RUN . "$CARGO_HOME/env" && cargo install tauri-cli cargo-geiger prek

# Enable pnpm
USER root
RUN corepack enable && corepack prepare pnpm@latest --activate

# Install global JS tools
USER agent
ENV PNPM_HOME="/home/agent/.local/share/pnpm"
ENV PATH="${PNPM_HOME}:${PATH}"
ENV SHELL="/bin/bash"

RUN pnpm setup && \
    pnpm add -g \
    typescript \
    @tauri-apps/cli \
    playwright

# Install Playwright browsers
USER root
RUN npx playwright install
USER agent

# Install pensa CLI from source
USER agent
COPY --chown=agent:agent pensa-src /tmp/pensa-src
RUN . "$CARGO_HOME/env" && cargo install --path /tmp/pensa-src && rm -rf /tmp/pensa-src

ENV CARGO_TARGET_DIR="/home/agent/target"

# Ensure agent owns their home directory
RUN chown -R agent:agent /home/agent

USER agent
WORKDIR /home/agent

# Verify installations
RUN rustc --version && cargo --version && cargo geiger --version && prek --version && node --version && pnpm --version && npx playwright --version && pn --help
```

### sgf template build

Builds the `ralph-sandbox:latest` Docker image:

1. Locate the pensa crate source (at `crates/pensa/` relative to the springfield workspace, resolved via `CARGO_MANIFEST_DIR`)
2. Create a temporary build context directory
3. Write the Dockerfile (embedded in the sgf binary at compile time via `include_str!`)
4. Copy the pensa crate source into the build context as `pensa-src/`, inlining workspace `Cargo.toml` fields (`version`, `edition`, `license`) so it builds standalone
5. Compute SHA-256 hashes of the pensa source (Cargo.toml + all `src/*.rs` files, sorted) and Dockerfile content
6. Run `docker --context <CONTEXT> build -t ralph-sandbox:latest --label pn_hash=<sha256> --label dockerfile_hash=<sha256> .`
7. Clean up the temporary directory

The `pn` binary is compiled from source inside the Docker container via `cargo install --path`, ensuring the binary matches the container's architecture (no cross-compilation required on the host).

The labels enable pre-flight staleness detection. After updating pensa source or the Dockerfile, `sgf template build` bakes the new hashes into the image. The pre-flight check compares these labels against current values on every loop launch.

### Sandbox Behavior

- **File sync**: Workspace directory syncs bidirectionally between host and sandbox at the same absolute path via Mutagen. Changes the agent makes sync back to the host.
- **Credentials**: Docker Desktop automatically injects API keys from the host into the sandbox. Keys never enter the sandbox filesystem.
- **Agent user**: The agent runs as non-root `agent` user with `sudo` access inside the sandbox.
- **Pensa access**: Ralph writes `.pensa/daemon.url` before sandbox launch, containing `http://localhost:<port>` (read from `.pensa/daemon.port`). It also runs `docker sandbox network proxy <name> --allow-host localhost` to allow the sandbox's HTTP proxy to route `localhost` requests back to the host. When `pn` inside the sandbox finds `daemon.url`, it treats the daemon as remote (no auto-start) and forces HTTP traffic through the proxy (ignoring `no_proxy=localhost`). This ensures all sandbox instances share the same host daemon and database — no stale copies, atomic claims work across sandboxes.
- **Cargo target directory**: `CARGO_TARGET_DIR` is set to `/home/agent/target` (container-local) to avoid Mutagen sync corrupting compiled artifacts. Without this, rustc encounters SIGBUS errors when Mutagen partially syncs `.rlib`/`.rmeta` files mid-compilation.

---

## Hardcoded Defaults

| Setting | Value | Override |
|---------|-------|----------|
| Iterations | `30` | Positional arg: `sgf build auth 10` |
| Auto-push | `true` | `--no-push` flag |
| Docker template | `ralph-sandbox:latest` | None (constant) |
| Docker context | auto-detect | `SGF_DOCKER_CONTEXT` env var |
| Pensa daemon port | per-project derived | `--port` flag on `pn daemon` |

---

## Key Design Principles

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt enforces: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**One task, fresh context**: Each iteration picks one unblocked task, implements it fully, commits, and exits. The loop restarts with a clean context window. No accumulated confusion, no multi-task sprawl.

**Atomic iterations**: An iteration either commits fully or is discarded entirely. Partial work from crashed iterations is never preserved — sgf's pre-launch recovery wipes uncommitted state before the next run.

**Structured memory over markdown**: Pensa replaces markdown-based issue logging and plan tracking. A single CLI command replaces the error-prone multi-step process of creating directories and writing files. `pn` is the exclusive task tracker — agents must never use TodoWrite, TaskCreate, or markdown files for tracking work.

**Tasks as implementation plan**: There is no separate "implementation plan" entity. The living set of pensa tasks linked to a spec *is* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Editable prompts**: Prompts are plain markdown files owned by the project. Edit them as you learn what works. To improve defaults, update Springfield's templates.

**Env-var-driven context injection**: `PROMPT_FILES` lists the files to include in `study` instructions (default: `$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md`). For automated stages, ralph reads `PROMPT_FILES` and passes `--append-system-prompt` with a `study @<file>` instruction for each entry. For interactive stages, the `$AGENT_CMD` wrapper handles injection. sgf does not read or process `PROMPT_FILES` — it only validates prompt templates exist and passes raw paths through.

**Protected scaffolding**: `.sgf/` is protected from agent writes via Claude deny settings. The developer is the authority on prompts, backpressure, and project configuration.

**Decentralized projects**: Springfield is project-aware — it reads `.sgf/` from the current working directory. No global registry. Each project is self-contained.

**Sandboxed by default**: Automated stages (build, verify, test-plan, test) run inside Docker sandboxes via ralph for filesystem isolation. Interactive stages (spec, issues log) call `$AGENT_CMD` directly — no ralph, no Docker — so the agent can access files outside the repo. The wrapper never uses `--dangerously-skip-permissions` — without sandbox isolation, Claude's normal permission prompts are the safety boundary.

---

## Future Work

- **Context-efficient backpressure**: Swallow all build/test/lint output on success (show only a checkmark), dump full output only on failure. Preserves context window budget. See HumanLayer's `run_silent()` pattern.
- **Claude Code hooks for enforcement**: Use `PreToolUse` / `PostToolUse` hooks to enforce backpressure at the framework level — auto-run linters after file edits, block destructive commands. Could be scaffolded by `sgf init`.
- **TUI**: CLI-first for now. TUI can be added later as a view layer. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.
- **`sgf status` output spec**: Define what `sgf status` shows (running loops, pensa summary, recent activity). Specify after real usage reveals what's needed.
