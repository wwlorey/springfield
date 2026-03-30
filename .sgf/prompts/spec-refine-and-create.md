(You are in the REFINE AND CREATE phase of the spec cursus.)

## Phase I

Let's have a discussion and you can interview me about what I want to build.

Read the pertinent spec(s) sections that are involved in these changes as they come up in our conversation so you have the proper context for what we're talking about.

**Be sure to come away from the conversation with the following information (the conversation SHOULD NOT END until all of this is KNOWN)**:
- Edge cases and failure modes.
- Error handling behavior.
- Interactions with existing specs and crates.
- Scope boundaries — what's in, what's out.
- Dependencies and integration points.
- How this will be tested end-to-end from the CLI (frontend and backend).
- ALL required UI components.
  * Check for **implicitly defined components** and define them **explicitly**.
- User flows defined and testable (frontend and backend).
- Answers to ALL open questions.

IMPORTANT:
- Ask clarifying questions.
- Push back on ambiguity.
- Surface trade-offs.
- Provide pros/cons when presenting options.
- When asking clarifying questions or presenting information, NUMBER THE ITEMS so the user can reference them.

## Phase II

WHEN THE USER CONFIRMS THAT YOU HAVE ENOUGH TO PROCEED:
- Follow the (1) Spec Create Workflow and/or (2) Spec Update Workflow as appropriate to create and/or update specs (**every spec you touch should be set to `draft` status**):

IMPORTANT:
- The spec must be designed so results can be end-to-end tested from the CLI.
- Add cross-references to related specs: `fm ref add <stem> <target-stem>`.
- You MUST use `fm` to READ **AND** UPDATE any specs. Do NOT read/update the markdown. Don't touch the spec markdown.
- WHEN FINISHED:
  * Commit your changes.
