# Springfield Spec Review — TODO

Findings from four-agent spec review. Tackle outside this conversation.

---

## Extraneous Content (Pruning)

- [ ] **Condense Future Work items 2-3** — Claude Code hooks and TUI entries are over-detailed for aspirational items. One line each.

---

## Systems Unification & Operation

- [ ] **Specify `sgf logs` behavior** — What it shows (raw output, NDJSON, pensa mutations), where AFK logs are stored, how long retained.
- [ ] **Add `specs/README.md` conflict prevention** — Have `sgf spec` check for running loops and warn/refuse if any are active, since both spec and build stages can update this file.
---

## Concurrency & Failure Modes

- [ ] **Enforce spec-revision pause mechanically** — Add a `spec_revision_active` flag to pensa. When set, `pn ready` returns empty with a message. `sgf spec` sets on entry, clears on exit. `pn doctor --fix` clears if stale.

---

## Agent Ergonomics & Developer Experience

- [ ] **Consider `pn q` removal or redefinition** — Currently redundant with `pn create --json`. Either remove or give it a genuinely different semantic.
- [ ] **Add `sgf pause` / `sgf resume`** — Write a sentinel file between iterations; ralph checks and stops gracefully. Developer can pause without killing mid-commit.
- [ ] **Consider `sgf context` or `pn ready --verbose`** — Single command that emits next ready task + relevant spec filename + backpressure commands. Collapses the 3-4 orientation reads into one call, saving context window.
- [ ] **Add `sgf status --watch`** — Refreshing dashboard in a single terminal. Shows running loops, pensa summary, recent task state changes.
- [ ] **Consider `pn skip <id>`** — Alias for closing with reason "skipped" and distinct `close_reason`, so repeatedly-failing tasks don't block loops.
- [ ] **Consider `pn pin <id>`** — Makes `pn ready` return that task first regardless of normal ordering, for developer-directed prioritization.
