# Springfield (`sgf`)

A suite of Rust tools for orchestrating AI-driven software development using iterative agent (Ralph) loops. The CLI entry point is `sgf`.

## Origin & Workflow

Springfield codifies a workflow inspired by Geoffrey Huntley and the [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum) — breaking projects into well-scoped tasks and executing them through tight, single-task ralph loops with fresh context each iteration.

The workflow grew out of hands-on experience building projects with Claude Code. The manual process involves:

1. Running a Claude Code session that interviews the developer, then generates specs and an implementation plan
2. Running iterative ralph loops in interactive mode for a few supervised rounds
3. Switching to AFK mode and letting loops run autonomously
4. Running verification loops that certify the codebase adheres to specs
5. Running test plan generation loops that produce test items
6. Running test execution loops that run test items and produce a test report
7. Revising specifications — because verification revealed gaps or because the developer wants to add or change features — then generating new plan items and re-entering the build cycle

The process is cyclical: discuss → build → verify → revise specs → build again. Step 7 is always human-in-the-loop — the developer re-enters a discussion session to update specs and create new plan items for the delta.

Each stage uses a different prompt. Today these prompts are manually selected and kicked off, and live as near-duplicate markdown files across projects.

The agent's persistent memory system, pensa, is inspired by Steve Yegge's [beads](https://github.com/steveyegge/beads) — rebuilt in Rust with tighter integration into the Springfield workflow.

## Problems Solved

- **Manual orchestration** — switching prompts and kicking off stages by hand
- **Prompt duplication** — near-identical prompt files across projects with minor per-project caveats sprinkled in
- **Messy issue tracking** — markdown-based issue logging is unreliable for agents, who struggle with the multi-step process of creating directories, writing files, and following naming conventions
- **No persistent structured memory** — agents lose context between sessions and have no reliable way to track work items and issues across loop iterations
- **No unified monitoring** — no way to observe multiple loops across projects

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (edition 2024)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (invoked by ralph via `$AGENT_CMD`)
- [prek](https://github.com/j178/prek) (git hook manager — runs `pn export`/`pn import` hooks)

### Install

```sh
cargo install --path crates/pensa
cargo install --path crates/ralph
cargo install --path crates/springfield
```

### Scaffold a Project

```sh
cd your-project
sgf init
```

This creates `.sgf/`, `.pensa/`, `specs/`, prompt templates, `MEMENTO.md`, `CLAUDE.md`, `BACKPRESSURE.md`, and merges entries into `.gitignore`, `.claude/settings.json` (including native sandbox configuration), and `.pre-commit-config.yaml`.

`BACKPRESSURE.md` lives at the project root (not inside `.sgf/`) so it is discoverable by ralph's `study` instruction and `$AGENT_CMD` wrappers.

Then install the git hooks:

```sh
prek install
```

### Usage

Every `sgf <command>` resolves to a **cursus** pipeline definition (see [Cursus Pipelines](#cursus-pipelines) below). Built-in commands (`init`, `logs`, `resume`, `status`) are resolved first; everything else maps to a `.toml` file in `.sgf/cursus/`.

```sh
sgf spec                    # spec creation and refinement (multi-iter pipeline)
sgf build [spec]            # run build loop (interactive)
sgf build [spec] -a         # run build loop in AFK mode (unattended)
sgf verify                  # verify codebase against specs
sgf test-plan               # generate test items
sgf test [spec]             # execute test items
sgf doc                     # documentation generation
sgf issues-log              # log bugs interactively
sgf logs <loop-id>          # tail a running loop's output
sgf resume                  # resume a previous session or stalled cursus run
sgf resume <run-id>         # resume a specific session or cursus run by ID
```

CLI flags apply to all iters in a cursus:
- `-a` — force AFK mode on all iters
- `-i` — force interactive mode on all iters
- `-n <count>` — override iteration count on all iters
- `--no-push` — disable auto-push on all iters

### Session Resume

Springfield persists Claude Code session IDs so you can resume interrupted or completed sessions. Each iteration within a loop gets its own session ID. Session metadata is stored as JSON sidecar files in `.sgf/run/{loop_id}.json` (gitignored).

**How it works:**

1. Before each iteration, a fresh UUID is generated and passed to `cl` via `--session-id <uuid>`
2. After each iteration, sgf appends the iteration's session ID and completion timestamp to `.sgf/run/{loop_id}.json`
3. `sgf resume` reads the metadata, flattens all iterations across all loops into a single list, and passes `--resume <session_id>` to `cl` for the selected iteration

**Usage:**

```sh
sgf resume              # show interactive picker of recent sessions
sgf resume <loop-id>    # show picker for iterations within a specific loop
```

The interactive picker displays a flat list of all iteration sessions across all loops, sorted newest-first (capped at 20 entries):

```
Recent sessions:
  1. build-20260316T162408  iter 2   afk          completed    2m ago
  2. build-20260316T162408  iter 1   afk          completed    2m ago
  3. spec-20260316T120000   iter 1   interactive  interrupted  1h ago
Select session (1-3):
```

When a `loop-id` is provided, the picker shows only the iterations for that loop. If the loop has a single iteration, it resumes directly without showing the picker.

Resumed sessions always run in interactive mode (full terminal passthrough) regardless of the original mode.

`sgf resume` also supports cursus-specific stall recovery — see [Stall Recovery](#stall-recovery) below.

**Ralph flags:** ralph accepts `--session-id <uuid>` (for new sessions) and `--resume <session_id>` (for resuming), both passed through to `cl`.

### Cursus Pipelines

A **cursus** (Latin: "a running, course, path") is a declarative pipeline comprising one or more **iters** (Latin: "journey, passage") — discrete execution stages that run sequentially. Cursus definitions are TOML files in `.sgf/cursus/` (project-local) or `~/.sgf/cursus/` (global defaults). Local definitions override global ones.

Every `sgf <command>` resolves to a cursus definition. Simple commands like `build` are single-iter cursus definitions. Complex workflows like `spec` are multi-iter pipelines with transitions, context passing, and review loops.

#### TOML Format

Each `.toml` file in `.sgf/cursus/` defines one cursus. The filename (minus `.toml`) is the command name.

**Top-level fields:**

```toml
description = "Spec creation and refinement"
alias = "s"           # optional short alias
trigger = "manual"    # only "manual" is supported currently
auto_push = true      # auto-push after commits (default: false)
```

**Iters** are defined as an array of tables:

```toml
[[iters]]
name = "build"
prompt = "build.md"        # resolved via layered .sgf/prompts/ lookup
mode = "interactive"       # "interactive" (default) or "afk"
iterations = 30            # max ralph iterations (default: 1)
auto_push = true           # override cursus-level auto_push (optional)
```

#### Single-Iter Cursus

A single-iter cursus is the simplest form — equivalent to a `config.toml` entry. Example `build.toml`:

```toml
description = "Implementation loop"
alias = "b"
auto_push = true

[[iters]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
```

#### Multi-Iter Cursus

Multi-iter cursus definitions chain stages together. Example `spec.toml` for spec creation and refinement:

```toml
description = "Spec creation and refinement"
alias = "s"
auto_push = true

[[iters]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
produces = "discuss-summary"
auto_push = false

[[iters]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
produces = "draft-presentation"
consumes = ["discuss-summary"]

[[iters]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iters.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iters]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iters]]
name = "approve"
prompt = "spec-approve.md"
mode = "interactive"
consumes = ["draft-presentation"]
```

#### Context Passing

Iters share information via **produces** and **consumes**:

- An iter with `produces = "discuss-summary"` writes a summary file to `.sgf/run/<run-id>/context/discuss-summary.md`
- An iter with `consumes = ["discuss-summary"]` receives that file's content injected into its system prompt
- When multiple iters produce the same key, the later iter's file overwrites the earlier one (e.g., `revise` updates `draft-presentation`)

The run context directory is also available as `SGF_RUN_CONTEXT` in the environment.

#### Sentinel-Based Transitions

After each iter completes, cursus checks for sentinel files (in priority order):

| Sentinel File | Meaning | Behavior |
|---------------|---------|----------|
| `.ralph-complete` | Iter succeeded | Advance to next iter (or `next` override). Final iter → pipeline complete |
| `.ralph-reject` | Reviewer rejected | Follow `on_reject` transition |
| `.ralph-revise` | Minor revision needed | Follow `on_revise` transition |
| (none, iterations exhausted) | Stalled | Pipeline enters stalled state |

Interactive iters with `iterations = 1` that end without a sentinel are treated as successfully completed.

Transitions are defined in a `[iters.transitions]` table and must reference an existing iter name in the same cursus:

```toml
[iters.transitions]
on_reject = "draft"     # jump back to draft on rejection
on_revise = "revise"    # jump to revise for minor changes
```

The `next` field overrides the default sequential flow:

```toml
next = "review"         # after this iter, go to review instead of the next in the list
```

#### Stall Recovery

When an iter exhausts its iterations without producing a sentinel file, the pipeline stalls:

```
╭─ Cursus STALLED ─────────────────────────────────╮
│  Cursus:    spec                                  │
│  Iter:      draft                                 │
│  Reason:    Iterations exhausted (10/10)          │
│                                                   │
│  To resume: sgf resume spec-20260317T140000       │
╰───────────────────────────────────────────────────╯
```

Resume with `sgf resume <run-id>` to retry the stalled iter, skip to the next iter, or abort the run.

#### Command Resolution

Resolution order for `sgf <command>`:

1. Reserved built-ins: `init`, `logs`, `resume`, `status`
2. `./.sgf/cursus/<command>.toml` (project-local override)
3. `~/.sgf/cursus/<command>.toml` (global default)
4. Alias match across all resolved cursus definitions
5. `config.toml` fallback (legacy, removed once migration is complete)
6. Error: `unknown command: <command>`

#### Migration from `config.toml`

Each `[section]` in the old `.sgf/prompts/config.toml` becomes its own cursus TOML in `.sgf/cursus/`:

| Old (`config.toml`) | New (`.sgf/cursus/`) |
|----------------------|----------------------|
| `[build]` | `build.toml` |
| `[spec]` | `spec.toml` (multi-iter) |
| `[verify]` | `verify.toml` |
| `[test]` | `test.toml` |
| `[test-plan]` | `test-plan.toml` |
| `[doc]` | `doc.toml` |
| `[issues-log]` | `issues-log.toml` |

Cursus definitions take precedence over `config.toml`. Both resolution paths coexist during the transition.

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
│   ├── springfield/           — CLI binary (`sgf`), entry point, scaffolding, cursus orchestration
│   │   └── src/cursus/        — cursus pipeline engine (TOML parsing, execution, state, context)
│   ├── pensa/                 — agent persistent memory (CLI binary + library)
│   └── ralph/                 — loop runner (standalone binary)
```

**`springfield`** (binary: `sgf`) — The CLI entry point. All developer interaction goes through this binary. It delegates to the other crates internally. Responsible for project scaffolding and cursus pipeline orchestration. The `cursus` module handles TOML parsing/validation, iter execution, sentinel detection, context passing, and run state persistence.

**`pensa`** (Latin: "tasks", singular: pensum) — A Rust CLI that serves as the agent's persistent structured memory. Replaces markdown-based issue logging and implementation plan tracking. Inspired by [beads](https://github.com/steveyegge/beads) but built in Rust with tighter integration into the Springfield workflow. Stores issues with typed classification, dependencies, priorities, ownership, and status tracking. Uses SQLite locally with JSONL export for git portability. Why not [Dolt](https://github.com/dolthub/dolt)? SQLite + JSONL is simpler: SQLite is tiny, JSONL travels with git (no DoltHub remote needed), and `rusqlite` is mature. Dolt's strengths (table-level merges, branching) matter more in multi-user scenarios.

**`ralph`** — The loop runner. Invokes `$AGENT_CMD` directly (no Docker) to execute Claude Code iteratively against a prompt file. Supports interactive mode (terminal passthrough with notification sounds) and AFK mode (NDJSON stream parsing with formatted output). Operates within Claude Code's native OS-level sandbox — ralph overrides `allowUnsandboxedCommands` to `false` via `--settings`, preventing automated agents from escaping sandbox bounds. Standalone binary — `sgf` invokes it as a subprocess, passing prompt paths and environment variables. Ralph reads `PROMPT_FILES` and passes a `study @<file>` instruction via `--append-system-prompt` to Claude Code, ensuring the agent actively reads and processes context files. Originally developed in the [buddy-ralph](../buddy-ralph/ralph/) project; copied into this workspace as a clean break with full ownership.

### Prompt Delivery and System Prompt Injection

sgf does not assemble or preprocess prompts. Templates in `.sgf/prompts/` are passed directly to ralph or `$AGENT_CMD`.

**Automated stages** (`build`, `verify`, `test-plan`, `test`) go through ralph, which owns system prompt injection. Ralph reads the `PROMPT_FILES` env var (default: `$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md`) and passes `--append-system-prompt` with a `study @<file>` instruction for each entry to Claude Code.

**Interactive stages** (`spec`, `issues log`) call `$AGENT_CMD` directly — no ralph wrapper. The `$AGENT_CMD` wrapper (default: `claude`) is responsible for system prompt injection (e.g., reading `PROMPT_FILES` and passing `--append-system-prompt` with `study` instructions).

## References

- [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)
- [Beads — graph issue tracker for AI agents](https://github.com/steveyegge/beads)
- [Dolt — version-controlled SQL database](https://github.com/dolthub/dolt)
- [prek — Rust-based git hook manager](https://github.com/j178/prek)
- [Ralph implementation (buddy-ralph)](../buddy-ralph/ralph/)
- [buddy-ralph project structure](../buddy-ralph/) — reference implementation of the manual workflow Springfield codifies
- [Loom specs/README.md](https://github.com/ghuntley/loom/blob/trunk/specs/README.md) — reference format for spec index tables
