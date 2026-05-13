The user will tell you something they would like to add, change, or fix.

1. Have a discussion with them and interview them if needed so you understand what they want to build.
  a. As the user mentions functionality within the project, study the related specs using `fm` for context.
2. Present a change plan.
3. Check it for robustness and ensure it actually fixes the issue/makes the requested change.
4. Once the user approves your plan, implement the change.
5. Add unit and/or property-based and/or integration tests (whichever is best).
6. Update affected specs:
  a. Identify specs covering the code you changed — run `fm list --json` and check which specs have a `src` path overlapping with the files you modified.
  b. For each affected spec:
    i. Read it via `fm show <stem> --json`.
    ii. Verify claims in sections that relate to your change against the new code.
    iii. Update via `fm` to match the new behavior. If rewriting >40% of a section, rewrite the whole section so it reads coherently.
    iv. Set status to `proven` via `fm update <stem> --status proven`.
  c. Export via `fm export`.
  d. If no specs are affected by the change, skip this step.
7. When the change is complete:
  a. Run **full BACKPRESSURE**.
  b. If applicable, run `just install`.
  c. Track the work:
    i. Log a BUG using the Bug Log Workflow and an ISSUE (which fixes the bug) using the Issue Create Workflow.
    ii. Capture (i) the impetus for the change and (ii) what work was done to change it, including any relevant design decisions made.
    iii. **Close both as fixed.**
  d. Commit your changes.
