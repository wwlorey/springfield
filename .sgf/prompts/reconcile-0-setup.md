(You are in the SETUP phase of the reconcile cursus.)

Set all specs to `draft` status to mark them as needing reconciliation.

## Process

1. Run `fm list --json` to get all specs.
2. For each spec, set its status to `draft`: `fm update <stem> --status draft`
3. Export and commit: `fm export` then commit with message `reconcile(setup): mark all specs as draft for reconciliation`.
4. Touch `.iter-complete`.
