## Specs

If `specs/` exists in a repo, this houses the specifications for that codebase and is the **source of truth**. `specs/README.md` is the spec lookup table.

## Sandbox

When sandboxed:

- Use relative paths (from the repo root) for file operations, not absolute paths. The sandbox allows writes to . but not to absolute paths outside the explicit allowlist.

## pn — Task & Issue Tracker

`pn` (pensa) is the exclusive task and issue tracker. Never use TodoWrite, TaskCreate, or markdown files for tracking work.

### Rules

- Always pass `--json` when reading data.
- There is NO `pn claim` subcommand. Use `pn update <id> --claim`.
- Status values use **underscores**: `open`, `in_progress`, `closed`. Never use hyphens (`in-progress` is invalid).

### Issue Claim Workflow

1. Query for issues (e.g., `pn ready --json` or `pn ready --spec auth --json`).
   IMPORTANT: **Do NOT run `pn list` to see open issues. Use `pn ready`.**
2. **If there are no issues returned from `pn ready`, there are no available issues to claim right now.**
3. Pick ONE issue and claim: `pn update <id> --claim`.
4. If claim fails (`already_claimed`) → re-query and pick another.
  a. NOTE: Do NOT work on an already-claimed issue.
    i. (Even if it is claimed under your name.)

### Issue Close Workflow

1. Comment on the issue (`pn comment add <id> "<insights>"`):
  a. crucial, useful lessons learned (if any)
  b. notable design/testing decisions made (if any)
2. Close or release:
  a. IF BUG: release with `pn release <bug-id>`
  b. ELSE: close with `pn close <id> --reason "<what was done>"`
3. Commit your changes with `[<task-id>]` prefix (e.g., `[pn-a1b2c3d4] Implement login validation`)

### Bug Logging Workflow

`pn create "<description>" -t bug`

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

### Priorities

| Priority | Meaning | When to use |
|----------|---------|-------------|
| `p0` | Critical | Blocking all progress — broken builds, data loss, security holes |
| `p1` | High | Important and urgent — should be picked before p2/p3 work |
| `p2` | Normal | Default. Standard implementation tasks, tests, non-urgent bugs |
| `p3` | Low | Nice-to-have — polish, minor improvements, can wait indefinitely |

