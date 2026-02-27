# Springfield Spec Review — In Progress

---

- [ ] **Define prompt template variable syntax** — Document which variables exist in which prompts, and have `sgf` validate that required variables are present before launching a loop.
- [ ] **Require task ID in commit messages** — Convention: `[pn-abc123] Implement login validation`. Enables `git log --grep`, iteration tracking, and future rollback tooling.
- [ ] **Add task sizing guidance to spec phase** — "Each task should be completable in a single iteration — roughly one file or one logical change. If >3-4 files, split it." Add a task-splitting protocol for build agents.

