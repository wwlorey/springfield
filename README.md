# Springfield (`sgf`)

A suite of Rust tools for orchestrating AI-driven software development using iterative agent loops. The CLI entry point is `sgf`.

Springfield codifies a workflow inspired by Geoffrey Huntley and the [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum) — breaking projects into well-scoped tasks and executing them through tight, single-task agent loops with fresh context each iteration. The process is cyclical: discuss → build → verify → revise specs → build again.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (edition 2024)
- [just](https://github.com/casey/just) (command runner)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (invoked by the iteration runner via `$AGENT_CMD`)
- [prek](https://github.com/j178/prek) (git hook manager — runs `pn export`/`pn import` hooks)

### Install

```sh
just install
```

Then symlink your Claude Code binary so the wrapper can find it:

```sh
ln -s $(which claude) ~/.local/bin/claude-wrapper-secret
```

The indirection through `cl` → `claude-wrapper-secret` lets you interpose on every Claude Code invocation — injecting context, enforcing constraints, or adding instrumentation — without modifying the upstream binary.

### Scaffold a Project

```sh
cd your-project
sgf init
```

This creates `.sgf/`, `.pensa/`, `.forma/`, prompt templates, `CLAUDE.md`, and merges entries into `.gitignore`, `.claude/settings.json` (including native sandbox configuration), and `.pre-commit-config.yaml`.

Then install the git hooks:

```sh
prek install
```

### Usage

Commands are either **built-ins** or **cursus pipelines**. Built-in commands (`init`, `list`, `logs`, `resume`) are resolved first; everything else maps to a cursus `.toml` file in `.sgf/cursus/`. You can also pass a prompt file directly.

```sh
sgf <command>               # run a cursus pipeline defined in .sgf/cursus/<command>.toml
sgf <command> -a            # run in AFK mode (unattended)
sgf list                    # show available cursus commands and built-ins
sgf logs <loop-id>          # tail a running loop's output
sgf resume                  # resume a previous session or stalled cursus run
sgf resume <run-id>         # resume a specific run by ID
sgf my-task.md              # run a prompt file as a simple iteration loop
sgf my-task.md -a -n 5      # prompt file with AFK and 5 iterations
```

CLI flags apply to all iters in a cursus:
- `-a` — force AFK mode on all iters
- `-i` — force interactive mode on all iters
- `-n <count>` — override iteration count on all iters
- `--no-push` — disable auto-push on all iters
- `--skip-preflight` — disable all pre-launch checks including recovery and daemon startup

### Cursus Pipelines

A **cursus** (Latin: "a running, course, path") is a declarative pipeline comprising one or more **iters** (Latin: "journey, passage") — discrete execution stages that run sequentially. Cursus definitions are TOML files in `.sgf/cursus/` (project-local) or `~/.sgf/cursus/` (global defaults). Local definitions override global ones. The filename (minus `.toml`) becomes the command name.

Each iter references a **prompt** — a markdown file resolved via layered lookup from `.sgf/prompts/` (project-local then global). Prompts define what the agent does; cursus definitions define how iters are sequenced, how many iterations to run, and whether to run interactively or in AFK mode.

```toml
description = "Example pipeline"
alias = "x"
auto_push = true

[[iter]]
name = "do-work"
prompt = "my-prompt.md"    # resolved from .sgf/prompts/
mode = "interactive"       # "interactive" (default) or "afk"
iterations = 30            # max iterations (default: 1)
```

Multi-iter pipelines support `produces`/`consumes` for context passing between stages, `[iter.transitions]` for conditional branching (e.g., reviewer reject → redraft), and sentinel-based completion detection.

### Development

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

## Architecture

```
springfield/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── springfield/           — CLI binary (`sgf`), entry point, scaffolding, iteration runner, cursus orchestration
│   ├── pensa/                 — agent persistent memory, issue tracker (CLI binary `pn` + library)
│   ├── forma/                 — specification management (CLI binary `fm` + library)
│   ├── claude-wrapper/        — agent wrapper (`cl`), layered .sgf/ context injection
│   ├── shutdown/              — shared graceful shutdown, ChildGuard, ProcessSemaphore
│   └── vcs-utils/             — shared VCS utilities (git HEAD detection, auto-push)
```

**`springfield`** (binary: `sgf`) — The CLI entry point. Responsible for project scaffolding, cursus pipeline orchestration, and the iteration runner (agent spawning, sentinel detection, AFK formatting).

**`pensa`** (binary: `pn`) — The agent's persistent structured memory and issue tracker. Stores issues with typed classification, dependencies, priorities, ownership, and status tracking. Uses SQLite locally with JSONL export for git portability. Inspired by [beads](https://github.com/steveyegge/beads).

**`forma`** (binary: `fm`) — Specification management. Stores specs with typed sections, cross-references, and status tracking. Uses SQLite locally with JSONL export for git portability. The `.forma/` directory holds the database, JSONL exports, and generated markdown.

**`claude-wrapper`** (binary: `cl`) — Agent wrapper that injects layered `.sgf/` context into every Claude Code invocation. Resolves context files from a two-tier lookup (project-local `./.sgf/` then global `~/.sgf/`), constructs `--append-system-prompt` arguments, and execs `claude-wrapper-secret` (see [Install](#install)).

**`shutdown`** — Shared graceful shutdown utilities: `ShutdownController`, `ChildGuard` (RAII child process guard), `ProcessSemaphore`, and `kill_process_group`.

**`vcs-utils`** — Shared VCS utilities for git HEAD detection and auto-push.

## References

- [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)
- [Beads — graph issue tracker for AI agents](https://github.com/steveyegge/beads)
- [prek — Rust-based git hook manager](https://github.com/j178/prek)
