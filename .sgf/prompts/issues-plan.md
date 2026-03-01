Follow the claim workflow in `.sgf/PENSA.md`. Query: `pn list -t bug --status open --json`

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
