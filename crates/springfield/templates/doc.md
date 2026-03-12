Study the following to learn about the `pn doctor` command:

>  `pn doctor [--fix] --json`
>
>  | Check | Detects | `--fix` behavior |
>  |-------|---------|------------------|
>  | `stale_claim` | Issues stuck in `in_progress` | Releases claim, sets status to `open` |
>  | `orphaned_dep` | Deps referencing nonexistent issues | Deletes orphaned dep records |
>  | `jsonl_drift` | JSONL/SQLite count mismatch | No auto-fix — run `pn export` or `pn import` depending on direction |
>
>  - JSONL drift remediation: if DB has more → `pn export`; if JSONL has more → `pn import`.

Run `pn doctor --json`.

IF issues are returned:
1. Check to see which of those issues have been completed.
2. Check to see which of those issues are still valid.
3. Comment pertinent info found during your research to those issues.
4. Mark as complete any completed and/or invalid issues.
