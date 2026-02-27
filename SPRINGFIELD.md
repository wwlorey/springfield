# Springfield — Design Document

## What Is Springfield?

Springfield is a suite of Rust tools for orchestrating AI-driven software development using iterative agent loops. It codifies a workflow inspired by Geoffrey Huntley and the [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum) — breaking projects into well-scoped tasks and executing them through tight, single-task loops with fresh context each iteration.

The CLI entry point is `sgf`.

---

## Origin & Motivation

The workflow that Springfield formalizes has been developed through hands-on experience building projects with Claude Code. The current manual process involves:

1. Running a Claude Code session that interviews the developer, then generates specs and an implementation plan
2. Running iterative Claude Code loops (via a tool called Ralph) in interactive mode for a few supervised rounds
3. Switching to AFK mode and letting it run autonomously
4. Running verification loops that certify the codebase adheres to specs
5. Running test plan generation loops that produce test items
6. Running test execution loops that run the test items and produce a test report
7. Revising specifications — either because verification revealed gaps or because the developer wants to add/change features — then generating new implementation plan items and re-entering the build cycle

The process is cyclical: discuss → build → verify → revise specs → build again. Step 7 is always human-in-the-loop — the developer re-enters a discussion session to update existing specs and create new plan items for the delta.

Each stage uses a different prompt. Today these prompts are manually selected and kicked off, and live as near-duplicate markdown files across projects.

The problems Springfield solves:
- **Manual orchestration**: switching prompts and kicking off stages by hand
- **Prompt duplication**: near-identical prompt files across projects with minor per-project caveats sprinkled in
- **Messy issue tracking**: markdown-based issue logging is unreliable for agents — they struggle with the multi-step process of creating directories, writing files, and following naming conventions
- **No persistent structured memory**: agents lose context between sessions and have no reliable, non-markdown way to track work items and issues across loop iterations
- **No unified monitoring**: no way to observe multiple loops across projects

---

## Architecture

### Workspace Structure

```
springfield/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── sgf/                   — CLI binary, entry point, scaffolding, prompt assembly
│   ├── pensa/                 — agent persistent memory (CLI binary + library)
│   └── ralph/                 — loop runner (standalone binary)
```

### Components

**`sgf`** — The CLI entry point. All developer interaction goes through this binary. It delegates to the other crates internally. Also responsible for project scaffolding.
- `sgf init` — scaffolds project structure: `.sgf/` (config, backpressure, prompts), `.pensa/`, skeleton `memento.md`, empty `specs/README.md`, Claude deny settings for `.sgf/` protection

**`pensa`** (Latin: "tasks", singular: pensum) — A Rust CLI that serves as the agent's persistent structured memory. Replaces markdown-based issue logging and implementation plan tracking. Inspired by [beads](https://github.com/steveyegge/beads) but built in Rust with tighter integration into the Springfield workflow. Stores issues with typed classification, dependencies, priorities, ownership, and status tracking. Uses SQLite locally with JSONL export for git portability.

**`ralph`** — The loop runner. Executes Claude Code iteratively against a prompt file inside Docker sandboxes. Supports interactive mode (terminal passthrough with notification sounds) and AFK mode (NDJSON stream parsing with formatted output). Standalone binary — `sgf` invokes it as a subprocess, assembling prompts and passing them as arguments. Originally developed in the [buddy-ralph](../buddy-ralph/ralph/) project; copied into this workspace as a clean break with full ownership.

### sgf-to-ralph Contract

Ralph is template-unaware. `sgf` owns all prompt assembly — it reads the template from `.sgf/prompts/<stage>.md`, substitutes variables, writes the result to `.sgf/prompts/.assembled/<stage>.md`, and passes that file path to ralph. Ralph receives a final prompt and runs it.

#### Invocation

```
sgf → ralph [-a] [--loop-id ID] [--template T] [--auto-push BOOL] [--max-iterations N] ITERATIONS PROMPT
```

`sgf` reads `.sgf/config.toml` and translates settings into CLI flags. Ralph does not read config files — all configuration arrives via flags.

#### CLI Flags

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `-a` / `--afk` | bool | sgf command (e.g., `sgf build -a`) | AFK mode: NDJSON stream parsing with formatted output |
| `--loop-id` | string | sgf-generated | Unique identifier for this loop run (see Loop ID format below) |
| `--template` | string | `config.toml → docker_template` | Docker sandbox template image |
| `--auto-push` | bool | `config.toml → auto_push` | Auto-push after commits |
| `--max-iterations` | u32 | `config.toml → default_iterations` | Safety limit for iterations |
| `--command` | string | (testing only) | Override executable for testing |
| `ITERATIONS` | u32 | sgf command args or `config.toml → default_iterations` | Number of iterations to run |
| `PROMPT` | path | `.sgf/prompts/.assembled/<stage>.md` | Assembled prompt file |

#### Exit Codes

| Code | Meaning | sgf response |
|------|---------|--------------|
| `0` | Sentinel found (`.ralph-complete`) — loop completed its work | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

Claude Code crashes (non-zero exit from a single iteration) and push failures are handled within ralph as warnings — they do not produce distinct exit codes. Ralph logs the failure and continues to the next iteration.

#### Completion Sentinel

The agent creates a `.ralph-complete` file when `pn ready` returns no tasks. Ralph checks for this file after each iteration. If found, ralph deletes it, performs a final auto-push (if enabled), and exits with code `0`.

#### Prompt Assembly

`sgf` handles all prompt templating before invoking ralph:

1. Read the template from `.sgf/prompts/<stage>.md`
2. Substitute variables (e.g., spec filter, stage-specific flags)
3. Write the assembled prompt to `.sgf/prompts/.assembled/<stage>.md`
4. Pass the file path as ralph's `PROMPT` argument

The `.assembled/` directory is gitignored. Assembled prompts persist for debugging — inspect what was actually sent to the agent.

#### Loop ID Format

`sgf` generates loop IDs with the pattern: `<stage>[-<spec>]-<YYYYMMDDTHHmmss>`

Examples:
- `build-auth-20260226T143000` (build loop filtered to auth spec)
- `verify-20260226T150000` (verify loop, no spec filter)
- `issues-plan-20260226T160000` (issues plan loop)

Ralph includes the loop ID in log output. `sgf logs` uses the loop ID to locate log files.

#### Logging

`sgf` tees ralph's stdout to both the terminal and `.sgf/logs/<loop-id>.log`. Ralph owns formatting — in AFK mode it emits human-readable one-liners (tool calls, text blocks); in interactive mode it passes through the terminal. `sgf` does not parse ralph's output.

The `.sgf/logs/` directory is gitignored.

#### Recovery

Ralph does not perform iteration-start cleanup. Recovery is `sgf`'s responsibility, executed before launching ralph.

**PID files**: `sgf` writes `.sgf/run/<loop-id>.pid` on launch (containing its process ID) and removes it on clean exit. The `.sgf/run/` directory is gitignored.

**Pre-launch cleanup**: Before launching ralph, `sgf` scans all PID files in `.sgf/run/`:

- **Any PID alive** (verified via `kill -0`) → another loop is running. Skip cleanup and launch normally — the dirty tree or in-progress claims may belong to that loop.
- **All PIDs stale** (process dead) → no loops are running. Remove stale PID files, then recover:
  1. `git checkout -- .` — discard modifications to tracked files
  2. `git clean -fd` — remove untracked files (respects `.gitignore`, so `db.sqlite`, logs, and assembled prompts are safe)
  3. `pn doctor --fix` — release stale claims and repair integrity

**Principle**: Work is only preserved when committed. Uncommitted changes from crashed iterations are discarded — the agent that produced them is gone and cannot continue them.

---

## Pensa — Agent Persistent Memory

### Purpose

Give agents a CLI-accessible, structured way to log issues and track work items that persists across sessions. A single command like `pn create "login crash on empty password" -p p0 -t bug` (where `-t` specifies the issue type) replaces the error-prone multi-step process of creating directories and writing markdown files.

### Storage Model

Dual-layer storage (same pattern as beads):
- **`.pensa/db.sqlite`** — the working database, gitignored. Rebuilt from JSONL on clone.
- **`.pensa/*.jsonl`** — the git-committed exports. Separate files per entity: `issues.jsonl`, `deps.jsonl`, `comments.jsonl`. Events are not exported (derivable from issue history, avoids monotonic file growth). Human-readable, diffs cleanly.

Sync is automated via prek (git hooks):
- **Pre-commit hook**: runs `pn export` to write SQLite → JSONL
- **Post-merge hook**: runs `pn import` to rebuild JSONL → SQLite

**Runtime sharing model**: All concurrent loops share a single `.pensa/db.sqlite` via bind-mounted host directory. SQLite uses the default DELETE journal mode (not WAL — WAL requires shared memory via mmap, which breaks across Docker Desktop's VirtioFS boundary). Pensa sets these pragmas on every connection: `busy_timeout=5000` (retries for 5s on lock contention instead of failing immediately) and `foreign_keys=ON` (enforces referential integrity for deps and comments). Pensa's write operations are brief and infrequent (a few per minute across all loops), so serialized writes are effectively invisible. JSONL files are the git-portable layer — they capture a snapshot at commit time via `pn export` and are never read at runtime by concurrent loops. On clone or post-merge, `pn import` rebuilds SQLite from JSONL.

*Note: Docker Desktop on macOS/Windows uses VirtioFS, which has [known file-locking limitations](https://github.com/docker/for-mac/issues/7004). Springfield's low write frequency makes contention practically unlikely, but concurrent loops are most reliable on native Linux Docker.*

**Why not Dolt?** Dolt (version-controlled SQL database) was evaluated as an alternative that would eliminate the dual-layer sync. However, SQLite + JSONL is the better fit: SQLite is tiny and ubiquitous (no extra binary in Docker sandboxes), JSONL travels with the project's git repo (no second remote like DoltHub needed), and `rusqlite` is a mature Rust integration (vs. shelling out to `dolt sql -q`). Dolt's strengths — native table-level merges, built-in branching — matter more in multi-user scenarios, which Springfield doesn't target. The sync hooks are a few lines of prek config.

### Schema

Everything is an issue (following the GitHub model, matching beads' single-entity approach). Issues are distinguished by `issue_type` — a required enum — rather than separate entity types.

#### Issues table

Each issue has: `id`, `title`, `description`, `issue_type`, `status`, `priority`, `spec`, `fixes`, `assignee`, `created_at`, `updated_at`, `closed_at`, `close_reason`.

**`id`** — Format: `pn-` prefix + 8 hex chars from UUIDv7 (timestamp component + random bytes). Example: `pn-a1b2c3d4`. Short enough for agents to type in commands, collision-resistant across concurrent agents and branches. Not content-based — two agents logging the same bug get different IDs.

**`issue_type`** (required) — Enum: `bug`, `task`, `test`, `chore`. Set at creation, immutable after that. Matching beads' `issue_type` concept.
- **`bug`** — problems discovered during build/verify/test
- **`task`** — implementation plan items from the spec phase
- **`test`** — test plan items from the test-plan phase
- **`chore`** — tech debt, refactoring, dependency updates, CI fixes

**`spec`** (optional) — filename stem of the spec this issue implements (e.g., `auth` for `specs/auth.md`). Populated for `task` items, typically absent for `bug` and `chore` items. There is no separate "implementation plan" entity — the living set of tasks linked to a spec *is* the implementation plan for that spec.

**`fixes`** (optional) — ID of a bug that this issue resolves. Set on `task` items created by `sgf issues plan`. When a task with a `fixes` link is closed, the linked bug is automatically closed with reason `"fixed by pn-xxxx"`. Models the same relationship as GitHub's "fixes #123".

**`priority`** — Enum: `p0` (critical), `p1` (high), `p2` (normal), `p3` (low). Smaller number = more urgent.

**Statuses**: `open`, `in_progress`, `closed`.

#### Dependencies table

`issue_id`, `depends_on_id` (composite PK). Models blocking relationships. `pn ready` uses this to filter to unblocked issues.

**Bugs are never "ready"**: `pn ready` only returns items with `issue_type` in (`task`, `test`, `chore`) — bugs are excluded entirely. Bugs are problem reports, not work items. The `sgf issues plan` loop converts bugs into tasks (with a `fixes` link), and those tasks flow through `pn ready` like any other work item.

#### Comments table

`id` (hash-based, same format as issue IDs), `issue_id`, `actor`, `text`, `created_at`. Agents record observations about issues between fresh-context iterations without overwriting the description.

#### Events table (audit log)

`issue_id`, `event_type`, `actor`, `detail`, `created_at`. Every mutation (create, update, close, reopen, claim, comment, dep add/remove) gets logged. Powers `pn history`.

### CLI Commands

All commands support `--json` for agent consumption.

**Global flags**: `--actor <name>` (who is running this command — for audit trail; resolution: `--actor` flag > `PN_ACTOR` env var > `git config user.name` > `$USER`).

**`--claim`** is shorthand for `--status in_progress -a <current_actor>`. **`--unclaim`** is shorthand for `--status open -a ""`. **`pn release <id>`** is an alias for `pn update <id> --unclaim`.

#### Working with issues

```
pn create "title" -t <issue_type> [-p <pri>] [-a <assignee>] [--spec <stem>] [--fixes <bug-id>] [--description <text>] [--dep <id>]
pn q "title" -t <issue_type> [-p <pri>] [--spec <stem>]  # quick capture, outputs only ID
pn update <id> [--title <t>] [--status <s>] [--priority <p>] [-a <assignee>] [--description <d>] [--claim] [--unclaim]
pn close <id> [--reason "..."] [--force]
pn reopen <id> [--reason "..."]
pn release <id>                            # unclaim: set status→open, clear assignee
pn delete <id> [--force]
pn show <id> [--short]
```

#### Views and queries

```
pn list [--status <s>] [--priority <p>] [-a <assignee>] [-t <issue_type>] [--spec <stem>] [--sort <field>] [-n <limit>]
pn ready [-n <limit>] [-p <pri>] [-a <assignee>] [-t <issue_type>] [--spec <stem>]  # returns [] when nothing matches
pn claim-next [-p <pri>] [-t <issue_type>] [--spec <stem>]  # atomic ready + claim; returns claimed issue or null
pn blocked
pn search <query>                          # substring match on title + description
pn count [--by-status] [--by-priority] [--by-issue-type] [--by-assignee]
pn status                               # project health snapshot
pn history <id>                          # issue change history from audit log
```

#### Dependencies

```
pn dep add <child> <parent>
pn dep remove <child> <parent>
pn dep list <id>
pn dep tree <id> [--direction up|down]
pn dep cycles
```

#### Comments

```
pn comment add <id> "text"
pn comment list <id>
```

#### Data and maintenance

```
pn export                                # SQLite → JSONL (issues.jsonl, deps.jsonl, comments.jsonl)
pn import                                # JSONL → SQLite (rebuild from committed files)
pn doctor [--fix]                        # health checks: stale claims (in_progress >30min), orphaned deps, sync drift. --fix releases stale claims and repairs integrity.
pn where                                 # show .pensa/ directory path
```

### Git Hooks (via prek)

[prek](https://github.com/j178/prek) is used for git hook management. `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: Export pensa DB to JSONL
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: Import JSONL to pensa DB
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
```

---

## Per-Repo Project Structure

After `sgf init`, a project contains:

```
.pensa/
├── db.sqlite                  (gitignored)
├── issues.jsonl               (committed)
├── deps.jsonl                 (committed)
└── comments.jsonl             (committed)
.sgf/
├── config.toml                (committed — stack type, project config)
├── backpressure.md            (committed — build/test/lint/format commands)
├── logs/                      (gitignored — AFK loop output)
│   └── <loop-id>.log
├── run/                       (gitignored — PID files for running loops)
│   └── <loop-id>.pid
└── prompts/
    ├── build.md               (committed — editable prompt templates)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    ├── issues.md
    ├── issues-plan.md
    └── .assembled/            (gitignored — assembled prompts for debugging)
        └── <stage>.md
.pre-commit-config.yaml        (prek hooks for pensa sync)
memento.md                     (committed — thin reference document)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (links to memento + agents)
test-report.md                 (generated — overwritten each run, committed)
verification-report.md         (generated — overwritten each run, committed)
specs/
├── README.md                  (agent-maintained spec index — loom-style tables)
└── *.md                       (prose specification files)
```

### File Purposes

**`memento.md`** — A thin reference document the agent reads at the start of every iteration. Contains references to external files, not content itself:
- Project stack type (one line)
- Reference to `.sgf/backpressure.md` — "read this for build/test/lint/format commands"
- Reference to `specs/README.md` — "read this for the spec index"
- Reference to pensa (`pn`) — "use `pn` for all issue and task tracking"

The memento is a table of contents. The agent reads it, follows the references to get detail, and dives into specific files only when needed. Auto-loaded via CLAUDE.md at the start of every iteration.

**Memento lifecycle**:
- **`sgf init`** generates a skeleton memento: stack type, references to backpressure and an empty spec index, pensa reference. For empty projects this is all that's needed — the references point to files that exist but have no content yet.
- **`sgf spec`** — the spec agent updates `specs/README.md` (adding/revising rows in the spec index table) after writing spec files. The memento itself doesn't change — it already points to `specs/README.md`.
- **`sgf build`** — the build agent updates `specs/README.md` if it creates new modules that should appear in the code column. The memento itself doesn't change.
- There is no standalone `sgf memento` command. The memento is written once by `sgf init` and rarely needs modification after that — the content it references is what evolves.

**`specs/README.md`** — Agent-maintained spec index, matching the [loom format](https://github.com/ghuntley/loom/blob/trunk/specs/README.md). Categorized tables with `| Spec | Code | Purpose |` columns mapping each spec to its implementation location and a one-line summary. This is the living map of what's been specified and where it lives in code. Agents update this file when adding or revising specs.

**`.sgf/backpressure.md`** — Build, test, lint, and format commands for the project. Generated by `sgf init` from stack-specific templates stored in Springfield's source. Developer-editable after scaffolding (e.g., to add flags or change test commands). Agents must not modify this file — it is protected by Claude deny settings (see below). Static after initialization in most projects.

**`AGENTS.md`** — Hand-authored operational guidance. Contains information that doesn't fit the memento's structured format — code style preferences, runtime notes, special instructions. Linked from CLAUDE.md so Claude Code auto-loads it.

**`CLAUDE.md`** — Entry point for Claude Code. Links to memento.md and AGENTS.md.

**`.sgf/config.toml`** — Project-specific configuration for the `sgf` tool itself (not read by agents):
- `stack` — project type (rust, typescript, tauri, go, etc.), used by `sgf` to select backpressure templates
- `docker_template` — Docker sandbox template name (default per stack preset)
- `auto_push` — whether loops auto-push after commits (default: `true`)
- `default_iterations` — default iteration count for AFK loops (default: `25`)

**`.sgf/prompts/`** — Editable prompt templates for each workflow stage (`spec.md`, `build.md`, `verify.md`, `test-plan.md`). Seeded by `sgf init` from Springfield's built-in templates. Once seeded, the project owns these files — edit them freely to evolve the prompts as you learn what works for the project. To improve defaults for future projects, update the templates in the Springfield repo itself.

**`.sgf/` protection** — The entire `.sgf/` directory is protected from agent modification via Claude settings. `sgf init` scaffolds a `.claude/settings.json` (or updates an existing one) with deny rules for `.sgf/**`. This is enforced at the framework level — agents cannot write to prompts, backpressure, or config regardless of prompt instructions.

**`specs/`** — Prose specification files (one per topic of concern). These are authored documents — written during the spec phase, consumed during builds. Indexed in `specs/README.md`.

---

## SGF Commands

```
sgf init [--stack <type>]       — scaffold a new project (interactive preset selection, or --stack flag)
sgf spec                       — generate specs and implementation plan
sgf build [--spec <stem>] [-a] [iterations] — run a Ralph loop (interactive or AFK)
sgf verify                     — run verification loop
sgf test-plan                  — run test plan generation loop
sgf test [--spec <stem>] [-a] [iterations] — run test execution loop (interactive or AFK)
sgf issues log                 — interactive session for logging bugs
sgf issues plan [-a] [iterations] — run bug planning loop (AFK)
sgf status                     — show project state (active loops, pensa summary)
sgf logs <loop-id>             — tail a running loop's output
```

### Deployment Model

**Decentralized**: Springfield is project-aware — it reads `.sgf/` from the current working directory. There is no global registry or central config. Each project is self-contained. To work on multiple projects, run `sgf` from each project directory.

### Sandboxing

All sessions run inside Docker sandboxes, including human-in-the-loop stages like `sgf spec` and `sgf issues log`.

---

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready --spec <stem>` returns nothing (all tasks for a spec are done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., `sgf build` + `sgf issues plan`) can run concurrently on the same branch. SQLite provides atomic claims via `pn claim-next` (`SELECT ... WHERE status = 'open' + UPDATE` in a single transaction, preventing double-claiming). `pn export` runs at commit time via the pre-commit hook. If `git push` fails due to a concurrent commit, the loop should `git pull --rebase` and retry. Stop build loops before running `sgf spec` to avoid task-supersession race conditions.

### Standard Loop Iteration

Build and Test stages share a common iteration pattern. Each iteration:

1. **Orient** — read `memento.md`
2. **Claim** — `pn claim-next [filters] --json`. If none, write `.ralph-complete` and exit — the loop is finished.
3. **Work** — stage-specific (see below)
4. **Log issues** — if problems are discovered: `pn create "description" -t bug`
5. **Close/release** — close or release the work item
6. **Commit** — commit all changes (the pre-commit hook runs `pn export` automatically, syncing SQLite to JSONL)

Each iteration gets fresh context. The pensa database persists state between iterations. The stages differ only in their claim filters, work, and close steps:

| Stage | Claim | Work | Close |
|-------|-------|------|-------|
| Build | `pn claim-next [--spec <stem>] --json` | Implement the task; apply backpressure (build, test, lint per `.sgf/backpressure.md`) | `pn close <id> --reason "..."` |
| Test | `pn claim-next -t test [--spec <stem>] --json` | Execute the test | `pn close <id> --reason "..."` |

### 1. Spec (`sgf spec`)

Opens a Claude Code session with the spec prompt. The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates both deliverables:

1. Write spec files to `specs/`
2. Update `specs/README.md` with new index entries (loom-style `| Spec | Code | Purpose |` rows)
3. Create implementation plan items via `pn create -t task --spec <stem>`, with dependencies and priorities
4. Commit and push

The interview and generation happen in a single session. The agent asks clarifying questions as needed, but the goal is always to produce specs and a plan. The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line.

There is no separate "implementation plan" entity. The set of open tasks linked to a spec via `--spec <stem>` *is* the implementation plan for that spec. Querying the plan is just `pn list -t task --spec <stem>` (where `-t` filters by issue type).

**Spec revision**: This same workflow applies to revising existing specs — run `sgf spec` again. **Stop any running build loops before revising specs** to avoid race conditions where in-progress tasks get superseded mid-iteration. When revising, the agent:
1. Reviews existing tasks for the spec: `pn list --spec <stem> --json`
2. Closes tasks that are no longer relevant: `pn close <id> --reason "superseded by revised spec"`
3. Creates new tasks for the delta: `pn create "..." -t task --spec <stem>`
4. Updates the spec file in `specs/`
5. Restart build loops after revision is committed

Specs are living documents, never sealed/frozen.

### 2. Build (`sgf build`)

Follows the standard loop iteration. Runs a Ralph loop using `.sgf/prompts/build.md`. Accepts an optional `--spec <stem>` flag to focus on a single spec's tasks. When omitted, the agent works across all specs and open issues.

`sgf` assembles the prompt by injecting the spec filter (if any) into the build template before passing it to Ralph. The build stage adds **backpressure** to the work step — after implementing the task, the agent runs build, test, and lint commands per `.sgf/backpressure.md` before committing.

Run interactively first for a few supervised rounds, then switch to AFK mode (`-a`) for autonomous execution.

### 3. Verify (`sgf verify`)

Runs a Ralph loop using `.sgf/prompts/verify.md`. The agent:
1. Reads specs from `specs/README.md` index
2. Investigates each spec against the actual codebase
3. Marks conformance (matches / partial / missing)
4. Generates a verification report
5. Logs any gaps as pensa issues

### 4. Test Plan (`sgf test-plan`)

Runs a Ralph loop using `.sgf/prompts/test-plan.md`. The agent:
1. Studies specs and codebase
2. Generates a testing plan
3. Ensures tests are automatable (can be run by agents in loops)
4. Creates test items via `pn create -t test --spec <stem>`, with dependencies and priorities

### 5. Test (`sgf test`)

Follows the standard loop iteration. Runs a Ralph loop using `.sgf/prompts/test.md`. Accepts an optional `--spec <stem>` flag to focus on a single spec's test items. When omitted, the agent works across all specs.

After all test items are closed, a final iteration generates `test-report.md` in the project root — a summary of all test results, pass/fail status, and any bugs logged.

### 6. Issues Log (`sgf issues log`)

Runs a Ralph loop using `.sgf/prompts/issues.md`. Each iteration handles one bug:
1. The developer describes a bug they've observed
2. The agent interviews them to capture details — steps to reproduce, expected vs actual behavior, relevant context
3. Logs the bug to pensa via `pn create -t bug`
4. The session exits and a fresh one spawns

One bug per iteration, fresh context each time. The developer describes, the agent captures, the session dies. This prevents context from accumulating across unrelated bugs and keeps each interaction focused.

### 7. Issues Plan (`sgf issues plan`)

Runs a Ralph loop using `.sgf/prompts/issues-plan.md`. A separate concurrent process from `sgf build`. Does not follow the standard loop iteration — bugs are problem reports excluded from `pn ready`, so Issues Plan uses its own claim flow:

1. **Orient** — read `memento.md`
2. **Find** — `pn list -t bug --status open --json`. If none, write `.ralph-complete` and exit.
3. **Claim** — `pn update <id> --claim`
4. **Work** — study codebase, create fix task: `pn create -t task "fix: ..." --fixes <bug-id>`
5. **Release** — `pn release <id>` (bug stays open as the problem record)
6. **Commit** — commit all changes

The fix task flows through `pn ready` and gets picked up by the build loop. When the build loop closes the fix task, the linked bug is automatically closed with reason `"fixed by pn-xxxx"`.

**Typical setup**: two terminals running concurrently — `sgf issues plan` producing fix tasks from bugs, `sgf build` consuming them alongside other work items. A third terminal with `sgf issues` open for the developer to log new bugs as they're found.

### 8. Inline Issue Logging

Issues are also logged by agents during any stage via `pn create`. The build loop logs bugs it discovers during implementation. The verify loop logs spec gaps. The test loop logs test failures. `sgf issues log` is for developer-reported bugs; inline logging is for agent-discovered bugs.

---

## Prompts

Each workflow stage has a corresponding prompt file in `.sgf/prompts/`. These are plain markdown files — edit them directly.

**Seeding**: `sgf init` copies the current prompt templates from Springfield into `.sgf/prompts/`. From that point, the project owns its prompts.

**Editing**: Prompts evolve as you learn what works for a project. Add caveats ("Mac-first builds"), change the workflow ("commit" vs "commit and push"), tune instructions. Edit the files in your editor, read diffs in git — they're just markdown.

**Upstream improvements**: To improve defaults for all future projects, update the templates in the Springfield repo. Existing projects keep their copies and can pull changes manually if desired.

**Backpressure**: Backpressure commands (build, test, lint, format) live in `.sgf/backpressure.md`, not in the prompts. The memento references the backpressure file, so agents discover it automatically. The specific commands are defined once and edited by the developer — agents cannot modify them.

This replaces the duplication seen in buddy-ralph's `prompts/building/` directory where 8 similar files existed with minor variations. Instead of near-duplicate files with caveats sprinkled in, there's one editable copy per project.

---

## Key Design Principles

**Fresh context per iteration**: Each Ralph loop iteration starts with a clean context window. The agent reads the memento and pensa state to orient itself. No accumulated confusion.

**One task per iteration**: The agent picks one unblocked task, implements it fully, applies backpressure, commits, and exits. The loop restarts with fresh context.

**Structured memory over markdown**: Pensa replaces unstructured markdown files for issues and tasks. A single CLI command replaces multi-step file creation. The agent finds this easier and more reliable.

**Tasks are the plan**: There is no separate "implementation plan" entity. The set of open `task` issues linked to a spec via the `spec` field is the implementation plan for that spec. Revising a spec means closing superseded tasks and creating new ones — the plan is always the current state of pensa, not a document that drifts.

**Editable prompts over duplication**: `sgf init` seeds prompt templates into the project. Each project owns and can evolve its prompts. No near-duplicate files across projects — one editable copy per stage, per project.

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt must enforce: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**Backpressure drives quality**: Build, test, lint, and format commands (defined in `.sgf/backpressure.md`) are applied after every change. Failed validation forces correction before commits.

**Thin memento, rich references**: The memento is a table of contents — it contains references to backpressure, the spec index, and pensa, not the content itself. What evolves is the referenced files; the memento is written once by `sgf init` and rarely changes. Matches the loom pattern.

**Scaffolding is protected**: The `.sgf/` directory (prompts, backpressure, config) is developer-owned and agent-readonly, enforced via Claude deny settings. Agents read these files but cannot modify them.

**Decentralized projects**: Each project is self-contained. No global state, no central server, no coordination between projects. Run `sgf` from the project directory.

**Sandboxed execution**: All sessions run in Docker sandboxes — autonomous and human-in-the-loop alike.

---

## Resolved Decisions

- **Build order**: Pensa first (self-contained, agents need it immediately), then sgf init (scaffolding), then sgf spec/build (prompt assembly + ralph integration).

---

## Potential Future Work

- **Context-efficient backpressure**: Swallow all build/test/lint output on success (show only a checkmark), dump full output only on failure. Preserves context window budget. Could be a wrapper script agents call or a prompt-level instruction. See HumanLayer's `run_silent()` pattern.
- **Claude Code hooks for enforcement**: Use `PreToolUse` / `PostToolUse` hooks to enforce backpressure at the framework level — auto-run linters after file edits, block destructive commands. Defense-in-depth: even if prompt instructions are ignored, hooks still fire. Could be scaffolded into `.sgf/` by `sgf init`.
- **TUI**: CLI-first for now. TUI can be added later as a view layer over the same operations. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.

---

## References

- [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)
- [Beads — graph issue tracker for AI agents](https://github.com/steveyegge/beads)
- [Dolt — version-controlled SQL database](https://github.com/dolthub/dolt)
- [prek — Rust-based git hook manager](https://github.com/j178/prek)
- [Ralph implementation (buddy-ralph)](../buddy-ralph/ralph/)
- [buddy-ralph project structure](../buddy-ralph/) — reference implementation of the manual workflow Springfield codifies
- [Loom specs/README.md](https://github.com/ghuntley/loom/blob/trunk/specs/README.md) — reference format for spec index tables