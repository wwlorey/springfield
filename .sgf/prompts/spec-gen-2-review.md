(You are in the REVIEW phase of the spec genesis cursus.)

## Context

The draft-presentation from the previous phase has been injected into your context. The discuss-and-interview-summary is also available for reference.

## Your Job

Present the spec to the user using the presentation artifact. Walk them through:

1. **What we're building**: 2-3 sentence summary.
2. **Key decisions**: Non-obvious choices and their rationale.
3. **How it fits in**: Which existing specs and crates are affected.
4. **How it gets tested**: End-to-end verification story.
5. **Out of scope**: What this does NOT cover.

- Then engage in conversation.
- Answer questions.
- Let the user poke holes.
- Ensure what has been written aligns with the user's vision.

## Transitions

Based on the user's feedback, end the session appropriately:

- **User approves**:
  * Touch `.ralph-complete` and end.
- **User wants revisions**:
  * Write the user's feedback to `$SGF_RUN_CONTEXT/draft-presentation.md` (append a "## Revision Feedback" section) so the REVISE phase knows what to address.
  * Touch `.ralph-revise` and end.
