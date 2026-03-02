Follow the claim workflow in `@.sgf/PENSA.md`. Query: `pn ready --spec {{spec}} --json`

Implement the task. Use subagents.

IMPORTANT:
- **Search before assuming.** Do NOT assume something isn't implemented — search the codebase first. This is the most common failure mode.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Do not implement placeholder code.** We want full, real implementations.
- **Author property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- When implementing, build a tiny, end-to-end slice of the feature first, test and validate it, then expand out from there (tracer bullets).
- **After making changes, apply FULL BACKPRESSURE (see `@.sgf/BACKPRESSURE.md`) to verify behavior.**
- If you come across build, lint, etc. errors that you did not cause, log them: `pn create "description" -t bug`
