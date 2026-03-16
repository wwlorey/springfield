Study the following to learn about the `pn doctor` and `fm doctor` commands:

>  `pn doctor [--fix] --json`
>
>  | Check | Detects | `--fix` behavior |
>  |-------|---------|------------------|
>  | `stale_claim` | Issues stuck in `in_progress` | Releases claim, sets status to `open` |
>  | `orphaned_dep` | Deps referencing nonexistent issues | Deletes orphaned dep records |
>  | `jsonl_drift` | JSONL/SQLite count mismatch | No auto-fix — run `pn export` or `pn import` depending on direction |
>
>  - JSONL drift remediation: if DB has more → `pn export`; if JSONL has more → `pn import`.

>  `fm doctor [--fix] --json`
>
>  | Check | Detects | `--fix` behavior |
>  |-------|---------|------------------|
>  | `sync_drift` | JSONL/SQLite count mismatch (specs, sections, refs) | No auto-fix — run `fm export` or `fm import` depending on direction |
>  | `orphaned_ref` | Refs pointing to non-existent specs | Deletes orphaned refs |
>  | `orphaned_section` | Sections referencing non-existent specs | Deletes orphaned sections |
>
>  - Sync drift remediation: if DB has more → `fm export`; if JSONL has more → `fm import`.

Run `pn doctor --json` and `fm doctor --json`.

IF issues are returned from `pn doctor`:
1. Check to see which of those issues have been completed.
2. Check to see which of those issues are still valid.
3. Comment pertinent info found during your research to those issues.
4. Mark as complete any completed and/or invalid issues.

IF findings are returned from `fm doctor`:
1. Review each finding and determine the appropriate remediation.
2. Apply fixes as appropriate (`fm doctor --fix --json` for orphaned refs/sections, or `fm export`/`fm import` for sync drift).

Commit your changes.
