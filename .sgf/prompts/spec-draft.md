You are in the DRAFT phase of the spec refinement pipeline. You are working autonomously (AFK). Your goal is to produce a complete, polished spec that passes internal quality gates.

## Context

The discuss-summary from the previous phase has been injected into your context. Use it as the basis for your spec.

## Your Job

1. **Create or update the `fm` spec** in `draft` status.
   - Use the Spec Create Workflow or Spec Update Workflow as appropriate.
   - Write full section content. No placeholders. No skeletons. No "TBD."
   - Every required section (`overview`, `architecture`, `dependencies`, `error-handling`, `testing`) must have substantive content.
   - Add custom sections as needed for the specific feature.

2. **Resolve open questions yourself.** If you encounter uncertainties during drafting:
   - Read existing code and specs (`fm show <stem> --json`).
   - Reason through the design.
   - Make a decision and document it in the spec.
   - Do NOT defer decisions to the user. You are bringing polished work for review.

3. **Self-critique against the quality checklist** (iterate until all pass):
   - **Structural completeness**: All sections have substantive content.
   - **Internal consistency**: No contradictions. Terminology is consistent throughout.
   - **Testability**: Can be end-to-end tested from the CLI. Testing approach is concrete.
   - **Cross-spec coherence**: No conflicts with existing specs. Cross-references (`fm ref add`) are correct and complete.
   - **Edge cases and error handling**: Failure modes identified. Error behavior specified.
   - **Dependency clarity**: External dependencies named. Integration points defined. API contracts specified.
   - **Scope boundaries**: Clear in/out of scope. No ambiguous "maybe" features.
   - **Implementability**: A build agent could implement this with no additional context.

4. **Prepare the presentation artifact.** Write the following to `$SGF_RUN_CONTEXT/draft-presentation.md`:
   - **What we're building**: 2-3 sentence summary.
   - **Key decisions**: Non-obvious choices and their rationale.
   - **How it fits in**: Which existing specs and crates are affected.
   - **How it gets tested**: End-to-end verification story. CLI commands that validate it.
   - **Out of scope**: What this does NOT cover.

## Rules

- The spec must be designed so results can be end-to-end tested from the CLI.
- The quality checklist is internal discipline — do not include a checklist report in the presentation.
- Add cross-references to related specs: `fm ref add <stem> <target-stem>`.
- Commit your changes when finished.
