# Springfield Spec Review — TODO

Findings from four-agent spec review. Tackle outside this conversation.

---

## Extraneous Content (Pruning)

- [ ] **Delete "Resolved Decisions" section** — 18 of 19 bullets duplicate earlier content. Move "Build order" (only unique item) to a note in the Architecture section.
- [ ] **Deduplicate "tasks are the plan"** — stated 3 times (Schema, Spec stage, Design Principles). State once in Schema, reference from Spec stage, remove from Design Principles.
- [ ] **Deduplicate `.sgf/` protection** — stated 3 times (File Purposes, Design Principles, Resolved Decisions). Keep in File Purposes only.
- [ ] **Deduplicate bug-to-task lifecycle** — stated in Schema, Issues Plan stage, and Resolved Decisions. Keep Schema + workflow, cut Resolved Decisions entry.
- [ ] **Cut prek YAML block** — 17 lines of implementation detail. Replace with a two-sentence description of what the hooks do.
- [ ] **Condense "Why not Dolt?"** — 5 lines justifying a made decision. Condense to one parenthetical.
- [ ] **Trim Design Principles section** — 80% restates earlier content. Keep "Search before assuming" and 1-2 others that add new insight. Reference earlier sections for the rest.
- [ ] **Condense memento lifecycle** — 4 bullets to say "doesn't change after init." Replace with one sentence.
- [ ] **Factor out loop boilerplate** — Build, Test, and Issues Plan stages repeat the same 8-step pattern. Define the "standard loop iteration" once, then each stage references it with its specifics.
- [ ] **Condense Future Work items 2-3** — Claude Code hooks and TUI entries are over-detailed for aspirational items. One line each.

---

## Systems Unification & Operation

- [ ] **Specify sgf-to-ralph CLI contract** — Define ralph's arguments, exit codes, NDJSON stream schema, and prompt templating mechanism (variable syntax, required placeholders). This is the primary integration seam and currently gets one sentence.
- [ ] **Define `pn ready` empty-result behavior** — Specify what happens when no tasks are available: agent outputs a sentinel (e.g., `SGF_LOOP_COMPLETE`), ralph recognizes it and terminates the loop cleanly.
- [ ] **Add `sgf stop` command** — No way to cleanly stop a running loop. Should send SIGTERM to ralph, which completes/aborts current iteration, releases claimed task, and exits.
- [ ] **Specify sandbox environment** — Document what is bind-mounted into Docker containers, what binaries are available inside (pn, git, build tools), and what operations happen inside vs. outside.
- [ ] **Clarify pre-commit hook staging** — Does the hook also `git add .pensa/*.jsonl`? If not, exports won't be committed. Remove explicit `pn export` from agent prompts (hook handles it) or document why both are needed.
- [ ] **State the SQLite/JSONL/git consistency model** — One sentence: "All concurrent loops share a single `.pensa/db.sqlite` via bind-mounted host directory. WAL mode enables concurrent access." Without this, the concurrent loop design is internally contradictory.
- [ ] **Define prompt template variable syntax** — Document which variables exist in which prompts, and have `sgf` validate that required variables are present before launching a loop.
- [ ] **Specify `sgf status` output** — Show running loops (iteration count, last task), pensa summary (open/in_progress/closed by type), recent activity.
- [ ] **Specify `sgf logs` behavior** — What it shows (raw output, NDJSON, pensa mutations), where AFK logs are stored, how long retained.
- [ ] **Evaluate ralph as library crate** — Currently a subprocess; sgf is its sole consumer. Library crate would give typed errors, shared config, no serialization boundary. Could keep a thin binary for standalone use.
- [ ] **Add `specs/README.md` conflict prevention** — Have `sgf spec` check for running loops and warn/refuse if any are active, since both spec and build stages can update this file.
- [ ] **Document recovery procedure** — After a failed iteration: `git checkout .` to discard uncommitted changes, `pn doctor --fix` to release stale claims, restart loop. Consider having ralph do this automatically at iteration start.
- [ ] **Consider whether build loop should handle trivial bugs inline** — Currently all bugs go through the issues-plan pipeline (3+ iteration boundaries). Trivial self-discovered bugs could be fixed in the same iteration.

---

## Concurrency & Failure Modes

- [ ] **Mandate WAL mode and busy timeout** — Spec should require `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000` on database creation. Without busy timeout, concurrent loops get intermittent `SQLITE_BUSY` errors.
- [ ] **Run `pn doctor --fix` at iteration start** — Ralph should automatically release stale claims before `pn ready`. Costs one query, prevents stuck tasks from crashed iterations.
- [ ] **Replace 30-min stale threshold with heartbeat** — Fixed timeout is too aggressive (Rust builds can exceed 30min). Ralph should periodically touch `updated_at` on claimed tasks; doctor checks `updated_at` staleness instead of claim time.
- [ ] **Add atomic `pn claim-next` command** — Combines `pn ready` + `pn update --claim` in a single transaction, eliminating the TOCTOU race window. Returns either a claimed task or "nothing available."
- [ ] **Handle rebase conflicts** — Specify retry count (e.g., 3). On conflict: `git rebase --abort`, re-export JSONL from SQLite, create fresh commit, retry push. If conflict is in source code, iteration is failed — release task and move on.
- [ ] **Make `pn export` rebase-aware** — During rebase (detect via `.git/rebase-merge/`), skip export. Post-rebase: run `pn export` once for the final state.
- [ ] **Address JSONL merge conflicts** — Guaranteed to happen with concurrent loops. Options: (a) custom git merge driver for `*.jsonl`, (b) post-rebase unconditional `pn export` rebuild treating SQLite as sole source of truth, (c) both. Option (b) is simplest.
- [ ] **Specify SQLite persistence model across Docker sandboxes** — Bind-mount from host? Rebuilt each iteration via `pn import`? This determines whether atomic claims work across concurrent loops. Document the chosen model.
- [ ] **Handle dirty working tree at iteration start** — Ralph should discard uncommitted changes (`git checkout -- .`) from crashed previous iterations before starting work. Document: "Work is only preserved when committed."
- [ ] **Specify Claude Code crash behavior** — Non-zero exit from Claude Code = iteration failed. Ralph logs failure, discards uncommitted changes, proceeds to next iteration. Document whether ralph releases the claim or lets doctor handle it.
- [ ] **Document single-branch concurrency model** — Explicitly state that branch-based workflows (feature branches both modifying pensa) require manual JSONL conflict resolution or a custom merge driver. All concurrent work happens on one branch.
- [ ] **Enforce spec-revision pause mechanically** — Add a `spec_revision_active` flag to pensa. When set, `pn ready` returns empty with a message. `sgf spec` sets on entry, clears on exit. `pn doctor --fix` clears if stale.
- [ ] **Document iteration atomicity principle** — Add to Design Principles: "Iterations are atomic — either fully committed or fully discarded. Partial work from crashed iterations is discarded on next startup."

---

## Agent Ergonomics & Developer Experience

- [ ] **Specify backpressure error recovery protocol** — Define retry policy (e.g., "fix once per validation step; if second run fails, log bug, unclaim task"). Add circuit breaker for cascading failures. This is the most complex part of the loop and currently gets one sentence.
- [ ] **Mandate failure comments for inter-iteration learning** — Build prompt must require: before dying on failure, `pn comment add <id> "Attempted: <what>. Failed: <why>."`. Next iteration checks `pn comment list <id>` after claiming.
- [ ] **Document `--json` output schema** — Specify envelope format (`{"ok": bool, "data": ..., "error": ...}`), that errors with `--json` go to stdout as JSON, and field names for each command's response.
- [ ] **Add task sizing guidance to spec phase** — "Each task should be completable in a single iteration — roughly one file or one logical change. If >3-4 files, split it." Add a task-splitting protocol for build agents.
- [ ] **Consider `pn q` removal or redefinition** — Currently redundant with `pn create --json`. Either remove or give it a genuinely different semantic.
- [ ] **Require task ID in commit messages** — Convention: `[pn-abc123] Implement login validation`. Enables `git log --grep`, iteration tracking, and future rollback tooling.
- [ ] **Add `sgf pause` / `sgf resume`** — Write a sentinel file between iterations; ralph checks and stops gracefully. Developer can pause without killing mid-commit.
- [ ] **Consider `sgf context` or `pn ready --verbose`** — Single command that emits next ready task + relevant spec filename + backpressure commands. Collapses the 3-4 orientation reads into one call, saving context window.
- [ ] **Add `sgf status --watch`** — Refreshing dashboard in a single terminal. Shows running loops, pensa summary, recent task state changes.
- [ ] **Consider `pn skip <id>`** — Alias for closing with reason "skipped" and distinct `close_reason`, so repeatedly-failing tasks don't block loops.
- [ ] **Consider `pn pin <id>`** — Makes `pn ready` return that task first regardless of normal ordering, for developer-directed prioritization.
