# pn — Task & Issue Tracker

`pn` (pensa) is the exclusive task and issue tracker. Never use TodoWrite, TaskCreate, or markdown files for tracking work.

## Rules

- Always pass `--json` when reading data.
- One task per iteration. Claim → work → close → commit.

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

1. Query for work (e.g., `pn ready --spec auth --json`).
2. If nothing returned → `touch .ralph-complete` and stop.
3. Pick ONE item and claim: `pn update <id> --claim`.
4. If claim fails (`already_claimed`) → re-query and pick another.

## Logging Bugs

When you discover issues you didn't cause: `pn create "<description>" -t bug`

## Closing Work

1. Comment with lessons learned: `pn comment add <id> "<insights>"` (only if useful to future agents)
2. Close: `pn close <id> --reason "<what was done>"`
3. Commit with `[<task-id>]` prefix (e.g., `[pn-a1b2c3d4] Implement login validation`)
