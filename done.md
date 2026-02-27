# Springfield Spec Review — Done

---

## Agent Ergonomics & Developer Experience

- [x] **Require task ID in commit messages** — Convention: `[pn-abc123] Implement login validation`. Added to Standard Loop Iteration step 7: commit messages in build, test, and issues-plan stages are prefixed with `[<task-id>]`. Enforced via prompt instructions, not git hooks. Other stages (spec, verify, test-plan, issues-log) commit without a prefix. Enables `git log --grep` for per-task history.

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
- [x] **Deduplicate bug-to-task lifecycle** — Schema keeps the authoritative definitions: `fixes` field describes auto-close behavior, "Bugs are never ready" states the `pn ready` exclusion rule. Issues Plan stage trimmed to one sentence (release vs close). Two redundant lifecycle re-explanations removed.

## Concurrency & Failure Modes

- [x] **Mandate WAL mode and busy timeout** — Resolved differently: use DELETE mode (not WAL). WAL requires shared memory via mmap, which breaks across Docker Desktop's VirtioFS boundary. DELETE mode with `busy_timeout=5000` is sufficient for Springfield's low write frequency. Added `foreign_keys=ON` to connection pragmas (enforces referential integrity for deps and comments). Pragmas now explicit in Storage Model section.
- [x] **Specify SQLite persistence model across Docker sandboxes** — Answered by the consistency model: bind-mount from host. All containers share one db.sqlite. Atomic claims work across concurrent loops.
- [x] **Add atomic `pn claim-next` command** — Resolved differently: no new command. Following beads' pattern, `pn update --claim` is the atomic operation (`UPDATE ... WHERE status = 'open'`, fails with `already_claimed` if another agent got there first). Agent keeps choice — queries via pensa, picks a task, attempts claim, re-queries on conflict. Standard Loop Iteration stays 7 steps with explicit Query → Choose & Claim separation.
- [x] **Document recovery procedure** — Recovery is sgf's responsibility, not ralph's. sgf writes PID files to `.sgf/run/<loop-id>.pid` on launch. Before launching ralph, sgf checks all PIDs: if any alive, skip cleanup (concurrent loop is running); if all stale, recover (`git checkout -- .`, `git clean -fd`, `pn doctor --fix`). Added `.sgf/run/` to project structure.
- [x] **Specify Claude Code crash behavior** — Ralph does no cleanup between iterations. On CC crash (non-zero exit), ralph logs the failure and continues to the next iteration without resetting dirty state or releasing claimed tasks. Forward correction: the next agent inherits whatever state exists. Stale claims and dirty trees accumulate within a ralph run and are cleared by sgf's pre-launch recovery.
- [x] **Handle dirty working tree at iteration start** — Resolved by crash behavior decision: ralph does not clean up between iterations. Forward correction within a run; sgf pre-launch recovery between runs.
- [x] **Run `pn doctor --fix` at iteration start** — Resolved by crash behavior decision: ralph does not run doctor between iterations. Stale claims are cleared by sgf pre-launch recovery (`pn doctor --fix`) before the next ralph run.
- [x] **Document iteration atomicity principle** — Added "Atomic iterations" to Design Principles: an iteration either commits fully or is discarded entirely. Kept terse (one sentence + reference to sgf pre-launch recovery) to match the trimmed style of the section.
- [x] **Handle rebase conflicts** — Not needed. Mutagen file sync means all sandboxes share the same git history — concurrent loops see each other's commits within seconds, so push conflicts don't arise and `git pull --rebase` is unnecessary. Removed incorrect `git pull --rebase` advice from Concurrency model paragraph. Replaced with explanation of why shared-filesystem sync eliminates the problem.
- [x] **Make `pn export` rebase-aware** — Not needed. Same reasoning: rebase isn't part of the workflow when sandboxes share git state via Mutagen sync.

## Systems Unification & Operation

- [x] **Evaluate ralph as library crate** — Keep as subprocess. Ralph has standalone CLI value beyond sgf — developer may use it directly. The well-defined CLI contract (flags, exit codes, invocation pattern) is the right boundary. Library crate's typed errors and shared config don't outweigh losing ralph as an independent tool.
- [x] **Clarify pre-commit hook staging** — `pn export` auto-stages: writes SQLite → JSONL then runs `git add .pensa/*.jsonl`. This makes the pre-commit hook self-contained — no shell glue needed in the hook config. Beads sidesteps this problem entirely by using Dolt (which has built-in version control), but Springfield's SQLite + JSONL design requires explicit staging. Updated Storage Model sync description and `pn export` command comment.
- [x] **Add `sgf stop` command** — Won't fix. Killing the process is sufficient; no dedicated stop command needed.
- [x] **Specify sandbox environment** — Docker Desktop sandboxes are microVMs with Mutagen-based file sync (not bind mounts). Documented: Docker-over-native-sandbox rationale (network isolation incompatible with AFK), template images (Dockerfile-based, `pn` baked in, rebuild after updates), credentials via Docker proxy, agent user. Discovered SQLite can't be shared across sandboxes (POSIX locks don't cross sync boundary) — introduced pensa client/daemon architecture: daemon on host owns SQLite, CLI is thin HTTP client connecting via `host.docker.internal:7533`. Added `sgf template build` command, `pn daemon` commands. Updated Storage Model, Sandboxing section, and concurrency model.

## Concurrency & Failure Modes

- [x] **Replace 30-min stale threshold with heartbeat** — Resolved simpler: dropped the time threshold entirely. sgf's pre-launch recovery already confirms all loops are dead via PID checks before calling `pn doctor --fix`, so every in_progress claim is stale by definition. No heartbeat needed — PID liveness is the authority. Doctor now releases all in_progress claims unconditionally.
- [x] **Address JSONL merge conflicts** — Not needed. Mutagen file sync means all sandboxes share the same git history on the host — there are no independent push/pull races, so JSONL merge conflicts can't arise. The pensa daemon serializes all database access, and `pn export` produces a consistent snapshot at commit time. The problem this item described was eliminated by the sandbox architecture (Mutagen sync + pensa daemon).
- [x] **Document single-branch concurrency model** — Not needed. Single-branch operation is already implied by the architecture: Mutagen sync shares one host directory across all sandboxes, the pensa daemon serves one SQLite per project, and `sgf` has no branch-targeting mechanism. Multi-branch conflicts can't arise without deliberately going outside Springfield's workflow.

## Agent Ergonomics & Developer Experience

- [x] **Add task sizing guidance to spec phase** — Closed without spec change. Task sizing is a prompt template concern, not a spec concern. The spec already defers to the prompt ("The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line"). Implementation plan structure — tooling setup first, cited bullet points, docs + integration tests last — lives in `.sgf/prompts/spec.md`, seeded by `sgf init` and editable per-project.

## Systems Unification & Operation

- [x] **Consider whether build loop should handle trivial bugs inline** — Closed without spec change. The spec already gives agents discretion — Standard Loop Iteration step 5 says "**if** problems are discovered." Agents fix trivial issues inline as part of the current task; only genuinely non-trivial, out-of-scope bugs are worth logging to pensa. This is prompt-level tuning (build prompt can reinforce "fix trivial issues you encounter; only log bugs outside the scope of your current task"), not a spec-level change.
- [x] **Consider `pn skip <id>`** — Closed without spec change. Not needed — stale claims already prevent within-run thrashing (crashed agent's `in_progress` claim excludes the task from `pn ready` for the rest of that ralph run). Across runs, `pn doctor --fix` clears claims, but persistent failures are visible to the developer via logs/status. `pn close --reason "skipped"` + `pn reopen` covers deliberate deferral. A dedicated skip command/status adds API surface for something the existing mechanics handle.
