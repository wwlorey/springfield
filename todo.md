# Springfield Spec Review — TODO

Findings from four-agent spec review. Tackle outside this conversation.

---

## Extraneous Content (Pruning)

- [ ] **Deduplicate `.sgf/` protection** — stated 2 times (File Purposes, Design Principles). Keep in File Purposes only.
- [ ] **Deduplicate bug-to-task lifecycle** — stated in Schema and Issues Plan stage. Keep Schema + workflow, cut the other.
- [ ] **Condense "Why not Dolt?"** — 5 lines justifying a made decision. Condense to one parenthetical.
- [ ] **Condense memento lifecycle** — 4 bullets to say "doesn't change after init." Replace with one sentence.
- [ ] **Condense Future Work items 2-3** — Claude Code hooks and TUI entries are over-detailed for aspirational items. One line each.

---

## Systems Unification & Operation

- [ ] **Add `sgf stop` command** — No way to cleanly stop a running loop. Should send SIGTERM to ralph, which completes/aborts current iteration, releases claimed task, and exits.
- [ ] **Define prompt template variable syntax** — Document which variables exist in which prompts, and have `sgf` validate that required variables are present before launching a loop.
- [ ] **Specify `sgf status` output** — Show running loops (iteration count, last task), pensa summary (open/in_progress/closed by type), recent activity.
- [ ] **Specify `sgf logs` behavior** — What it shows (raw output, NDJSON, pensa mutations), where AFK logs are stored, how long retained.
- [ ] **Evaluate ralph as library crate** — Currently a subprocess; sgf is its sole consumer. Library crate would give typed errors, shared config, no serialization boundary. Could keep a thin binary for standalone use.
- [ ] **Add `specs/README.md` conflict prevention** — Have `sgf spec` check for running loops and warn/refuse if any are active, since both spec and build stages can update this file.
- [ ] **Consider whether build loop should handle trivial bugs inline** — Currently all bugs go through the issues-plan pipeline (3+ iteration boundaries). Trivial self-discovered bugs could be fixed in the same iteration.

---

## Concurrency & Failure Modes

- [ ] **Run `pn doctor --fix` at iteration start** — Ralph should automatically release stale claims before `pn ready`. Costs one query, prevents stuck tasks from crashed iterations.
- [ ] **Replace 30-min stale threshold with heartbeat** — Fixed timeout is too aggressive (Rust builds can exceed 30min). Ralph should periodically touch `updated_at` on claimed tasks; doctor checks `updated_at` staleness instead of claim time.
- [ ] **Handle rebase conflicts** — Specify retry count (e.g., 3). On conflict: `git rebase --abort`, re-export JSONL from SQLite, create fresh commit, retry push. If conflict is in source code, iteration is failed — release task and move on.
- [ ] **Make `pn export` rebase-aware** — During rebase (detect via `.git/rebase-merge/`), skip export. Post-rebase: run `pn export` once for the final state.
- [ ] **Address JSONL merge conflicts** — Guaranteed to happen with concurrent loops. Options: (a) custom git merge driver for `*.jsonl`, (b) post-rebase unconditional `pn export` rebuild treating SQLite as sole source of truth, (c) both. Option (b) is simplest.
- [ ] **Handle dirty working tree at iteration start** — Ralph should discard uncommitted changes (`git checkout -- .`) from crashed previous iterations before starting work. Document: "Work is only preserved when committed."
- [ ] **Document single-branch concurrency model** — Explicitly state that branch-based workflows (feature branches both modifying pensa) require manual JSONL conflict resolution or a custom merge driver. All concurrent work happens on one branch.
- [ ] **Enforce spec-revision pause mechanically** — Add a `spec_revision_active` flag to pensa. When set, `pn ready` returns empty with a message. `sgf spec` sets on entry, clears on exit. `pn doctor --fix` clears if stale.
- [ ] **Document iteration atomicity principle** — Add to Design Principles: "Iterations are atomic — either fully committed or fully discarded. Partial work from crashed iterations is discarded on next startup."

---

## Agent Ergonomics & Developer Experience

- [ ] **Add task sizing guidance to spec phase** — "Each task should be completable in a single iteration — roughly one file or one logical change. If >3-4 files, split it." Add a task-splitting protocol for build agents.
- [ ] **Consider `pn q` removal or redefinition** — Currently redundant with `pn create --json`. Either remove or give it a genuinely different semantic.
- [ ] **Require task ID in commit messages** — Convention: `[pn-abc123] Implement login validation`. Enables `git log --grep`, iteration tracking, and future rollback tooling.
- [ ] **Add `sgf pause` / `sgf resume`** — Write a sentinel file between iterations; ralph checks and stops gracefully. Developer can pause without killing mid-commit.
- [ ] **Consider `sgf context` or `pn ready --verbose`** — Single command that emits next ready task + relevant spec filename + backpressure commands. Collapses the 3-4 orientation reads into one call, saving context window.
- [ ] **Add `sgf status --watch`** — Refreshing dashboard in a single terminal. Shows running loops, pensa summary, recent task state changes.
- [ ] **Consider `pn skip <id>`** — Alias for closing with reason "skipped" and distinct `close_reason`, so repeatedly-failing tasks don't block loops.
- [ ] **Consider `pn pin <id>`** — Makes `pn ready` return that task first regardless of normal ordering, for developer-directed prioritization.
