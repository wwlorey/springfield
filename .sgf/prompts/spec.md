Let's have a discussion and you can interview me about what I want to build (as needed).

Read the spec(s) that are involved in these changes as I mention them (if applicable).

---

After the discussion, produce the following deliverables:

1. Write/update spec files (`specs/*.md`).
  a. NOTE: Favor updating existing spec files over creating new ones unless doing so makes sense (e.g. we're making a brand new package).
2. Update `specs/README.md` with any new entries in this format: `| Spec | Code | Purpose |`.
   (Study `@.sgf/loom-specs-README.md` for the reference format.)
3. Use `pn` to create implementation items which cite (1) the specification with lookups for the source code and (2) documentation that needs to be viewed/changed/added.
  a. NOTE: Implementation items should be scoped to atomic changes—the smallest self-contained modifications to the codebase that can be implemented and tested independently.

The implementation plan should END with:
1. Outstanding documentation tasks (README.md, etc. as appropriate).
2. Integration test tasks that verify the feature works end-to-end.

IMPORTANT:
- **The spec should be designed so that the result can be end-to-end tested from the command line.** If more tools are required to achieve this, make that known.
- **Commit your changes when finished.**
