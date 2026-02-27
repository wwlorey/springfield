# springfield Specification

CLI entry point for Springfield. All developer interaction goes through this binary. It handles project scaffolding, prompt assembly, loop orchestration, recovery, and daemon lifecycle. Delegates iteration execution to ralph and persistent memory to pensa.

## Overview

`sgf` provides:
- **Project scaffolding**: `sgf init` creates the full project structure (`.sgf/`, `.pensa/`, prompts, backpressure, memento, specs index, Claude deny settings, git hooks)
- **Prompt assembly**: Read templates, substitute variables, validate, write assembled prompts
- **Loop orchestration**: Launch ralph with the correct flags, manage PID files, tee logs
- **Recovery**: Pre-launch cleanup of dirty state from crashed iterations
- **Daemon lifecycle**: Start the pensa daemon before launching loops
- **Workflow commands**: `spec`, `build`, `verify`, `test-plan`, `test`, `issues log`, `issues plan`

---

## CLI Commands

```
sgf init                               — scaffold a new project
sgf spec                               — generate specs and implementation plan (interactive)
sgf build <spec> [-a] [--no-push] [N]  — run build loop
sgf verify [-a] [--no-push] [N]        — run verification loop
sgf test-plan [-a] [--no-push] [N]     — run test plan generation loop
sgf test <spec> [-a] [--no-push] [N]   — run test execution loop
sgf issues log                         — interactive session for logging bugs
sgf issues plan [-a] [--no-push] [N]   — run bug planning loop
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

Scaffolds a new project. Takes no arguments.

### What it creates

```
.pensa/                                (directory only — daemon creates db.sqlite on start)
.sgf/
├── backpressure.md                    (universal backpressure template)
├── logs/                              (empty, gitignored)
├── run/                               (empty, gitignored)
└── prompts/
    ├── build.md                       (prompt template)
    ├── spec.md
    ├── verify.md
    ├── test-plan.md
    ├── test.md
    ├── issues.md
    ├── issues-plan.md
    └── .assembled/                    (empty, gitignored)
.claude/settings.json                  (deny rules for .sgf/**)
.pre-commit-config.yaml                (prek hooks for pensa sync)
.gitignore                             (Springfield entries + stack-specific entries)
memento.md                             (skeleton reference document)
CLAUDE.md                              (links to memento + AGENTS.md)
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
.sgf/logs/
.sgf/run/
.sgf/prompts/.assembled/
.ralph-complete
.ralph-ding

# Rust
/target

# Node
node_modules/

# SvelteKit
.svelte-kit/
```

All entries are always added regardless of what exists in the directory. If an entry already exists anywhere in the file, it is not added again.

### Skeleton memento.md

```markdown
# Memento

## Stack

<!-- Replace with your project's stack (e.g., Rust, TypeScript, Tauri, Go) -->

## References

- Build, test, lint, format commands: `.sgf/backpressure.md`
- Spec index: `specs/README.md`
- Issue and task tracking: `pn` CLI (pensa)
```

### CLAUDE.md

```markdown
Read memento.md and AGENTS.md before starting work.
```

### specs/README.md

```markdown
# Specs

| Spec | Code | Purpose |
|------|------|---------|
```

### Idempotence

`sgf init` is safe to re-run. It skips files that already exist (prompts, memento, CLAUDE.md, specs/README.md) and only merges additive content (deny rules, git hooks, gitignore entries). It never overwrites existing content.

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
CLAUDE.md                      (links to memento + AGENTS.md)
test-report.md                 (generated — overwritten each test run, committed)
verification-report.md         (generated — overwritten each verify run, committed)
specs/
├── README.md                  (agent-maintained spec index — loom-style tables)
└── *.md                       (prose specification files)
```

### File Purposes

**`memento.md`** — A thin reference document the agent reads at the start of every iteration. Contains references to external files, not content itself. The agent reads it, follows the references to get detail, and dives into specific files only when needed. Auto-loaded via CLAUDE.md. Generated by `sgf init`, rarely changes after that — the files it references are what evolve.

**`specs/README.md`** — Agent-maintained spec index, matching the [loom format](https://github.com/ghuntley/loom/blob/trunk/specs/README.md). Categorized tables with `| Spec | Code | Purpose |` columns mapping each spec to its implementation location and a one-line summary. Agents update this file when adding or revising specs.

**`.sgf/backpressure.md`** — Build, test, lint, and format commands for the project. Generated by `sgf init` from a universal template (see [Backpressure Template](#backpressure-template)). Developer-editable after scaffolding. Agents must not modify this file — it is protected by Claude deny settings.

**`AGENTS.md`** — Hand-authored operational guidance. Contains information that doesn't fit the memento's structured format — code style preferences, runtime notes, special instructions. Not generated by `sgf init` — the developer creates this when needed.

**`CLAUDE.md`** — Entry point for Claude Code. Links to memento.md and AGENTS.md. Auto-loaded by Claude Code at the start of every session.

**`.sgf/prompts/`** — Editable prompt templates for each workflow stage. Seeded by `sgf init` from Springfield's built-in templates. Once seeded, the project owns these files — edit them to evolve the prompts. To improve defaults for future projects, update the templates in the Springfield repo.

**`.sgf/` protection** — The entire `.sgf/` directory is protected from agent modification via Claude deny settings. `sgf init` scaffolds these rules. Agents cannot write to prompts, backpressure, or config regardless of prompt instructions.

**`specs/`** — Prose specification files (one per topic of concern). Authored during the spec phase, consumed during builds. Indexed in `specs/README.md`.

---

## Prompt Assembly

`sgf` handles all prompt templating before invoking ralph. Ralph is template-unaware — it receives a final assembled prompt.

### Assembly Process

1. Read the template from `.sgf/prompts/<stage>.md`
2. Substitute variables — replace `{{var}}` tokens with their values
3. Validate — scan for unresolved `{{...}}` tokens and fail with an error before launching
4. Write the assembled prompt to `.sgf/prompts/.assembled/<stage>.md`
5. Pass the file path as ralph's `PROMPT` argument

### Template Variables

Templates use `{{var}}` syntax. The only variable is `spec`:

| Variable | Stages | Value | Example |
|----------|--------|-------|---------|
| `spec` | `build`, `test` | Spec stem from positional arg | `auth` |

Templates for other stages contain no variables and are passed through unchanged (but still written to `.assembled/` for consistency).

The `.assembled/` directory is gitignored. Assembled prompts persist for debugging — inspect what was actually sent to the agent.

---

## sgf-to-ralph Contract

### Invocation

```
sgf → ralph [-a] [--loop-id ID] [--template T] [--auto-push BOOL] [--max-iterations N] ITERATIONS PROMPT
```

`sgf` translates its own flags and hardcoded defaults into ralph CLI flags. Ralph does not read config files — all configuration arrives via flags.

### CLI Flags Passed to Ralph

| Flag | Type | Source | Description |
|------|------|--------|-------------|
| `-a` / `--afk` | bool | sgf command (e.g., `sgf build -a`) | AFK mode |
| `--loop-id` | string | sgf-generated | Unique loop identifier |
| `--template` | string | hardcoded: `ralph-sandbox:latest` | Docker sandbox template |
| `--auto-push` | bool | `true` unless `--no-push` passed to sgf | Auto-push after commits |
| `--max-iterations` | u32 | hardcoded: `30` | Safety limit |
| `ITERATIONS` | u32 | positional arg or default `30` | Number of iterations |
| `PROMPT` | path | `.sgf/prompts/.assembled/<stage>.md` | Assembled prompt file |

### Exit Codes

| Code | Meaning | sgf response |
|------|---------|--------------|
| `0` | Sentinel found (`.ralph-complete`) — loop completed | Log success, clean up |
| `1` | Error (bad args, missing prompt, etc.) | Log error, alert developer |
| `2` | Iterations exhausted — may have remaining work | Developer decides: re-launch or stop |
| `130` | Interrupted (SIGINT/SIGTERM) | Log interruption, clean up |

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

---

## Recovery

Ralph does not perform iteration-start cleanup. Recovery is `sgf`'s responsibility, executed before launching ralph.

### PID Files

`sgf` writes `.sgf/run/<loop-id>.pid` on launch (containing its process ID) and removes it on clean exit. The `.sgf/run/` directory is gitignored.

### Pre-launch Cleanup

Before launching ralph, `sgf` scans all PID files in `.sgf/run/`:

- **Any PID alive** (verified via `kill -0`) → another loop is running. Skip cleanup and launch normally — the dirty tree or in-progress claims may belong to that loop.
- **All PIDs stale** (process dead) → no loops are running. Remove stale PID files, then recover:
  1. `git checkout -- .` — discard modifications to tracked files
  2. `git clean -fd` — remove untracked files (respects `.gitignore`, so `db.sqlite`, logs, and assembled prompts are safe)
  3. `pn doctor --fix` — release stale claims and repair integrity

**Principle**: Work is only preserved when committed. Uncommitted changes from crashed iterations are discarded — the agent that produced them is gone and cannot continue them.

---

## Daemon Lifecycle

`sgf` starts the pensa daemon automatically before launching any loop (if not already running):

1. Check if the daemon is reachable (`pn daemon status`)
2. If not, start it: `pn daemon --project-dir <project-root> &` (backgrounded)
3. Wait for readiness (poll `pn daemon status` with short timeout)
4. Proceed with loop launch

The daemon runs for the duration of the `sgf` session. It stops on SIGTERM or when `sgf` shuts down.

---

## Workflow Stages

**Stage transitions are human-initiated.** The developer decides when to move between stages. Suggested heuristics: run verify when `pn ready --spec <stem>` returns nothing (all tasks for a spec are done); run test-plan after verify passes; run test after test-plan produces test items. These are guidelines, not gates.

**Concurrency model**: Multiple loops (e.g., `sgf build` + `sgf issues plan`) can run concurrently on the same branch. The pensa daemon serializes all database access, providing atomic claims via `pn update --claim` (fails with `already_claimed` if another agent got there first). `pn export` runs at commit time via the pre-commit hook. Concurrent sandboxes share the same git history via Mutagen file sync — push conflicts don't arise because each sandbox sees the other's commits within seconds. **Stop build loops before running `sgf spec`** to avoid task-supersession race conditions.

### Standard Loop Iteration

Build, Test, and Issues Plan stages share a common iteration pattern. Each iteration:

1. **Orient** — read `memento.md`
2. **Query** — find work items via pensa (stage-specific query). If none, write `.ralph-complete` and exit.
3. **Choose & Claim** — pick a task from the results, then `pn update <id> --claim`. If the claim fails (`already_claimed`), re-query and pick another.
4. **Work** — stage-specific implementation
5. **Log issues** — if problems are discovered: `pn create "description" -t bug`
6. **Close/release** — close or release the work item
7. **Commit** — prefix the commit message with `[<task-id>]` (e.g., `[pn-a1b2c3d4] Implement login validation`). The pre-commit hook runs `pn export` automatically.

Each iteration gets fresh context. The pensa database persists state between iterations.

| Stage | Query | Work | Close |
|-------|-------|------|-------|
| Build | `pn ready --spec <stem> --json` | Implement the task; apply backpressure | `pn close <id> --reason "..."` |
| Test | `pn ready -t test --spec <stem> --json` | Execute the test | `pn close <id> --reason "..."` |
| Issues Plan | `pn list -t bug --status open --json` | Study codebase, create fix task with `--fixes` | `pn release <id>` (bug stays open) |

### 1. Spec (`sgf spec`)

Opens an interactive Claude Code session with the spec prompt. The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates deliverables:

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

### 2. Build (`sgf build <spec>`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/build.md`. Requires a spec stem — `sgf build auth` builds tasks for the `auth` spec.

`sgf` assembles the prompt by substituting `{{spec}}` in the build template. The build stage adds **backpressure** — after implementing the task, the agent runs build, test, and lint commands per `.sgf/backpressure.md`.

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

### 5. Test (`sgf test <spec>`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/test.md`. Requires a spec stem — `sgf test auth` runs test items for the `auth` spec.

After all test items are closed, a final iteration generates `test-report.md` — a summary of all test results, pass/fail status, and any bugs logged.

### 6. Issues Log (`sgf issues log`)

Runs via ralph using `.sgf/prompts/issues.md`. Each iteration handles one bug:

1. The developer describes a bug they've observed
2. The agent interviews them to capture details — steps to reproduce, expected vs actual behavior, relevant context
3. Logs the bug via `pn create -t bug`
4. The session exits and a fresh one spawns

One bug per iteration, fresh context each time.

### 7. Issues Plan (`sgf issues plan`)

Follows the standard loop iteration. Runs via ralph using `.sgf/prompts/issues-plan.md`. A separate concurrent process from `sgf build`.

Unlike build and test, the close step uses `pn release <id>` — the bug stays open while the fix task (created with `--fixes`) flows through `pn ready` into the build loop.

**Typical setup**: two terminals — `sgf issues plan` producing fix tasks from bugs, `sgf build` consuming them alongside other work items. A third terminal with `sgf issues log` for the developer to report new bugs.

### 8. Inline Issue Logging

Issues are also logged by agents during any stage via `pn create`. The build loop logs bugs it discovers during implementation. The verify loop logs spec gaps. The test loop logs test failures. `sgf issues log` is for developer-reported bugs; inline logging is for agent-discovered bugs.

---

## Prompt Templates

Each workflow stage has a corresponding prompt template in `.sgf/prompts/`. These are the default contents that `sgf init` writes. The project owns these files after scaffolding — edit them freely.

### spec.md

```markdown
Read `memento.md`.

Let's have a discussion and you can interview me about what I want to build.

---

After the discussion, produce the following deliverables:

1. Write spec files to `specs/`
2. Update `specs/README.md` with new entries in this format: `| Spec | Code | Purpose |`
   Reference: https://github.com/ghuntley/loom/blob/trunk/specs/README.md
3. Create implementation plan items via pensa:
   `pn create -t task --spec <stem> "task title" [-p <priority>] [--dep <id>]`
   Set dependencies between tasks where order matters.

The implementation plan should BEGIN with:
1. Any project-specific tooling setup needed

And the implementation plan should END with:
1. Documentation tasks (README.md, etc. as appropriate)
2. Integration test tasks that verify the feature works end-to-end

IMPORTANT:
- **The spec should be designed so that the result can be end-to-end tested from the command line.** If more tools are required to achieve this, make that known.
- **Commit and push the changes when finished.**
```

### build.md

```markdown
Read `memento.md`.

Run `pn ready --spec {{spec}} --json`.

If no tasks are returned, `touch .ralph-complete` and stop.

Otherwise, choose ONE task and claim it: `pn update <id> --claim`
If the claim fails (`already_claimed`), re-run `pn ready --spec {{spec}} --json` and pick another.

Implement the task. Use subagents.

IMPORTANT:
- **Search before assuming.** Do NOT assume something isn't implemented — search the codebase first. This is the most common failure mode.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Do not implement placeholder code.** We want full, real implementations.
- **Author property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- When implementing, build a tiny, end-to-end slice of the feature first, test and validate it, then expand out from there (tracer bullets).
- **After making changes, apply FULL BACKPRESSURE (see `.sgf/backpressure.md`) to verify behavior.**
- If you come across build, lint, etc. errors that you did not cause, log them: `pn create "description" -t bug`
- When the ONE task is done:
  * Add crucial lessons learned as a comment: `pn comment add <id> "..."` (only information you wish you had known before, if any)
  * Close the task: `pn close <id> --reason "..."`
  * **Commit the changes**, prefixing the message with `[<task-id>]` (e.g., `[pn-a1b2c3d4] Implement login validation`).
```

### verify.md

```markdown
Read `memento.md`.
Read `specs/README.md`.

If `verification-report.md` exists, read it.

If **ALL** specs listed in `specs/README.md` have been verified in the report (whether they match or are missing), `touch .ralph-complete` and stop.

Otherwise, choose ONE unverified spec and investigate (a) whether it is actually implemented in the codebase and (b) how well it matches the spec.

1. If it matches the spec, mark it as ✅ (Matches spec)
2. If it is a partial match, mark it as ⚠️ (Partial match / minor discrepancies)
3. If it is missing or very different, mark it as ❌ (Missing or significantly different)

For any gaps or issues found, log them: `pn create "description" -t bug`

Update `verification-report.md` with your findings and update the **Recommendations** section as appropriate.

When the ONE spec has been verified, **commit the changes.**
```

### test-plan.md

```markdown
Read `memento.md`.
Read `specs/README.md`.

Study the specs and codebase. Generate a testing plan.

For each test, create a pensa item:
`pn create -t test --spec <stem> "test title" [-p <priority>] [--dep <id>]`

IMPORTANT:
- Tests must be automatable — they will be run by agents in loops.
- Tests should be end-to-end testable from the command line.
- Set dependencies between test items where order matters.
- **Commit and push the changes when finished.**
- When all test items have been created, `touch .ralph-complete` and stop.
```

### test.md

```markdown
Read `memento.md`.

Run `pn ready -t test --spec {{spec}} --json`.

If no test items are returned:
1. Generate `test-report.md` — summarize all test results, pass/fail status, and any bugs logged.
2. `touch .ralph-complete` and stop.

Otherwise, choose ONE test item and claim it: `pn update <id> --claim`
If the claim fails (`already_claimed`), re-run `pn ready -t test --spec {{spec}} --json` and pick another.

Execute the test. Use subagents.

IMPORTANT:
- **Use specs as guidance.** Follow the design patterns and expected behavior defined in the relevant spec.
- **Author property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- If **newly authored**, routine tests are **unreasonably slow**, consider using **fast params (or mock params, whichever is best, as long as testing is solid)** and gate the slow production-param tests behind `#[ignore]`.
- **After making changes, apply FULL BACKPRESSURE (see `.sgf/backpressure.md`) to verify behavior.**
- If you come across bugs or failures, log them: `pn create "description" -t bug`
- When the ONE test is done:
  * Add crucial lessons learned as a comment: `pn comment add <id> "..."` (if any)
  * Close the test item: `pn close <id> --reason "..."`
  * **Commit the changes**, prefixing the message with `[<task-id>]`.
```

### issues.md

```markdown
Read `memento.md`.

Have a discussion and interview me about a new bug I want to report.

Information I might provide includes screenshots, logs, and descriptions. Your role is to capture the bug with enough detail for another agent to understand and fix it.

Ask clarifying questions:
- Steps to reproduce
- Expected vs actual behavior
- Relevant context (which feature, which spec, error messages)

When you have enough detail, log the bug:
`pn create "descriptive title" -t bug [-p <priority>] [--description "detailed description"]`

**Commit the changes.**
```

### issues-plan.md

```markdown
Read `memento.md`.

Run `pn list -t bug --status open --json`.

If no open bugs are returned, `touch .ralph-complete` and stop.

Otherwise, choose ONE bug and claim it: `pn update <id> --claim`
If the claim fails (`already_claimed`), re-run `pn list -t bug --status open --json` and pick another.

Study the codebase to understand the bug and design a fix. Use subagents.

Create a fix task:
`pn create -t task "fix: <description>" --fixes <bug-id> [--spec <stem>] [-p <priority>] [--dep <id>]`

IMPORTANT:
- **Use specs as guidance.** When designing a fix, follow the design patterns, types, and architecture defined in the relevant spec.
- **Plan for property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- When the ONE bug has been planned:
  * Add lessons learned and design decisions as a comment on the bug: `pn comment add <id> "..."`
  * Release the bug (it stays open — the fix task flows into the build loop): `pn release <id>`
  * Log as new bugs any other issues you've discovered: `pn create "description" -t bug`
  * **Commit the changes**, prefixing the message with `[<bug-id>]`.
```

---

## Backpressure Template

The following is the full content of `.sgf/backpressure.md` that `sgf init` writes. The developer deletes sections that don't apply to their project and edits commands as needed.

````markdown
# Backpressure — Building, Testing, Linting, Formatting, Integration Tests, and Code Scanning

After making changes, apply FULL BACKPRESSURE to verify behavior.

---

## Backend (Rust)

- **Build all:** `cargo build --workspace`
- **Build single:** `cargo build -p <crate>` (e.g., `cargo build -p my-crate`)
- **Test all:** `cargo test --workspace`
- **Test single:** `cargo test -p <crate> <test_name>` (e.g., `cargo test -p my-crate test_login`)
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all`
- **Detect unsafe code usage:** `cargo geiger`

### Long Running Tests

Some tests are gated behind `#[ignore]` because they use expensive operations (e.g., production Argon2 params, real LLM inference). These tests validate production behavior but are too slow for routine development.

- **Run ignored tests:** `cargo test -p <crate> <test_name> -- --ignored`
- **Run all tests including ignored:** `cargo test --workspace -- --ignored`

### Model-Dependent Tests (Requires Downloaded Models)

Some ignored tests require large models to be present. The model may be auto-downloaded on first build.

```bash
cargo test -p <crate> -- --ignored --test-threads=1
```

These tests run LLM inference on CPU and are slow (~2-10 min per test). Use `--test-threads=1` to avoid memory exhaustion from multiple model instances.

---

## Frontend (Tauri, SvelteKit)

> Stack: TypeScript, Svelte 5, SvelteKit (static adapter), Vitest, @testing-library/svelte, WebdriverIO
>
> **Working directory:** adjust as needed (all frontend commands run from the frontend directory)

- **Build frontend:** `pnpm build`
- **Build Tauri app:** `pnpm tauri build`
- **Build Tauri app (debug):** `pnpm tauri build --debug`
- **Unit tests:** `pnpm vitest run`
- **Unit tests (watch):** `pnpm vitest`
- **Unit test single file:** `pnpm vitest run <path>` (e.g., `pnpm vitest run src/lib/components/Auth/LoginScreen.test.ts`)
- **Type check:** `pnpm tsc --noEmit`
- **Svelte check:** `pnpm svelte-check --tsconfig ./tsconfig.json`
- **Lint:** `pnpm lint`
- **Lint fix:** `pnpm lint:fix`
- **Format:** `pnpm format`
- **Format check:** `pnpm format:check`

### E2E Tests (Linux Only)

E2E tests run on **Linux only** using WebKitWebDriver. macOS is not supported for E2E testing (no WebDriver access to WKWebView).

**Linux prerequisites:**
```bash
sudo apt-get install webkit2gtk-driver libwebkit2gtk-4.1-dev
```

**Running E2E tests:**
- **E2E tests (debug build, default):** `BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js`
- **E2E tests (release build):** `WDIO_RELEASE=1 BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js`
- **E2E single test:** `BUDDY_MOCK_AUDIO=1 pnpm wdio run wdio.conf.js --spec e2e/auth.test.ts`

**Environment variables:**
- `BUDDY_MOCK_AUDIO=1` - Required for recording tests (uses mock audio file instead of real microphone)
- `BUDDY_MOCK_LLM=1` - Use canned LLM responses (fast, for CI)
- `BUDDY_E2E_ISOLATED=1` - Clear app data before test suite (for full isolation)
- `WDIO_RELEASE=1` - Use release build instead of debug (default is debug for faster iteration)

---

## Frontend (SvelteKit, Vite)

> Stack: JavaScript, Svelte, Vitest, Playwright

- **Build:** `pnpm run build`
- **Unit tests:** `pnpm run test`
- **Unit tests (watch):** `pnpm run test:watch`
- **Unit test single file:** `pnpm vitest run <path>` (e.g., `pnpm vitest run src/lib/stores/progress.test.js`)
- **E2E tests:** `pnpm run test:e2e`
- **Lint:** `pnpm run lint`
- **Lint fix:** `pnpm run lint:fix`
- **Format:** `pnpm run format`
- **Format check:** `pnpm run format:check`
- **Validate data:** `pnpm run validate:data`
- **Full check:** `pnpm run check`
````

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
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    curl \
    wget \
    file \
    libxdo-dev \
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

# Install Tauri CLI
RUN . "$CARGO_HOME/env" && cargo install tauri-cli

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
    @tauri-apps/cli

# Install pensa CLI
USER root
COPY pn /usr/local/bin/pn
RUN chmod +x /usr/local/bin/pn

# Ensure agent owns their home directory
RUN chown -R agent:agent /home/agent

USER agent
WORKDIR /home/agent

# Verify installations
RUN rustc --version && cargo --version && node --version && pnpm --version && pn --help
```

### sgf template build

Builds the `ralph-sandbox:latest` Docker image:

1. Locate the `pn` binary (via `which pn`)
2. Create a temporary build context directory
3. Write the Dockerfile (embedded in the sgf binary at compile time via `include_str!`)
4. Copy the `pn` binary into the build context
5. Run `docker build -t ralph-sandbox:latest .`
6. Clean up the temporary directory

After updating the `pn` binary, re-run `sgf template build` to pick up the new version in the sandbox.

### Sandbox Behavior

- **File sync**: Workspace directory syncs bidirectionally between host and sandbox at the same absolute path via Mutagen. Changes the agent makes sync back to the host.
- **Credentials**: Docker Desktop credential proxy (`--credentials host`) injects API keys from the host. Keys never enter the sandbox.
- **Agent user**: The agent runs as non-root `agent` user with `sudo` access inside the sandbox.
- **Pensa access**: `pn` inside the sandbox connects to the host daemon via `http://host.docker.internal:7533`. The SQLite database never crosses the sync boundary.

---

## Hardcoded Defaults

| Setting | Value | Override |
|---------|-------|----------|
| Iterations | `30` | Positional arg: `sgf build auth 10` |
| Auto-push | `true` | `--no-push` flag |
| Docker template | `ralph-sandbox:latest` | None (constant) |
| Pensa daemon port | `7533` | None (constant) |

---

## Key Design Principles

**Search before assuming**: The agent must search the codebase before deciding something isn't implemented. Without this, agents create duplicate implementations. The build prompt enforces: "don't assume not implemented — search first." This is the single most common failure mode in Ralph loops.

**One task, fresh context**: Each iteration picks one unblocked task, implements it fully, commits, and exits. The loop restarts with a clean context window. No accumulated confusion, no multi-task sprawl.

**Atomic iterations**: An iteration either commits fully or is discarded entirely. Partial work from crashed iterations is never preserved — sgf's pre-launch recovery wipes uncommitted state before the next run.

**Structured memory over markdown**: Pensa replaces markdown-based issue logging and plan tracking. A single CLI command replaces the error-prone multi-step process of creating directories and writing files.

**Tasks as implementation plan**: There is no separate "implementation plan" entity. The living set of pensa tasks linked to a spec *is* the implementation plan. Query with `pn list -t task --spec <stem>`.

**Editable prompts**: Prompts are plain markdown files owned by the project. Edit them as you learn what works. To improve defaults, update Springfield's templates.

**Thin memento**: The memento is a table of contents, not a knowledge dump. It points to backpressure, specs, and pensa. The agent follows references to get detail.

**Protected scaffolding**: `.sgf/` is protected from agent writes via Claude deny settings. The developer is the authority on prompts, backpressure, and project configuration.

**Decentralized projects**: Springfield is project-aware — it reads `.sgf/` from the current working directory. No global registry. Each project is self-contained.

**Sandboxed execution**: All loops run inside Docker sandboxes for filesystem isolation with unrestricted network access. Credentials are injected via proxy, never entering the sandbox.

---

## Future Work

- **Context-efficient backpressure**: Swallow all build/test/lint output on success (show only a checkmark), dump full output only on failure. Preserves context window budget. See HumanLayer's `run_silent()` pattern.
- **Claude Code hooks for enforcement**: Use `PreToolUse` / `PostToolUse` hooks to enforce backpressure at the framework level — auto-run linters after file edits, block destructive commands. Could be scaffolded by `sgf init`.
- **TUI**: CLI-first for now. TUI can be added later as a view layer. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.
- **`sgf status` output spec**: Define what `sgf status` shows (running loops, pensa summary, recent activity). Specify after real usage reveals what's needed.
