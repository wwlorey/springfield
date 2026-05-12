(You are in the TAG phase of the reconcile cursus.)

All specs have been reconciled and coherence-checked. Now mark this reconciliation point in git.

## Process

1. Run `fm list --json` to get all spec stems.
2. Get today's date (use `date +%Y-%m-%d`).
3. For each spec stem, create a git tag: `git tag reconcile/<stem>/<date>`
   - If a tag for this stem and date already exists, append a counter: `reconcile/<stem>/<date>-2`, etc.
4. Touch `.iter-complete`.

IMPORTANT:
- Do NOT push tags. The cursus `auto_push` handles pushing commits, but tags should be pushed explicitly by the user if desired.
