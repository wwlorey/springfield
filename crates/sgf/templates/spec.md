Read `memento.md`.

Let's have a discussion and you can interview me about what I want to build.

---

After the discussion, produce the following deliverables:

1. Write spec files to `specs/`
2. Update `specs/README.md` with new entries in this format: `| Spec | Code | Purpose |`
   Reference: https://github.com/ghuntley/loom/blob/trunk/specs/README.md
3. Create implementation plan items via pensa:
   `pn create -t task --spec <stem> "task title" [-p <priority>] [--dep <id>]`
   Set dependencies between tasks where order matters.

The implementation plan should BEGIN with:
1. Any project-specific tooling setup needed

And the implementation plan should END with:
1. Documentation tasks (README.md, etc. as appropriate)
2. Integration test tasks that verify the feature works end-to-end

IMPORTANT:
- **The spec should be designed so that the result can be end-to-end tested from the command line.** If more tools are required to achieve this, make that known.
- **Commit the changes when finished.**
