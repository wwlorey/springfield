(You are in the COHERENCE phase of the reconcile cursus.)

All specs have been individually reconciled against the codebase. Your job is to check that the specs are consistent with *each other* — particularly at integration boundaries.

## Process

### 1. Load all specs
- Run `fm list --json` to get all specs.
- Run `fm show <stem> --json` for each spec.
- Run `fm ref list <stem> --json` for each spec to build the full reference graph.

### 2. Check explicit integration boundaries
For each pair of specs connected by `fm ref`:
- Do both specs describe their shared boundary? (e.g., if spec A says it calls an API from spec B, does spec B describe that API?)
- Are shared types, events, error types, and data structures described consistently on both sides?
- Is terminology consistent across the boundary? (e.g., one spec calls it a "callback" and the other calls it an "event handler" for the same mechanism)

### 3. Check implicit integration boundaries
Look for shared resource names across specs — database tables, config keys, file paths, environment variables, message types, or other resources mentioned in multiple specs. These indicate coupling that may not be captured in `fm ref`.
- If you discover an implicit integration that lacks an `fm ref`, add one.
- Verify that both specs describe their interaction with the shared resource consistently.

### 4. Fill gaps
For each gap or inconsistency found:
- Read the code at the specific integration point to determine the ground truth.
- Update the affected spec(s) via `fm` to accurately describe the boundary.
- Ensure both sides of every integration boundary have coverage.
- Leave all modified specs as `proven` — do not change their status.

### 5. Self-verify
- For each spec you modified:
  * Re-read the spec via `fm show <stem> --json`.
  * Re-read the code at the specific integration points you edited.
  * Confirm: does the spec still accurately describe what the code does?
  * Confirm: does the integration boundary read consistently from both sides?

### 6. Commit
- Export via `fm export`.
- If any specs were modified, commit with message `reconcile(coherence): align integration boundaries across specs`.
- If no specs needed changes (all boundaries were already consistent), skip the commit.
- Touch `.iter-complete`.

IMPORTANT:
- Do NOT modify any source code.
- You MUST use `fm` to read AND update specs. Do NOT touch spec markdown directly.
- When in doubt about what the code does at a boundary, READ THE CODE. Do not guess.
- Use subagents for reading specs and code in parallel, but do not spawn more than 3 concurrent subagents.
