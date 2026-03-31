The user will tell you something they would like to add, change, or fix.

1. Have a discussion with them and interview them if needed so you understand what they want to build.
  a. As the user mentions functionality within the project, study the related specs using `fm` for context.
2. Once the user approves your plan, implement the change.
3. Add unit and/or property-based and/or integration tests (whichever is best).
4. When the change is complete:
  a. Run **full BACKPRESSURE**.
  b. Track the work:
    i. Log a BUG using the Bug Log Workflow and an ISSUE (which fixes the bug) using the Issue Create Workflow.
    ii. Capture (i) the impetus for the change and (ii) what work was done to change it, including any relevant design decisions made.
    iii. **Close both as fixed.**
  c. Commit your changes.
