# Springfield Spec Review — Done

---

## Systems Unification & Operation

- [x] **Specify sgf-to-ralph CLI contract** — Define ralph's arguments, exit codes, NDJSON stream schema, and prompt templating mechanism (variable syntax, required placeholders). This is the primary integration seam and currently gets one sentence.
- [x] **State the SQLite/JSONL/git consistency model** — Added "Runtime sharing model" paragraph to Pensa Storage Model section. All concurrent loops share a single `.pensa/db.sqlite` via bind-mounted host directory. DELETE journal mode (not WAL) with `busy_timeout=5000`.
- [x] **Define `pn ready` empty-result behavior** — `pn ready --json` returns `[]` on empty. Agent creates `.ralph-complete` file and exits. Ralph detects the sentinel, does final push, exits code 0. Added empty-result instructions to build, test, and issues-plan loop steps. Added inline comment to `pn ready` CLI signature.

## Agent Ergonomics & Developer Experience

- [x] **Specify backpressure error recovery protocol** — Closed without spec change. The agent is smart enough to handle backpressure failures without a detailed protocol. Over-specifying recovery steps would constrain agent reasoning unnecessarily.
- [x] **Document `--json` output schema** — Following beads' pattern: no envelope, direct data to stdout, errors to stderr with optional `code` field, exit code 0/1. Added JSON Output subsection to Pensa with per-command output shape table, error codes (`not_found`, `already_claimed`, `cycle_detected`, `invalid_status_transition`), and issue object field list. Skipped `--suggest-next` (doesn't fit one-task-per-iteration model).
- [x] **Mandate failure comments for inter-iteration learning** — Closed without spec change. The Comments table already states agents use comments to "record observations between fresh-context iterations." Agents are smart enough to use comments without a mandated protocol. Prompt-level tuning can reinforce this if needed.

## Extraneous Content (Pruning)

- [x] **Factor out loop boilerplate** — Defined "Standard Loop Iteration" pattern (7 steps) shared by Build, Test, and Issues Plan. Per-stage deltas in a table. Dropped explicit `pn export` from agent steps — pre-commit hook handles it.
- [x] **Delete "Resolved Decisions" section** — Kept "Build order" (only unique item) in the section, removed 18 duplicate bullets. Updated downstream todo items that referenced Resolved Decisions.
- [x] **Trim Design Principles section** — Cut 9 of 11 principles that restated content from earlier sections. Kept "Search before assuming" (unique insight, most common failure mode) and "One task, fresh context" (merged two principles into one-liner with rationale). Added reference line pointing to earlier sections for the rest.
- [x] **Deduplicate `.sgf/` protection** — Already resolved by the Design Principles trimming. The canonical explanation lives only in File Purposes (line 373); Design Principles just references "protected scaffolding" in the trailing one-liner.
- [x] **Deduplicate "tasks are the plan"** — Schema keeps the authoritative definition. Spec stage replaced with one-line reference + query example. Design Principles already just a reference (from Trim task).
- [x] **Cut prek YAML block** — Replaced 17-line YAML block with two sentences describing what each hook does and when it fires. All information preserved: pre-commit runs `pn export`, post-merge/checkout/rewrite runs `pn import`.
- [x] **Condense memento lifecycle** — Replaced 4-bullet block with one sentence. All factual content preserved: what `sgf init` writes (stack type, references to backpressure, spec index, pensa), and the key insight that the memento is stable after init while the referenced files evolve.

## Concurrency & Failure Modes

- [x] **Mandate WAL mode and busy timeout** — Resolved differently: use DELETE mode (not WAL). WAL requires shared memory via mmap, which breaks across Docker Desktop's VirtioFS boundary. DELETE mode with `busy_timeout=5000` is sufficient for Springfield's low write frequency. Added `foreign_keys=ON` to connection pragmas (enforces referential integrity for deps and comments). Pragmas now explicit in Storage Model section.
- [x] **Specify SQLite persistence model across Docker sandboxes** — Answered by the consistency model: bind-mount from host. All containers share one db.sqlite. Atomic claims work across concurrent loops.
- [x] **Add atomic `pn claim-next` command** — Resolved differently: no new command. Following beads' pattern, `pn update --claim` is the atomic operation (`UPDATE ... WHERE status = 'open'`, fails with `already_claimed` if another agent got there first). Agent keeps choice — queries via pensa, picks a task, attempts claim, re-queries on conflict. Standard Loop Iteration stays 7 steps with explicit Query → Choose & Claim separation.
- [x] **Document recovery procedure** — Recovery is sgf's responsibility, not ralph's. sgf writes PID files to `.sgf/run/<loop-id>.pid` on launch. Before launching ralph, sgf checks all PIDs: if any alive, skip cleanup (concurrent loop is running); if all stale, recover (`git checkout -- .`, `git clean -fd`, `pn doctor --fix`). Added `.sgf/run/` to project structure.
- [x] **Specify Claude Code crash behavior** — Ralph does no cleanup between iterations. On CC crash (non-zero exit), ralph logs the failure and continues to the next iteration without resetting dirty state or releasing claimed tasks. Forward correction: the next agent inherits whatever state exists. Stale claims and dirty trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery.
- [x] **Handle dirty working tree at iteration start** — Resolved by crash behavior decision: ralph does not clean up between iterations. Forward correction within a run; sgf pre-launch recovery between runs.
- [x] **Run `pn doctor --fix` at iteration start** — Resolved by crash behavior decision: ralph does not run doctor between iterations. Stale claims are cleared by sgf pre-launch recovery (`pn doctor --fix`) before the next ralph run.
- [x] **Document iteration atomicity principle** — Added "Atomic iterations" to Design Principles: an iteration either commits fully or is discarded entirely. Kept terse (one sentence + reference to sgf pre-launch recovery) to match the trimmed style of the section.

## Systems Unification & Operation

- [x] **Clarify pre-commit hook staging** — `pn export` auto-stages: writes SQLite → JSONL then runs `git add .pensa/*.jsonl`. This makes the pre-commit hook self-contained — no shell glue needed in the hook config. Beads sidesteps this problem entirely by using Dolt (which has built-in version control), but Springfield's SQLite + JSONL design requires explicit staging. Updated Storage Model sync description and `pn export` command comment.
- [x] **Add `sgf stop` command** — Won't fix. Killing the process is sufficient; no dedicated stop command needed.
