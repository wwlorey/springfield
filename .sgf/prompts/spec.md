Let's have a discussion and you can interview me about what I want to build (as needed).

Read the spec(s) that are involved in these changes as I mention them (if applicable).

---

After the discussion, produce the following deliverables:

1. Create or update specs using `fm` (forma) commands.
  a. NOTE: Favor updating existing specs (`fm update`, `fm section set`) over creating new ones unless doing so makes sense (e.g. we're making a brand new package — use `fm create`).
  b. Use `fm list --json` to see existing specs. Use `fm show <stem> --json` to inspect a spec.
  c. After making changes, run `fm export` to regenerate the markdown artifacts.
2. Use `pn` to create implementation items which cite (1) the specification with lookups for the source code and (2) documentation that needs to be viewed/changed/added.
  a. NOTE: Implementation items should be scoped to atomic changes—the smallest self-contained modifications to the codebase that can be implemented and tested independently.

The implementation plan should END with:
1. Outstanding documentation tasks (README.md, etc. as appropriate).
2. Integration test tasks that verify the feature works end-to-end.

IMPORTANT:
- **The spec should be designed so that the result can be end-to-end tested from the command line.** If more tools are required to achieve this, make that known.
- **Commit your changes when finished.**
