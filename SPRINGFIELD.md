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

Claude Code crashes (non-zero exit from a single iteration) and push failures are handled within ralph as warnings — they do not produce distinct exit codes. Ralph logs the failure and continues to the next iteration without cleanup — no dirty-tree reset, no claim release, no recovery steps. The next iteration's agent inherits whatever state exists (uncommitted edits, stale claim) and proceeds via forward correction. Stale claims and dirty working trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery before the next run (see Recovery).

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
- **`.pensa/db.sqlite`** — the working database, gitignored. Lives on the host, owned by the pensa daemon. Rebuilt from JSONL on clone.
- **`.pensa/*.jsonl`** — the git-committed exports. Separate files per entity: `issues.jsonl`, `deps.jsonl`, `comments.jsonl`. Events are not exported (derivable from issue history, avoids monotonic file growth). Human-readable, diffs cleanly.

Git sync is automated via prek (git hooks):
- **Pre-commit hook**: runs `pn export` to write SQLite → JSONL and stage the JSONL files
- **Post-merge hook**: runs `pn import` to rebuild JSONL → SQLite

#### Runtime Architecture

Pensa uses a client/daemon model. The daemon runs on the host, owns the SQLite database, and handles all reads and writes. The `pn` CLI is a thin client that connects to the daemon over HTTP.

**Why a daemon?** Docker sandboxes use Mutagen-based file synchronization (not bind mounts). POSIX file locks don't propagate across the sync boundary — two sandboxes writing to the same SQLite file would corrupt it. The daemon keeps SQLite on the host behind a single process, making concurrent access from multiple sandboxes safe.

**Daemon** (`pn daemon`): Listens on a local port (default: `7533`). Owns `.pensa/db.sqlite` directly via `rusqlite`. Sets pragmas on every connection: `busy_timeout=5000`, `foreign_keys=ON`. All mutation is serialized through the daemon — no concurrent SQLite writers.

**CLI client**: Every `pn` command (create, list, ready, close, etc.) sends an HTTP request to the daemon. The CLI discovers the daemon via `PN_DAEMON` env var (default: `http://localhost:7533`). Inside Docker sandboxes, this is `http://host.docker.internal:7533`. If the daemon is unreachable, the CLI fails with a clear error.

**Lifecycle**: `sgf` starts the daemon automatically before launching loops (if not already running). The daemon can also be started manually via `pn daemon`. It runs in the foreground (daemonization is the caller's responsibility — `sgf` backgrounds it). Stops on SIGTERM or when `sgf` shuts down.

**JSONL export**: JSONL files are the git-portable layer — they capture a snapshot at commit time via `pn export` (called by the pre-commit hook) and are never read at runtime. On clone or post-merge, `pn import` rebuilds SQLite from JSONL. Since the daemon owns the database, `pn export` and `pn import` are daemon commands — the CLI sends the request, the daemon performs the I/O.

**Why not Dolt?** SQLite + JSONL is simpler: SQLite is tiny, JSONL travels with git (no DoltHub remote needed), and `rusqlite` is mature. Dolt's strengths (table-level merges, branching) matter more in multi-user scenarios.

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

**Bugs are never "ready"**: `pn ready` only returns items with `issue_type` in (`task`, `test`, `chore`) — bugs are excluded entirely. Bugs are problem reports, not actionable work items.

#### Comments table

`id` (hash-based, same format as issue IDs), `issue_id`, `actor`, `text`, `created_at`. Agents record observations about issues between fresh-context iterations without overwriting the description.

#### Events table (audit log)

`issue_id`, `event_type`, `actor`, `detail`, `created_at`. Every mutation (create, update, close, reopen, claim, comment, dep add/remove) gets logged. Powers `pn history`.

### CLI Commands

All commands support `--json` for agent consumption (see [JSON Output](#json-output)).

**Global flags**: `--actor <name>` (who is running this command — for audit trail; resolution: `--actor` flag > `PN_ACTOR` env var > `git config user.name` > `$USER`).

**`--claim`** is atomic: `UPDATE ... SET status = 'in_progress', assignee = <actor> WHERE id = <id> AND status = 'open'`. If another agent already claimed the issue, the command fails with an `already_claimed` error (and reports who holds it). The agent should re-run `pn ready` and pick a different task. **`--unclaim`** is shorthand for `--status open -a ""`. **`pn release <id>`** is an alias for `pn update <id> --unclaim`.

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

#### Daemon

```
pn daemon [--port <port>]               # start the daemon (foreground, default port 7533)
pn daemon status                        # check if daemon is running and reachable
```

#### Data and maintenance

```
pn export                                # SQLite → JSONL, then `git add .pensa/*.jsonl`
pn import                                # JSONL → SQLite (rebuild from committed files)
pn doctor [--fix]                        # health checks: stale claims (in_progress >30min), orphaned deps, sync drift. --fix releases stale claims and repairs integrity.
pn where                                 # show .pensa/ directory path
```

### JSON Output

Following beads' pattern: no envelope, direct data to stdout.

**Routing**: Success data → stdout. Errors → stderr. Both as JSON when `--json` is active.

**Exit codes**: `0` success, `1` error. No further granularity.

**Error shape** (stderr):
```json
{"error": "issue not found: pn-a1b2c3d4", "code": "not_found"}
```
The `code` field is present only when there's a machine-readable error code. Known codes: `not_found`, `already_claimed`, `cycle_detected`, `invalid_status_transition`.

**Null arrays**: Always `[]`, never `null`.

**Per-command output shapes** (stdout):

| Command | Shape |
|---------|-------|
| `create`, `update`, `close`, `reopen`, `release` | Single issue object |
| `q` | `{"id": "pn-a1b2c3d4"}` |
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

**Issue object fields** mirror the schema: `id`, `title`, `description`, `issue_type`, `status`, `priority`, `spec`, `fixes`, `assignee`, `created_at`, `updated_at`, `closed_at`, `close_reason`. Absent optional fields are omitted (not `null`).

### Git Hooks (via prek)

[prek](https://github.com/j178/prek) manages git hooks via `.pre-commit-config.yaml`. Two hooks automate pensa sync: a pre-commit hook runs `pn export` (SQLite → JSONL before every commit), and post-merge/post-checkout/post-rewrite hooks run `pn import` (JSONL → SQLite after pulls and rebases).

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

**Memento lifecycle**: `sgf init` generates the memento (stack type, references to backpressure, spec index, and pensa). It rarely changes after that — the files it references are what evolve.

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
sgf template build [--stack <type>] — rebuild Docker sandbox template (after updating pn or stack tools)
```

### Deployment Model

**Decentralized**: Springfield is project-aware — it reads `.sgf/` from the current working directory. There is no global registry or central config. Each project is self-contained. To work on multiple projects, run `sgf` from each project directory.

### Sandboxing

All sessions run inside Docker Desktop sandboxes (`docker sandbox run`), including human-in-the-loop stages like `sgf spec` and `sgf issues log`.

**Docker over native sandbox**: Claude Code's built-in sandbox (`/sandbox`) provides OS-level filesystem and network isolation, but bundles them together — network access requires per-domain approval, which is incompatible with AFK/unattended operation. Docker provides the filesystem isolation we need (protecting the host machine) while leaving network access unrestricted, which is the right tradeoff for an autonomous runner. The template image overhead is worth it.

**How sandboxes work**: Docker sandboxes are microVMs (not containers). The workspace directory syncs bidirectionally between host and VM at the same absolute path — `/Users/you/project` on the host appears at `/Users/you/project` inside the sandbox. This is file synchronization, not volume mounting. Changes the agent makes sync back to the host; changes on the host sync into the VM.

**Template images**: Each sandbox runs from a template image built on `docker/sandbox-templates:claude-code`, which includes Ubuntu, Git, Node.js, Python, Go, and common dev tools. Project-specific templates add stack tooling (e.g., Rust toolchain for `rust-sandbox`, Tauri dependencies for `tauri-sandbox`). Templates are Dockerfiles:

```dockerfile
FROM docker/sandbox-templates:claude-code
USER root
RUN apt-get update && apt-get install -y <stack-specific-tools>
COPY pn /usr/local/bin/pn
USER agent
```

**Pensa in the sandbox**: The `pn` binary is baked into every template image. Inside the sandbox, `pn` connects to the pensa daemon on the host via `http://host.docker.internal:7533` — the SQLite database never enters the sync boundary. All sandboxes and the host CLI share the same daemon, so concurrent access is safe. After updating `pn`, rebuild template images to pick up the new version.

**Credentials**: The sandbox uses Docker Desktop's credential proxy (`--credentials host`), which intercepts outbound API requests and injects authentication headers from the host. API keys never enter the sandbox.

**Agent user**: The agent runs as a non-root `agent` user with `sudo` access inside the sandbox.

---

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready --spec <stem>` returns nothing (all tasks for a spec are done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., `sgf build` + `sgf issues plan`) can run concurrently on the same branch. The pensa daemon serializes all database access, providing atomic claims via `pn update --claim` (fails with `already_claimed` if another agent got there first). `pn export` runs at commit time via the pre-commit hook. If `git push` fails due to a concurrent commit, the loop should `git pull --rebase` and retry. Stop build loops before running `sgf spec` to avoid task-supersession race conditions.

### Standard Loop Iteration

Build, Test, and Issues Plan stages share a common iteration pattern. Each iteration:

1. **Orient** — read `memento.md`
2. **Query** — find work items via pensa (stage-specific query, see table). If none, write `.ralph-complete` and exit — the loop is finished.
3. **Choose & Claim** — pick a task from the results, then `pn update <id> --claim`. If the claim fails (`already_claimed`), re-query and pick another.
4. **Work** — stage-specific (see below)
5. **Log issues** — if problems are discovered: `pn create "description" -t bug`
6. **Close/release** — close or release the work item
7. **Commit** — commit all changes (the pre-commit hook runs `pn export` automatically, syncing SQLite to JSONL)

Each iteration gets fresh context. The pensa database persists state between iterations. The stages differ only in their query, work, and close steps:

| Stage | Query | Work | Close |
|-------|-------|------|-------|
| Build | `pn ready [--spec <stem>] --json` | Implement the task; apply backpressure (build, test, lint per `.sgf/backpressure.md`) | `pn close <id> --reason "..."` |
| Test | `pn ready -t test [--spec <stem>] --json` | Execute the test | `pn close <id> --reason "..."` |
| Issues Plan | `pn list -t bug --status open --json` | Study codebase, create fix task: `pn create -t task "fix: ..." --fixes <bug-id>` | `pn release <id>` (bug stays open) |

### 1. Spec (`sgf spec`)

Opens a Claude Code session with the spec prompt. The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates both deliverables:

1. Write spec files to `specs/`
2. Update `specs/README.md` with new index entries (loom-style `| Spec | Code | Purpose |` rows)
3. Create implementation plan items via `pn create -t task --spec <stem>`, with dependencies and priorities
4. Commit and push

The interview and generation happen in a single session. The agent asks clarifying questions as needed, but the goal is always to produce specs and a plan. The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line.

Tasks linked to a spec *are* the implementation plan (see Schema). Query with `pn list -t task --spec <stem>`.

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

Follows the standard loop iteration. Runs a Ralph loop using `.sgf/prompts/issues-plan.md`. A separate concurrent process from `sgf build`.

Unlike build and test, the close step uses `pn release <id>` — the bug stays open while the fix task (created with `--fixes`) flows through `pn ready` into the build loop.

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

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt must enforce: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**One task, fresh context**: Each iteration picks one unblocked task, implements it fully, commits, and exits. The loop restarts with a clean context window. No accumulated confusion, no multi-task sprawl.

**Atomic iterations**: An iteration either commits fully or is discarded entirely. Partial work from crashed iterations is never preserved — sgf's pre-launch recovery wipes uncommitted state before the next run.

Remaining principles — structured memory, tasks-as-plan, editable prompts, thin memento, protected scaffolding, decentralized projects, sandboxed execution, backpressure — are defined in their respective sections above.

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