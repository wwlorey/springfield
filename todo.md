# Remaining TODO: Unify System Prompt Injection via PROMPT_FILES

## Completed

1. **prompt.rs** — env-var-driven system prompt injection with `prepend_system_files` param
2. **orchestrate.rs** — replaced `no_sandbox` with `interactive`, added `run_interactive_claude()`
3. **main.rs** — updated routing to use `interactive`
4. **ralph** — removed `--no-sandbox` and host execution paths
5. **init.rs** — removed MEMENTO/PENSA scaffolding
6. **test.md template** — fixed PENSA.md reference
7. **springfield.md spec** — updated
8. **ralph.md spec** — updated
9. **Pre-existing clippy fixes** — docker-ctx, pensa client/db collapsible-if

## Remaining

### Dotfiles — update `claude-wrapper` (separate repo/commit)

Update `/Users/william/Repos/dotfiles/.local/bin/claude-wrapper` to read `PROMPT_FILES` env var:

```bash
#!/bin/bash
set -euo pipefail

DEFAULT_FILES="$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md"
IFS=':' read -ra PROMPT_FILES <<< "${PROMPT_FILES:-$DEFAULT_FILES}"

args=()
for f in "${PROMPT_FILES[@]}"; do
  [[ -f "$f" ]] && args+=(--append-system-prompt-file "$f")
done

exec claude-wrapper-secret "${args[@]}" "$@"
```

### Verification (manual)

- `sgf init` in a temp dir — verify `.sgf/MEMENTO.md` and `.sgf/PENSA.md` are NOT created
- Verify assembled prompt for a build stage includes system prompt file contents when `PROMPT_FILES` is set
- Verify ralph `--help` no longer shows `--no-sandbox`
