(You are in the DISCUSS AND INTERVIEW phase of the precatur cursus.)

A list of `pn` bugs has been injected into your context.
1. Query for them using `pn` and
2. Study these issues and their fixes (i.e. the issues which fix the bugs).

Let's have a discussion and you can interview me about how:
1. The fixes for these bugs have led the codebase to diverge from the specs and
2. How the specs can be updated to align with the actuality of the codebase.

Read the pertinent spec(s) sections that are involved in these changes as they come up so you have the proper context for what we're talking about.

Be sure to come away from the conversation with the following information (the conversation SHOULD NOT END until all of this is KNOWN):
- Edge cases and failure modes.
- Error handling behavior.
- Interactions with existing specs and crates.
- Scope boundaries — what's in, what's out.
- Dependencies and integration points.
- How the result will be tested end-to-end from the CLI.
- Answers to all open questions.

IMPORTANT:
- Ask clarifying questions.
- Push back on ambiguity.
- Surface trade-offs. 
- Provide pros/cons when presenting options.
- When asking clarifying questions or presenting information, NUMBER THE ITEMS so the user can reference them.

WHEN THE USER CONFIRMS THAT YOU HAVE ENOUGH TO PROCEED:
1. Write a summary of the discussion to `$SGF_RUN_CONTEXT/discuss-and-interview-summary.md`.
  a. This summary will be consumed by the WRITE phase.
  b. Include:
    i. **Intent**: What we're building and why.
    ii. **Key decisions**: Design choices made during discussion, with rationale.
    iii. **Scope**: What's in and what's explicitly out.
    iv.  **Constraints**: Non-negotiable requirements, performance targets, compatibility needs.
    v.   **Open questions resolved**: Things that were unclear and how they were resolved.
    vi.  **Existing specs affected**: Which specs need updating or cross-referencing.
    vii. **Testing approach**: How this will be verified end-to-end from the CLI.
