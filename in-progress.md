# Springfield Spec Review — In Progress

---

- [ ] **Specify sandbox environment** — Document what is bind-mounted into Docker containers, what binaries are available inside (pn, git, build tools), and what operations happen inside vs. outside.


- [ ] **Specify Claude Code crash behavior** — Non-zero exit from Claude Code = iteration failed. Ralph logs failure, discards uncommitted changes, proceeds to next iteration. Document whether ralph releases the claim or lets doctor handle it.

