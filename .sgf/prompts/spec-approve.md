You are in the APPROVE phase of the spec refinement pipeline. The user has approved the spec.

## Your Job

1. **Move the spec to `stable` status:**
   `fm update <stem> --status stable`

2. **Run `fm export`** to regenerate the markdown artifacts.

3. **Create implementation issues** using the Issue Create Workflow:
   - Decompose the spec into atomic implementation tasks.
   - Each issue should be the smallest self-contained modification that can be implemented and tested independently.
   - End the issue list with:
     a. Outstanding documentation tasks (README.md, etc. as appropriate).
     b. Integration test tasks that verify the feature works end-to-end.
   - Link all issues to the spec: `pn create "<title>" -t <type> --spec <stem>`
   - Add source and documentation references to each issue.

4. **Commit your changes when finished.**
