(You are in the WRITE phase of the precatur cursus.)

The discuss-and-interview-summary from the previous phase has been injected into your context. Use it as the basis for your spec.

Follow the (1) Spec Create Workflow and/or (2) Spec Update Workflow as appropriate to create and/or update specs (**every spec you create or update should be set to `draft` status**):
- **Resolve open questions yourself.** If you encounter uncertainties during drafting:
  * Read existing code and specs (`fm show <stem> --json`).
  * Reason through the design.
  * Make a decision and document it in the spec.
  * Do NOT defer decisions to the user. You are bringing polished work for review.
- **Self-critique against the quality checklist** (iterate until all pass):
  * **Structural completeness**: All sections have substantive content.
  * **Internal consistency**: No contradictions. Terminology is consistent throughout.
  * **Testability**: Can be end-to-end tested from the CLI. Testing approach is concrete.
  * **Cross-spec coherence**: No conflicts with existing specs. Cross-references (`fm ref add`) are correct and complete.
  * **Edge cases and error handling**: Failure modes identified. Error behavior specified.
  * **Dependency clarity**: External dependencies named. Integration points defined. API contracts specified.
  * **Scope boundaries**: Clear in/out of scope. No ambiguous "maybe" features.
  * **Implementability**: A build agent could implement this with no additional context.

Once the specs have been written, **prepare the presentation artifact.** Write the following to `$SGF_RUN_CONTEXT/draft-presentation.md`:
- **What we're building**: 2-3 sentence summary.
- **Key decisions**: Non-obvious choices and their rationale.
- **How it fits in**: Which existing specs and crates are affected.
- **How it gets tested**: End-to-end verification story. CLI commands that validate it.
- **Out of scope**: What this does NOT cover.


IMPORTANT:
- The spec must be designed so results can be end-to-end tested from the CLI.
- The quality checklist is internal discipline — do not include a checklist report in the presentation.
- Add cross-references to related specs: `fm ref add <stem> <target-stem>`.
- You MUST use `fm` to READ **AND** UPDATE any specs. Do NOT read/update the markdown. Don't touch the spec markdown.
- WHEN FINISHED:
  * Commit your changes.
  * Touch `.ralph-complete`.
