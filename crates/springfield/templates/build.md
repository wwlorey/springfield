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
