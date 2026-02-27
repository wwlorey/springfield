# Springfield Spec Review — In Progress

---

- [ ] **Specify sandbox environment** — Document what is bind-mounted into Docker containers, what binaries are available inside (pn, git, build tools), and what operations happen inside vs. outside.

- [ ] **Clarify pre-commit hook staging** — Does the hook also `git add .pensa/*.jsonl`? If not, exports won't be committed. Remove explicit `pn export` from agent prompts (hook handles it) or document why both are needed.

