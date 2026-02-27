# Springfield Spec Review — In Progress

---

- [ ] **Address JSONL merge conflicts** — Guaranteed to happen with concurrent loops. Options: (a) custom git merge driver for `*.jsonl`, (b) post-rebase unconditional `pn export` rebuild treating SQLite as sole source of truth, (c) both. Option (b) is simplest.
- [ ] **Define prompt template variable syntax** — Document which variables exist in which prompts, and have `sgf` validate that required variables are present before launching a loop.
- [ ] **Deduplicate bug-to-task lifecycle** — stated in Schema and Issues Plan stage. Keep Schema + workflow, cut the other.

