# pn — Exclusive Work Item (Issue) Tracker

`pn` (pensa) is the exclusive work item (issue) tracker. Never use TodoWrite, TaskCreate, or markdown files for tracking work.

## Rules

- Always pass `--json` when reading data.

## Core Commands

| Command | Purpose |
|---------|---------|
| `pn ready [--spec <stem>] [-t <type>] --json` | List unblocked, unclaimed work items |
| `pn list [-t <type>] [--status <s>] [--spec <stem>] --json` | List items with filters |
| `pn show <id> --json` | Show item details |
| `pn create "<title>" -t <type> [--spec <stem>] [-p <priority>] [--dep <id>] [--fixes <id>]` | Create item (types: task, test, bug, chore) |
| `pn update <id> --claim` | Atomically claim an item |
| `pn update <id> --unclaim` | Release a claim |
| `pn close <id> --reason "<reason>"` | Close an item |
| `pn release <id>` | Release without closing |
| `pn comment add <id> "<text>"` | Add a comment |
| `pn dep add <id> --dep <other-id>` | Add a dependency |

## Claim Workflow

1. Query for issues (e.g., `pn ready --spec auth --json`).
2. **If there are no issues returned from `pn ready`, there are no available issues to claim right now.**
3. Pick ONE issue and claim: `pn update <id> --claim`.
4. If claim fails (`already_claimed`) → re-query and pick another.

## Logging Bugs

`pn create "<description>" -t bug`

## Bug Planning (when `pn ready` returns a bug)

1. Claim the bug: `pn update <id> --claim`
2. Study the codebase, create fix task(s): `pn create -t task "fix: ..." --fixes <bug-id> [--spec <stem>]`
3. Comment lessons learned: `pn comment add <bug-id> "..."`
4. Release the bug: `pn release <bug-id>`
5. Commit with `[<bug-id>]` prefix

## Closing Work

1. Comment with (1) crucial lessons learned and/or (2) notable design/testing decisions made (if any; only if useful to future agents): `pn comment add <id> "<insights>"`
2. Close: `pn close <id> --reason "<what was done>"`
3. Commit with `[<task-id>]` prefix (e.g., `[pn-a1b2c3d4] Implement login validation`)
