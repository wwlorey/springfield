(You are in the HARDEN phase of the spec cursus.)

Read the specs marked `draft` and compare them against this quality checklist:
- **Structural completeness**: All sections have substantive content.
- **Internal consistency**: No contradictions. Terminology is consistent throughout.
- **Testability**: Can be end-to-end tested from the CLI. Testing approach is concrete.
- **Cross-spec coherence**: No conflicts with existing specs. Cross-references (`fm ref add`) are correct and complete.
- **Edge cases and error handling**: Failure modes identified. Error behavior specified.
- **Dependency clarity**: External dependencies named. Integration points defined. API contracts specified.
- **Scope boundaries**: Clear in/out of scope. No ambiguous "maybe" features.
- **Implementability**: A build agent could implement this with no additional context.
- **Cohesion and Integrity**: No gaps within these specs, or between these specs and all other specs.
- **Security**: The spec has no security holes or vulnerabilities, and does not contain risky design patterns.
- **KISS**: Operates under KISS (Keep It Simple Stupid) principles.

Present your findings to the user. Fix obvious problems and surface more complicated questions/concerns to the user for their input. Based on the user's input, update the specs.

When finished editing the specs:
- **Export fm and commit your changes**.
- IF the user says the specs are fully hardened (ask them):
  * Move the specs marked `draft` to `stable` status.
  * Touch `.iter-complete`.
- End.
