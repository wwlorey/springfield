Follow the `pn` Issue Claim Workflow to choose **ONE** best next issue to implement.

IF there are no more issues:
- touch `.iter-complete` and end.

ELSE IF the claimed item is a **bug** (`issue_type == "bug"`):
- Follow the Bug Fix Workflow.

ELSE:
- Implement the issue. Use subagents.

NOTE:
- Make sure if you change any build flags, etc., to work on Linux that you make the DEFAULT run on Mac (for instance: building with Metal enabled).
- When implementing, build a tiny, end-to-end slice of the feature first, test and validate it, then expand out from there (cf. tracer bullets).
- If **newly authored**, routine tests are **unreasonably slow**, consider using **fast params (or mock params, whichever is best, as long as our testing is solid)** and gate the slow production-param tests behind `#[ignore]` (See AGENTS.md).
- If you come across build, lint, etc. errors that you did not cause, log them using `pn`.

IMPORTANT:
- **Assume NOT implemented.** Many specs describe planned features that may not yet exist in the codebase.
- **Use specs as guidance.** When implementing a feature, follow the design patterns, types, and architecture defined in the relevant spec.
- **Do not implement placeholder code.** We want full, real implementations.
- **Author PROPERTY BASED TESTS and/or UNIT TESTS** (whichever is best).
- **After making changes to the files apply FULL BACKPRESSURE to verify behavior.**
- **If you attempt to claim a `pn` issue and is already claimed, DO NOT WORK ON IT, even if it is claimed by you.**
- When the ONE issue is done:
  * Follow the `pn` Issue Close Workflow.
  * Stop working.
