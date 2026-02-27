# Springfield Spec Review — TODO

Findings from four-agent spec review. Tackle outside this conversation.

---

## Extraneous Content (Pruning)

- [ ] **Condense Future Work items 2-3** — Claude Code hooks and TUI entries are over-detailed for aspirational items. One line each.

---

## Systems Unification & Operation

- [ ] **Specify `sgf logs` behavior** — What it shows (raw output, NDJSON, pensa mutations), where AFK logs are stored, how long retained.
---

---

## Agent Ergonomics & Developer Experience

- [ ] **Consider `pn q` removal or redefinition** — Currently redundant with `pn create --json`. Either remove or give it a genuinely different semantic.
- [ ] **Add `sgf pause` / `sgf resume`** — Write a sentinel file between iterations; ralph checks and stops gracefully. Developer can pause without killing mid-commit.
- [ ] **Consider `sgf context` or `pn ready --verbose`** — Single command that emits next ready task + relevant spec filename + backpressure commands. Collapses the 3-4 orientation reads into one call, saving context window.
- [ ] **Add `sgf status --watch`** — Refreshing dashboard in a single terminal. Shows running loops, pensa summary, recent task state changes.
- [ ] **Consider `pn pin <id>`** — Makes `pn ready` return that task first regardless of normal ordering, for developer-directed prioritization.
