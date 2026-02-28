study specs/README.md
study springfield-implementation-plan.md

If **ALL** steps in the above implementation plan are complete, `touch .ralph-complete` and end.

Otherwise, choose ONE best next task from the above implementation plan and implement it. Use subagents.


NOTE:
- Make sure if you change any build flags, etc., to work on Linux that you make the DEFAULT run on Mac (for instance: building with Metal enabled). Mac is what we're ultimately building for.
- When implementing, build a tiny, end-to-end slice of the feature first, test and validate it, then expand out from there.
  * (Tracer bullets comes from the Pragmatic Programmer. When building systems, you want to write code that gets you feedback as quickly as possible. Tracer bullets are small slices of functionality that go through all layers of the system, allowing you to test and validate your approach early. This helps in identifying potential issues and ensures that the overall architecture is sound before investing significant time in development.)
- If **newly authored**, routine tests are **unreasonably slow**, consider using **fast params (or mock params, whichever is best, as long as our testing is solid)** and gate the slow production-param tests behind `#[ignore]` (See AGENTS.md).

IMPORTANT:
- **Assume NOT implemented.** Many specs describe planned features that may not yet exist in the codebase.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Do not implement placeholder code.** We want full, real implementations.
- **Author property based tests, unit tests, and/or integration tests.** (Whichever is best.)
- **After making changes to the files apply FULL BACKPRESSURE to verify behavior (see AGENTS.md).**
- When the ONE task is done:
  * **Update the implementation plan:**
    + Track your progress.
    + Add crucial lessons learned—only information you wish you had known before (if any).
    + Add notable design and/or testing decisions made (if any).
  * **Commit your changes.**
