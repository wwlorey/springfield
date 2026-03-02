Run `pn ready -t test --spec {{spec}} --json`.

If no test items are returned:
1. Generate `test-report.md` — summarize all test results, pass/fail status, and any bugs logged.
2. `touch .ralph-complete` and stop.

Otherwise, claim ONE test item per `@.sgf/PENSA.md`.

Execute the test. Use subagents.

IMPORTANT:
- **Use specs as guidance.** Follow the design patterns and expected behavior defined in the relevant spec.
- **Author property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- If **newly authored**, routine tests are **unreasonably slow**, consider using **fast params (or mock params, whichever is best, as long as testing is solid)** and gate the slow production-param tests behind `#[ignore]`.
- **After making changes, apply FULL BACKPRESSURE (see `@.sgf/BACKPRESSURE.md`) to verify behavior.**
- If you come across bugs or failures, log them: `pn create "description" -t bug`
