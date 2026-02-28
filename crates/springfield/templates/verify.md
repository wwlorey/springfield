Read `memento.md`.
Read `specs/README.md`.

If `verification-report.md` exists, read it.

If **ALL** specs listed in `specs/README.md` have been verified in the report (whether they match or are missing), `touch .ralph-complete` and stop.

Otherwise, choose ONE unverified spec and investigate (a) whether it is actually implemented in the codebase and (b) how well it matches the spec.

1. If it matches the spec, mark it as ✅ (Matches spec)
2. If it is a partial match, mark it as ⚠️ (Partial match / minor discrepancies)
3. If it is missing or very different, mark it as ❌ (Missing or significantly different)

For any gaps or issues found, log them: `pn create "description" -t bug`

Update `verification-report.md` with your findings and update the **Recommendations** section as appropriate.

When the ONE spec has been verified, **commit the changes.**
