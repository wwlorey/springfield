# springfield

CLI entry point for [Springfield](../../README.md). All developer interaction goes through this binary. It handles project scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle. Delegates iteration execution to [ralph](../ralph/) and persistent memory to [pensa](../pensa/).

## Commands

```
sgf init                               — scaffold a new project
sgf spec                               — generate specs and implementation plan (interactive)
sgf build [spec] [-a] [--no-push] [N]  — run build loop
sgf verify [-a] [--no-push] [N]        — run verification loop
sgf test-plan [-a] [--no-push] [N]     — run test plan generation loop
sgf test [spec] [-a] [--no-push] [N]   — run test execution loop
sgf issues log                         — interactive session for logging bugs
sgf status                             — show project state (future work)
sgf logs <loop-id>                     — tail a running loop's output
```

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | `false` | AFK mode: NDJSON stream parsing with formatted output |
| `--no-push` | `false` | Disable auto-push after commits |
| `N` (positional) | `30` | Number of iterations |

## Architecture

```
crates/springfield/
├── src/
│   ├── main.rs          — clap CLI skeleton, command dispatch
│   ├── lib.rs           — module declarations
│   ├── init.rs          — project scaffolding (sgf init)
│   ├── prompt.rs        — template validation, path resolution
│   ├── loop_mgmt.rs     — loop ID generation, PID files, log teeing
│   ├── recovery.rs      — pre-launch cleanup of crashed iterations
│   ├── orchestrate.rs   — ralph process lifecycle, flag translation, signal handling
├── templates/           — embedded prompt templates and backpressure doc
└── tests/               — integration tests
```

### Key Flows

**Scaffolding** (`sgf init`): Creates `.sgf/`, `.pensa/`, `specs/`, prompt templates, `memento.md`, `CLAUDE.md`, `specs/README.md`. Merges `.gitignore` entries, `.claude/settings.json` deny rules and native sandbox configuration, and `.pre-commit-config.yaml` hooks idempotently.

**Prompt delivery**: Validates that `.sgf/prompts/<stage>.md` exists and, for spec-dependent stages, that `specs/<spec>.md` exists. Passes the raw template path directly to ralph or `$AGENT_CMD` — no assembly or preprocessing.

**Loop orchestration** (`sgf build`, `sgf verify`, etc.): Runs pre-launch recovery, starts the pensa daemon, validates the prompt, generates a loop ID, writes a PID file, launches ralph with translated flags, tees output in AFK mode, handles exit codes, cleans up PID file.

**Recovery**: Scans `.sgf/run/` for stale PID files. If all PIDs are dead, runs `git checkout -- .`, `git clean -fd`, and `pn doctor --fix` to reset dirty state from crashed iterations.

## Quick Start

```sh
# Build
cargo build -p springfield

# Run tests
cargo test -p springfield

# Scaffold a new project
sgf init

# Run a build loop in AFK mode
sgf build auth -a

# Tail a running loop's logs
sgf logs build-auth-20260228T100000
```

## Relationship to Other Crates

- **[pensa](../pensa/)** — Agent persistent memory. `sgf` starts the pensa daemon before loops and uses `pn` for recovery (`pn doctor --fix`).
- **[ralph](../ralph/)** — Loop runner. `sgf` invokes ralph as a subprocess, passing raw prompt paths, flags, and loop configuration. Ralph owns system prompt injection via `PROMPT_FILES`.

See the [full specification](../../specs/springfield.md) for detailed behavior.
