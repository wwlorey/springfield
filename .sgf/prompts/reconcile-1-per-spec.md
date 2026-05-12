(You are in the RECONCILE phase of the reconcile cursus.)

You are aligning specs to match the reality of the codebase. **The code is the source of truth.** You are NOT changing code — you are updating specs to accurately describe what the code does.

## Setup

1. Run `fm list --json` to get all specs.
2. Pick the next spec whose status is `draft`.
3. If no specs are `draft` (all are `proven`), touch `.iter-complete` and end.

## Per-Spec Reconciliation

Process ONE spec per iteration. The framework handles iteration.

For the chosen spec:

### 1. Read the spec
- `fm show <stem> --json` — read the full spec with all sections.
- If the spec has a `src` path but that path does not exist in the codebase: the code was likely deleted, renamed, or merged. Search the codebase for where the code may have moved. If it no longer exists at all, delete the spec via `fm delete <stem> --force`, commit with `reconcile(<stem>): delete spec for removed code`, and end this iteration.

### 2. Read the code
- If the spec has a `src` path: read the source files under that path. Follow imports and dependencies to understand what the code actually does.
- If the spec has NO `src` path: query `pn` for closed bugs linked to this spec (step 3), then search the codebase for code related to the spec's domain.

### 3. Gather context
- Query `pn list --status closed --spec <stem> --json` for all closed issues (bugs, tasks, chores) linked to this spec. This captures both the bugs (impetus for change) and the issues that fixed them (implementation details).
- Query `pn list -t bug --status open --spec <stem> --json` and `pn list -t bug --status in_progress --spec <stem> --json` for known open bugs. Use these as context to distinguish intentional behavior from known defects — do NOT describe known-buggy behavior as intended in the spec.
- For divergences with no matching bug, run `git log` on the affected files and use commit messages as secondary rationale.
- Use this context to understand WHY code changed, but do NOT include historical rationale, changelogs, or bug references in the spec text. Specs describe current behavior only.

### 4. Verify every claim
- Go through each section of the spec.
- For every concrete behavioral claim (CLI flags, API endpoints, error behaviors, config options, data flows, etc.), confirm it still exists and works this way in the code.
- Identify claims that are no longer true — functionality that was removed, moved, or changed.

### 5. Update the spec
- Update all sections via `fm` to match what the code actually does.
- Remove claims that can't be verified against the code.
- Add new behavior that exists in the code but isn't described in the spec.
- Update cross-references: if the code imports from or depends on other crates, ensure `fm ref add <stem> <target-stem>` reflects that. Remove refs that no longer apply.
- **If you are substantially rewriting a section (more than roughly 40% of its content), rewrite the entire section from scratch so it reads as a coherent document, not a patchwork of edits.**
- Set the spec status to `proven`.
- **If the spec required NO changes** (it already accurately describes the code), just set the status to `proven` — do not rewrite sections unnecessarily.

### 6. Self-verify
- Re-read the full spec via `fm show <stem> --json`.
- Run this quality checklist against it:
  * **Internal consistency**: No contradictions within the spec. Terminology is consistent.
  * **Implementability**: A build agent could implement from this spec without additional context.
  * **Completeness**: No sections left empty or vestigial. No stale references.
  * **Accuracy**: Every claim matches the code.
- If the checklist fails, revise and re-check (up to 2 iterations).

### 7. Commit
- Export via `fm export`.
- If any spec content or metadata was changed, commit with message `reconcile(<stem>): align spec with codebase`.
- If only the status changed (spec was already accurate), commit with message `reconcile(<stem>): mark as proven`.

IMPORTANT:
- Do NOT modify any source code. You are only updating specs.
- Do NOT skip sections. Every section of every spec gets verified.
- You MUST use `fm` to read AND update specs. Do NOT touch spec markdown directly.
- Use subagents for reading code in parallel where it makes sense, but do not spawn more than 3 concurrent subagents.
- Process only ONE spec per iteration. End after committing.
