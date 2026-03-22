# springfield

CLI entry point for [Springfield](../../README.md). All developer interaction goes through this binary. It handles project scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle. Delegates iteration execution to [ralph](../ralph/) and persistent memory to [pensa](../pensa/).

## Commands

```
sgf <command> [spec] [-a | -i] [-n N] [--no-push]   — run a prompt-driven command
sgf init [--force]                                    — scaffold a new project
sgf resume [loop_id]                                  — resume a previous session
sgf logs <loop-id>                                    — tail a running loop's output
sgf status                                            — show project state (future work)
```

### `sgf resume`

Resumes a previously interrupted or completed session. Without arguments, presents an interactive picker showing recent sessions (cursus runs first, then legacy sessions). With a `loop_id`, resumes that specific run directly. Resume always launches in interactive mode regardless of the original session's mode.

### Command Resolution

`sgf <command>` resolves to `.sgf/prompts/<command>.md`. If no matching prompt file is found, aliases defined in `.sgf/prompts/config.toml` are checked. Built-in commands (`init`, `logs`, `status`) take priority over prompt files.

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | per-command config | AFK mode: NDJSON stream parsing with formatted output |
| `-i` / `--interactive` | per-command config | Interactive mode (mutually exclusive with `-a`) |
| `-n` / `--iterations` | per-command config | Number of iterations |
| `--no-push` | per-command config | Disable auto-push after commits |

### config.toml

Per-command defaults live in `.sgf/prompts/config.toml`. Each section is a TOML table keyed by command name:

```toml
[build]
alias = "b"              # short alias (e.g., `sgf b` → `sgf build`)
mode = "interactive"      # default mode: "interactive" or "afk"
iterations = 30           # default iteration count
auto_push = true          # auto-push after commits (--no-push overrides)
```

CLI flags override config values. If no config exists for a command, defaults are: mode `interactive`, iterations `1`, auto_push `false`.

## Architecture

```
crates/springfield/
├── src/
│   ├── main.rs          — clap CLI skeleton, dynamic command dispatch
│   ├── lib.rs           — module declarations
│   ├── config.rs        — config.toml parsing, alias resolution, validation
│   ├── init.rs          — project scaffolding (sgf init)
│   ├── prompt.rs        — template validation, path resolution
│   ├── loop_mgmt.rs     — loop ID generation, PID files, log teeing
│   ├── recovery.rs      — pre-launch cleanup of crashed iterations
│   ├── orchestrate.rs   — ralph process lifecycle, flag translation, signal handling
│   ├── style.rs         — terminal output styling (errors, warnings)
├── templates/           — embedded prompt templates and backpressure doc
└── tests/               — integration tests
```

### Key Flows

**Scaffolding** (`sgf init`): Creates `.sgf/`, `.pensa/`, `specs/`, prompt templates, `memento.md`, `CLAUDE.md`, `specs/README.md`. Merges `.gitignore` entries, `.claude/settings.json` deny rules and native sandbox configuration, and `.pre-commit-config.yaml` hooks idempotently.

**Prompt delivery**: Validates that `.sgf/prompts/<command>.md` exists (directly or via alias), and for spec-dependent commands, that `specs/<spec>.md` exists. Passes the raw template path directly to ralph or `cl` — no assembly or preprocessing.

**Loop orchestration** (`sgf <command>`): Loads config.toml, merges CLI flags with per-command defaults, runs pre-launch recovery, starts the pensa daemon, validates the prompt, generates a loop ID, writes a PID file, launches ralph with translated flags, tees output in AFK mode, handles exit codes, cleans up PID file.

**Recovery**: Scans `.sgf/run/` for stale PID files. If all PIDs are dead, runs `git checkout -- .`, `git clean -fd`, and `pn doctor --fix` to reset dirty state from crashed iterations.

## Quick Start

```sh
# Build
cargo build -p springfield

# Run tests
cargo test -p springfield

# Scaffold a new project
sgf init

# Run a build loop in AFK mode (alias: sgf b auth -a)
sgf build auth -a

# Tail a running loop's logs
sgf logs build-auth-20260228T100000
```

## Relationship to Other Crates

- **[pensa](../pensa/)** — Agent persistent memory. `sgf` starts the pensa daemon before loops and uses `pn` for recovery (`pn doctor --fix`).
- **[ralph](../ralph/)** — Loop runner. `sgf` invokes ralph as a subprocess, passing raw prompt paths, flags, and loop configuration. Ralph invokes `cl` (claude-wrapper) which handles layered context injection.

See the [full specification](../../specs/springfield.md) for detailed behavior.
