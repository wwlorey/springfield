# Springfield Spec Review — Done

---

## Systems Unification & Operation

- [x] **Specify sgf-to-ralph CLI contract** — Define ralph's arguments, exit codes, NDJSON stream schema, and prompt templating mechanism (variable syntax, required placeholders). This is the primary integration seam and currently gets one sentence.
- [x] **State the SQLite/JSONL/git consistency model** — Added "Runtime sharing model" paragraph to Pensa Storage Model section. All concurrent loops share a single `.pensa/db.sqlite` via bind-mounted host directory. DELETE journal mode (not WAL) with `busy_timeout=5000`.
- [x] **Define `pn ready` empty-result behavior** — `pn ready --json` returns `[]` on empty. Agent creates `.ralph-complete` file and exits. Ralph detects the sentinel, does final push, exits code 0. Added empty-result instructions to build, test, and issues-plan loop steps. Added inline comment to `pn ready` CLI signature.

## Concurrency & Failure Modes

- [x] **Mandate WAL mode and busy timeout** — Resolved differently: use DELETE mode (not WAL). WAL requires shared memory via mmap, which breaks across Docker Desktop's VirtioFS boundary. DELETE mode with `busy_timeout=5000` is sufficient for Springfield's low write frequency. Added `foreign_keys=ON` to connection pragmas (enforces referential integrity for deps and comments). Pragmas now explicit in Storage Model section.
- [x] **Specify SQLite persistence model across Docker sandboxes** — Answered by the consistency model: bind-mount from host. All containers share one db.sqlite. Atomic claims work across concurrent loops.
