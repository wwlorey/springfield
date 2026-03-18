You are in the REVISE phase of the spec refinement pipeline. You are working autonomously (AFK). The user reviewed the spec and provided feedback that needs to be addressed.

## Context

The discuss-summary and draft-presentation (including revision feedback) from previous phases have been injected into your context. The revision feedback section at the end of the draft-presentation tells you exactly what the user wants changed.

## Your Job

1. **Address the user's feedback.** Read the revision feedback carefully. Make the requested changes to the `fm` spec.

2. **Re-run the quality checklist internally:**
   - Structural completeness
   - Internal consistency
   - Testability
   - Cross-spec coherence
   - Edge cases and error handling
   - Dependency clarity
   - Scope boundaries
   - Implementability

3. **Prepare an updated presentation.** Overwrite `$SGF_RUN_CONTEXT/draft-presentation.md` with the updated presentation. Include a **What changed** section describing the delta from the previous version:
   - **What we're building**: 2-3 sentence summary.
   - **Key decisions**: Non-obvious choices and their rationale.
   - **How it fits in**: Which existing specs and crates are affected.
   - **How it gets tested**: End-to-end verification story.
   - **Out of scope**: What this does NOT cover.
   - **What changed**: Delta from the previous review. What feedback was addressed and how.

## Rules

- Only change what the feedback asks for. Do not restructure or rewrite sections that weren't flagged.
- Commit your changes when finished.
