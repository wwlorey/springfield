## pn — Issue Tracker

`pn` (pensa) is the exclusive issue (i.e. work item) tracker. Never use TodoWrite, TaskCreate, or markdown files for tracking work.

### Rules

- Always pass `--json` when reading data.
- There is NO `pn claim` subcommand. Use `pn update <id> --claim`.
- Status values use **underscores**: `open`, `in_progress`, `closed`. Never use hyphens (`in-progress` is invalid).

### Issue Create Workflow

1. Create the issue linked to its spec:
   `pn create "<title>" -t <type> --spec <stem> [-p <priority>] [--dep <id>] [--description "<desc>"]`
   a. NOTE: Issues should be scoped to atomic changes—the smallest self-contained modifications to the codebase that can be implemented and tested independently.
2. Attach source code references (files to view/change/add):
   `pn src-ref add <id> <path> --reason "<what and why>"`
3. Attach documentation references (docs to view/change/add):
   `pn doc-ref add <id> <path> --reason "<what and why>"`

### Issue Claim Workflow

1. Query for issues (e.g., `pn ready --json` or `pn ready --spec auth --json`).
  a. IMPORTANT: **Do NOT run `pn list` to see open issues. Use `pn ready`.**
2. **If there are no issues returned from `pn ready`, there are no available issues to claim right now.**
3. Pick ONE issue and claim: `pn update <id> --claim`.
4. If claim fails (`already_claimed`) → re-query and pick another.
  a. NOTE: Do NOT work on an already-claimed issue.
    i. (Even if it is claimed under your name.)

### Issue Close Workflow

1. Comment on the issue (`pn comment add <id> "<insights>"`):
  a. crucial, useful lessons learned (if any)
  b. notable design/testing decisions made (if any)
  c. root cause of issue (if applicable)
2. Close or release:
  a. IF BUG: release with `pn release <bug-id>`
  b. ELSE: close with `pn close <id> --reason "<what was done>"`
3. Commit YOUR changes with `[<issue-id>]` prefix (e.g., `[pn-a1b2c3d4] Implement login validation`)

### Bug Log Workflow

`pn create "<description>" -t bug`

### Bug Fix Workflow

- Study the codebase to understand the bug.
- IF the fix is small enough to quickly implement in this iteration:
  1. Fix it.
  2. Follow the Spec Update Workflow as appropriate.
- ELSE IF the fix is too large (multiple files/crates, significant refactor):
  1. Follow the Issue Create Workflow to create implementation items.
    a. Link the relevant bug with `--fixes <bug-id>`.
  2. Release the bug: `pn release <bug-id>`

### Core Commands

| Command | Purpose |
|---------|---------|
| `pn ready [--spec <stem>] [-t <type>] --json` | List unblocked, unclaimed work items |
| `pn list [-t <type>] [--status <s>] [--spec <stem>] --json` | List items with filters |
| `pn blocked --json` | List blocked items |
| `pn show <id> --json` | Show item details |
| `pn search "<query>" --json` | Full-text search across issues |
| `pn count [--by-status] [--by-priority] [--by-issue-type] [--by-assignee] --json` | Count/summarize items |
| `pn status --json` | Project status overview |
| `pn create "<title>" -t <type> [--spec <stem>] [-p <priority>] [--dep <id>] [--fixes <id>] [--description <desc>]` | Create item (types: task, test, bug, chore) |
| `pn update <id> --claim` | Atomically claim an item |
| `pn update <id> --unclaim` | Release a claim |
| `pn update <id> --assignee <name>` | Set assignee (not `--assign`) |
| `pn update <id> --status <status>` | Set status: `open`, `in_progress`, `closed` (underscores, not hyphens) |
| `pn close <id> --reason "<reason>" [--force]` | Close an item |
| `pn reopen <id> [--reason "<reason>"]` | Reopen a closed item |
| `pn release <id>` | Release without closing |
| `pn delete <id> [--force]` | Delete an item |
| `pn history <id> --json` | Show item change history |
| `pn comment add <id> "<text>"` | Add a comment |
| `pn comment list <id> --json` | List comments on an item |
| `pn dep add <id> --dep <other-id>` | Add a dependency |
| `pn dep remove <id> --dep <other-id>` | Remove a dependency |
| `pn dep list <id> --json` | List dependencies |
| `pn dep tree <id> --json` | Show dependency tree |
| `pn dep cycles --json` | Detect dependency cycles |
| `pn src-ref add <id> <path> [--reason "<text>"]` | Add source code reference to an issue |
| `pn src-ref list <id> --json` | List source code references |
| `pn src-ref remove <ref-id>` | Remove a source code reference |
| `pn doc-ref add <id> <path> [--reason "<text>"]` | Add documentation reference to an issue |
| `pn doc-ref list <id> --json` | List documentation references |
| `pn doc-ref remove <ref-id>` | Remove a documentation reference |

### Priorities

| Priority | Meaning | When to use |
|----------|---------|-------------|
| `p0` | Critical | Blocking all progress — broken builds, data loss, security holes |
| `p1` | High | Important and urgent — should be picked before p2/p3 work |
| `p2` | Normal | Default. Standard implementation tasks, tests, non-urgent bugs |
| `p3` | Low | Nice-to-have — polish, minor improvements, can wait indefinitely |



## fm — Specification Management

Specifications are the **source of truth** for all code. They are managed exclusively through `fm` (forma).

All spec mutations go through `fm`—never edit spec markdown directly. The generated `.forma/specs/*.md` and `.forma/README.md` are read-only artifacts produced by `fm export`.

### How `fm` relates to `pn`

- `pn create --spec <stem>` links an issue to a forma spec. Pensa validates the stem against forma.
- `fm check` cross-validates that all pensa issues with `--spec` values reference existing forma specs.

### Rules

- Always pass `--json` when reading data.
- Section bodies are read from **stdin** via `--body-stdin` (not as CLI arguments).
- Status values: `draft`, `stable`, `proven`.
- Specs are identified by **stem** (lowercase, alphanumeric + hyphens, e.g., `auth`, `claude-wrapper`).
- Sections are identified by **slug** (auto-generated from display name, e.g., `error-handling`).
- Required sections (`overview`, `architecture`, `dependencies`, `error-handling`, `testing`) are auto-scaffolded on `fm create` and cannot be removed.
- When passing body content to `fm section set --body-stdin`, always pipe raw content directly. Use `cat <file> |` or a Python/heredoc approach. Never use `echo "$var"` or unquoted shell expansion, as this can introduce backslash escaping artifacts.
- When updating and/or creating specs: **we are NOT documenting changes in the specs.** **Instead, we are updating or writing the specs to simply reflect the new content we agreed upon.** (e.g., We want "the 'hello world' crate prints 'hello world'," instead of "instead of printing 'goodbye world,' the 'hello world' crate now prints 'hello world.'")

### Spec Create Workflow

1. Create the spec: `fm create <stem> [--src <path>] --purpose "<text>"`
  a. NOTE: Favor updating existing specs (`fm update`, `fm section set`) over creating new ones unless doing so makes sense (e.g. we're making a brand new package — use `fm create`).
2. Fill in required sections (pipe body via stdin):
   `echo "body content" | fm section set <stem> "<slug>" --body-stdin`
3. Add custom sections as needed:
   `echo "body content" | fm section add <stem> "<name>" --body-stdin`
4. Add cross-references to related specs: `fm ref add <stem> <target-stem>`

### Spec Update Workflow

1. Read the current spec: `fm show <stem> --json`
2. Update metadata: `fm update <stem> [--status <s>] [--src <path>] [--purpose "<text>"]`
3. Update section bodies (pipe body via stdin):
   `echo "body content" | fm section set <stem> "<slug>" --body-stdin`

### Commands

#### Core Commands

| Command | Purpose |
|---------|---------|
| `fm create <stem> [--src <path>] --purpose "<text>"` | Create a new spec (scaffolds 5 required sections) |
| `fm show <stem> --json` | Show spec with all sections and refs |
| `fm list [--status <status>] --json` | List all specs, optionally filtered by status |
| `fm update <stem> [--status <s>] [--src <path>] [--purpose "<text>"]` | Update spec metadata |
| `fm delete <stem> [--force]` | Delete a spec (`--force` if sections have content) |
| `fm search "<query>" --json` | Case-insensitive search across stems, purposes, section bodies |
| `fm count [--by-status] --json` | Count specs |
| `fm status --json` | Summary of specs by status |
| `fm history <stem> --json` | Event log for a spec |

#### Section Commands

| Command | Purpose |
|---------|---------|
| `fm section add <stem> "<name>" --body-stdin [--after "<slug>"]` | Add custom section (body from stdin) |
| `fm section set <stem> "<slug>" --body-stdin` | Replace section body (body from stdin) |
| `fm section get <stem> "<slug>" --json` | Get a single section |
| `fm section list <stem> --json` | List all sections for a spec |
| `fm section remove <stem> "<slug>"` | Remove a custom section (required sections are protected) |
| `fm section move <stem> "<slug>" --after "<slug>"` | Reorder a section |

#### Ref Commands

| Command | Purpose |
|---------|---------|
| `fm ref add <stem> <target-stem>` | Add cross-reference (rejects cycles) |
| `fm ref remove <stem> <target-stem>` | Remove cross-reference |
| `fm ref list <stem> --json` | List specs this spec references |
| `fm ref tree <stem> [--direction up\|down] --json` | Recursive ref tree |
| `fm ref cycles --json` | Detect reference cycles |

#### Data & Maintenance Commands

| Command | Purpose |
|---------|---------|
| `fm export` | SQLite → JSONL + generated markdown, stages `.forma/` |
| `fm import` | JSONL → SQLite (used after clone/merge) |
| `fm check --json` | Validation report (required sections, src paths, refs, pensa integration) |
| `fm doctor [--fix] --json` | Health checks; `--fix` removes orphaned data |
| `fm where` | Print JSONL and DB directory paths |



## IMPORTANT

- **Use relative paths—from the repo root—for file operations, not absolute paths.**
- **When spawning agents/subagents for autonomous tasks**:
  * Use `cl` instead of the Agent tool:
    + `cl -p --dangerously-skip-permissions --max-turns 50 "task description here"`
  * Run multiple in parallel via background bash calls.
  * Do NOT use your Agent tool or any other built-in functionality for spawning agents.
- **When asked about what has been built** IN GENERAL or ON A PARTICULAR DAY/TIME:
  * Read the logs in `./.sgf/logs` to help formulate your answer.

### Session Start

- **Run this command at the beginning of EACH SESSION** to understand the structure of this project's specifications:
  * `fm list --json` — list all specifications (the source of truth for implementation)
