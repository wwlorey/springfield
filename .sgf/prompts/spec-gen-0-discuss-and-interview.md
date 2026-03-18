(You are in the DISCUSS AND INTERVIEW phase of the spec genesis cursus.)

Let's have a discussion and you can interview me about what I want to build.

Read the pertinent spec(s) sections that are involved in these changes as they come up in our conversation so you have the proper context for what we're talking about.

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
