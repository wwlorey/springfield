# Springfield Spec Review — In Progress

---

- [ ] **Define prompt template variable syntax** — Document which variables exist in which prompts, and have `sgf` validate that required variables are present before launching a loop.
- [ ] **Enforce spec-revision pause mechanically** — Add a `spec_revision_active` flag to pensa. When set, `pn ready` returns empty with a message. `sgf spec` sets on entry, clears on exit. `pn doctor --fix` clears if stale.
- [ ] **Specify `sgf logs` behavior** — What it shows (raw output, NDJSON, pensa mutations), where AFK logs are stored, how long retained.

