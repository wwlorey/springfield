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
