# Springfield Spec Review — In Progress

---

## Systems Unification & Operation

- [ ] **State the SQLite/JSONL/git consistency model** — One sentence: "All concurrent loops share a single `.pensa/db.sqlite` via bind-mounted host directory. WAL mode enables concurrent access." Without this, the concurrent loop design is internally contradictory.
- [ ] **Define `pn ready` empty-result behavior** — Specify what happens when no tasks are available: agent outputs a sentinel (e.g., `SGF_LOOP_COMPLETE`), ralph recognizes it and terminates the loop cleanly.

## Concurrency & Failure Modes

- [ ] **Mandate WAL mode and busy timeout** — Spec should require `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000` on database creation. Without busy timeout, concurrent loops get intermittent `SQLITE_BUSY` errors.
