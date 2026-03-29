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
- **User Interface**: All necessary UI elements for each user flow are present and connected to backend functionality.

Present your findings to the user. Update the specs to fix the obvious problems. Then, surface the more complicated questions/concerns to the user for their input, going one by one through each. Update the specs based on the user's input.

When finished editing the specs:
- **Export fm and commit your changes**.
- IF the user says the specs are fully hardened (ask them):
  * Move the specs marked `draft` to `stable` status.
  * Touch `.iter-complete`.
- End.
