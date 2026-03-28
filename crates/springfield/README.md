# springfield

CLI entry point for [Springfield](../../README.md). All developer interaction goes through this binary. It handles project scaffolding, cursus pipeline orchestration, loop management, recovery, and daemon lifecycle. Includes a built-in iteration runner (`iter_runner/`) and persistent memory via [pensa](../pensa/).

## Commands

```
sgf <command> [spec] [-a | -i] [-n N] [--no-push]   — run a cursus pipeline
sgf init [--force]                                    — scaffold a new project
sgf resume [run-id]                                   — resume a stalled/interrupted run
sgf logs <loop-id>                                    — tail a running loop's output
sgf list                                              — show available commands
```

### `sgf resume`

Resumes a previously interrupted or completed session. Without arguments, presents an interactive picker showing recent sessions (cursus runs first, then legacy sessions). With a `run-id`, resumes that specific run directly. Resume always launches in interactive mode regardless of the original session's mode. Also supports cursus-specific stall recovery.

### Command Resolution

`sgf <command>` resolves via cursus pipeline definitions. Resolution order:

1. Reserved built-ins: `init`, `list`, `logs`, `resume`, `status`
2. `./.sgf/cursus/<command>.toml` (project-local override)
3. `~/.sgf/cursus/<command>.toml` (global default)
4. Alias match across all resolved cursus definitions
5. Error: `unknown command: <command>`

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-a` / `--afk` | per-iter config | AFK mode: NDJSON stream parsing with formatted output |
| `-i` / `--interactive` | per-iter config | Interactive mode (mutually exclusive with `-a`) |
| `-n` / `--iterations` | per-iter config | Number of iterations |
| `--no-push` | per-iter config | Disable auto-push after commits |

CLI flags override cursus TOML values for all iters in a run.

## Architecture

```
crates/springfield/
├── src/
│   ├── main.rs          — clap CLI skeleton, dynamic command dispatch
│   ├── lib.rs           — module declarations
│   ├── init.rs          — project scaffolding (sgf init)
│   ├── prompt.rs        — template validation, path resolution
│   ├── loop_mgmt.rs     — loop ID generation, PID files, log teeing
│   ├── recovery.rs      — pre-launch cleanup of crashed iterations
│   ├── orchestrate.rs   — agent process lifecycle, flag translation, signal handling
│   ├── style.rs         — terminal output styling (errors, warnings)
│   ├── iter_runner/     — built-in iteration runner
│   │   ├── mod.rs       — core loop: spawn agent, check sentinels, iteration control
│   │   ├── banner.rs    — iteration banner rendering
│   │   ├── format.rs    — NDJSON stream formatting for AFK mode
│   │   └── style.rs     — iteration-specific terminal styling
│   └── cursus/          — declarative pipeline orchestration
│       ├── mod.rs       — pipeline entry point
│       ├── runner.rs    — stage execution and transitions
│       ├── state.rs     — pipeline state persistence
│       ├── context.rs   — context passing between stages
│       └── toml.rs      — TOML pipeline definition parsing
└── tests/               — integration tests
```

### Cursus Module

The `cursus/` module handles declarative pipeline orchestration. It parses cursus TOML definitions from `.sgf/cursus/`, validates iter configurations and transitions, executes iters sequentially via the iteration runner, manages context passing between iters (`produces`/`consumes`), persists run state for resume, and handles sentinel-based transitions and stall recovery. See the [main README](../../README.md#cursus-pipelines) for the full cursus TOML format and pipeline documentation.

### Iteration Runner

The iteration runner (`iter_runner/`) executes prompt-driven loops directly within the `sgf` process. It spawns the configured agent command (defaults to `cl`, the claude-wrapper binary), monitors for sentinel files (`.iter-complete` to signal completion, `.iter-ding` for notifications), and manages iteration counting, auto-push, and AFK-mode output formatting. Both the cursus runner and simple prompt mode use the iteration runner.

### Key Flows

**Scaffolding** (`sgf init`): Creates `.sgf/`, `.pensa/`, prompt templates, `MEMENTO.md`, `CLAUDE.md`, `BACKPRESSURE.md`. Merges `.gitignore` entries, `.claude/settings.json` deny rules and native sandbox configuration, and `.pre-commit-config.yaml` hooks idempotently.

**Prompt delivery**: Validates that `.sgf/prompts/<prompt>.md` exists (as referenced in the cursus iter's `prompt` field). Passes the raw template path directly to the agent command (`cl`).

**Pipeline orchestration** (`sgf <command>`): Resolves the command to a cursus TOML definition, runs pre-launch recovery, starts the pensa daemon, generates a loop ID, writes a PID file, executes the cursus pipeline (iter by iter via the iteration runner), handles sentinel-based transitions and context passing, tees output in AFK mode, manages stall recovery, and cleans up on completion.

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
- **[claude-wrapper](../claude-wrapper/)** — Agent wrapper. The iteration runner invokes `cl` (claude-wrapper) as the default agent command, which handles layered `.sgf/` context injection.

See `fm show springfield --json` for the full specification.
