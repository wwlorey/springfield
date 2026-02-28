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
