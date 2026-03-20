# claude-wrapper (`cl`)

Agent wrapper that injects layered `.sgf/` context into every Claude Code invocation.

## What it does

`cl` resolves context files (`MEMENTO.md`, `BACKPRESSURE.md`) from a two-tier lookup (project-local `./.sgf/` then global `~/.sgf/`), constructs a `--append-system-prompt` argument with `study @<file>` directives, and execs `claude-wrapper-secret` with the injected context plus all passthrough arguments.

## Installation

```
cargo install --path crates/claude-wrapper
```

This installs the `cl` binary.

## Usage

```
cl [any args...]
```

All arguments are forwarded to `claude-wrapper-secret`. If context files are found, a `--append-system-prompt` argument is prepended automatically.

### Context file resolution

For each context file, `cl` checks:

1. `./.sgf/<file>` (project-local override)
2. `~/.sgf/<file>` (global default)

The first existing path wins. If neither exists, the file is skipped with a warning to stderr.

### Downstream binary

`cl` execs `claude-wrapper-secret`, which must be available in `$PATH`. This is an opaque, user-provided binary (e.g., a shell script that sets API keys or selects models before calling `claude`). If not found, `cl` exits with code 1.

## Design goals

- **Single entry point**: All Claude Code invocations go through `cl`
- **Layered config**: Project-local `.sgf/` overrides global `~/.sgf/` per file
- **Opaque downstream**: `cl` knows nothing about what `claude-wrapper-secret` does
- **Testable**: Context resolution is a pure function; the binary never calls `claude` directly
