# Springfield Spec Review — In Progress

---

- [ ] **Specify sandbox environment** — Document what is bind-mounted into Docker containers, what binaries are available inside (pn, git, build tools), and what operations happen inside vs. outside.
- [ ] **Address JSONL merge conflicts** — Guaranteed to happen with concurrent loops. Options: (a) custom git merge driver for `*.jsonl`, (b) post-rebase unconditional `pn export` rebuild treating SQLite as sole source of truth, (c) both. Option (b) is simplest.

