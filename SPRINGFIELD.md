# Springfield — Design Document

## What Is Springfield?

Springfield is a suite of Rust tools for orchestrating AI-driven software development using iterative AI agent loops. It codifies a workflow inspired by the [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum) — breaking projects into well-scoped tasks and executing them through tight, single-task loops with fresh context each iteration.

The CLI entry point is `sgf`.

---

## Origin & Motivation

The workflow that Springfield formalizes has been developed through hands-on experience building projects with Claude Code. The current manual process involves:

1. Running a Claude Code session that interviews the developer, then generates specs and an implementation plan
2. Running iterative Claude Code loops (via a tool called Ralph) in interactive mode for a few supervised rounds
3. Switching to AFK mode and letting it run autonomously
4. Running verification loops that certify the codebase adheres to specs
5. Running test plan generation and verification loops that certify the code functions as intended
6. Revising specifications — either because verification revealed gaps or because the developer wants to add/change features — then generating new implementation plan items and re-entering the build cycle

The process is cyclical: discuss → build → verify → revise specs → build again. Step 6 is always human-in-the-loop — the developer re-enters a discussion session to update existing specs and create new plan items for the delta.

Each stage uses a different prompt. Today these prompts are manually selected and kicked off, and live as near-duplicate markdown files across projects.

The problems Springfield solves:
- **Manual orchestration**: switching prompts and kicking off stages by hand
- **Prompt duplication**: near-identical prompt files across projects with minor per-project caveats sprinkled in
- **Messy issue tracking**: markdown-based issue logging is unreliable for agents — they struggle with the multi-step process of creating directories, writing files, and following naming conventions
- **No persistent structured memory**: agents lose context between sessions and have no reliable, non-markdown way to track work items and issues across loop iterations
- **No unified monitoring**: no way to observe multiple loops across projects

---

## Architecture

### Workspace Structure

```
springfield/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── sgf/                   — CLI binary, entry point, scaffolding, prompt assembly
│   ├── pensa/                 — agent persistent memory (CLI binary + library)
│   └── ralph/                 — loop runner (library + binary)
```

### Components

**`sgf`** — The CLI entry point. All developer interaction goes through this binary. It delegates to the other crates internally. Also responsible for project scaffolding and memento generation.
- `sgf init` — scaffolding project structure, seeding prompt templates into `.sgf/prompts/`
- Memento generation — scanning project state and specs to build the lookup table

**`pensa`** (Latin: "tasks", singular: pensum) — A Rust CLI that serves as the agent's persistent structured memory. Replaces markdown-based issue logging and implementation plan tracking. Inspired by [beads](https://github.com/steveyegge/beads) but built in Rust with tighter integration into the Springfield workflow. Stores issues with tags, dependencies, priorities, ownership, and status tracking. Uses SQLite locally with JSONL export for git portability.

**`ralph`** — The loop runner. Executes Claude Code iteratively against a prompt file inside Docker sandboxes. Supports interactive mode (terminal passthrough with notification sounds) and AFK mode (NDJSON stream parsing with formatted output). Exists as both a library crate (called by `sgf build`) and a standalone binary for direct use. Originally developed in the [buddy-ralph](../buddy-ralph/ralph/) project.

---

## Pensa — Agent Persistent Memory

### Purpose

Give agents a CLI-accessible, structured way to log issues and track work items that persists across sessions. A single command like `pn create "login crash on empty password" -p 0 -t bug` replaces the error-prone multi-step process of creating directories and writing markdown files.

### Storage Model

Dual-layer storage (same pattern as beads):
- **`.pensa/db.sqlite`** — the working database, gitignored. Rebuilt from JSONL on clone.
- **`.pensa/issues.jsonl`** — the git-committed export. Human-readable, diffs cleanly.

Sync is automated via prek (git hooks):
- **Pre-commit hook**: runs `pn export` to write SQLite → JSONL
- **Post-merge hook**: runs `pn import` to rebuild JSONL → SQLite

**Why not Dolt?** Dolt (version-controlled SQL database) was evaluated as an alternative that would eliminate the dual-layer sync. However, SQLite + JSONL is the better fit: SQLite is tiny and ubiquitous (no extra binary in Docker sandboxes), JSONL travels with the project's git repo (no second remote like DoltHub needed), and `rusqlite` is a mature Rust integration (vs. shelling out to `dolt sql -q`). Dolt's strengths — native table-level merges, built-in branching — matter more in multi-user scenarios, which Springfield doesn't target. The sync hooks are a few lines of prek config.

### Schema

Everything is an issue (following the GitHub model). Issues are distinguished by tags rather than separate entity types.

Each issue has: ID (hash-based to prevent merge collisions), title, description, status, priority, tags, dependencies, owner, timestamps.

**Tags** (freeform, starting set):
- **`bug`** — problems discovered during build/verify
- **`task`** — implementation plan items from the spec phase
- **`chore`** — tech debt, refactoring, dependency updates, CI fixes

### CLI Commands

```
pn create "title" -p <priority> [-t bug|task|chore]
pn ready [--json]              # show unblocked issues
pn update <id> --claim         # take ownership, mark in-progress
pn close <id> --reason "..."   # complete with reasoning
pn show <id> [--json]          # full details + history
pn list [--json] [-t <tag>]    # list all, optionally filter by tag
pn dep add <child> <parent>    # wire up dependencies
pn sync                        # export SQLite → JSONL
pn import                      # rebuild SQLite from JSONL
```

All commands support `--json` for agent consumption.

### Git Hooks (via prek)

[prek](https://github.com/j178/prek) is used for git hook management. `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: Export pensa DB to JSONL
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: Import JSONL to pensa DB
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge]
```

---

## Per-Repo Project Structure

After `sgf init`, a project contains:

```
.pensa/
├── db.sqlite                  (gitignored)
└── issues.jsonl               (committed)
.sgf/
├── config.toml                (committed — stack type, project config)
└── prompts/                   (committed — editable prompt templates)
    ├── build.md
    ├── spec.md
    ├── verify.md
    └── test-plan.md
.pre-commit-config.yaml        (prek hooks for pensa sync)
memento.md                     (generated lookup table)
AGENTS.md                      (hand-authored operational guidance)
CLAUDE.md                      (links to memento + agents)
specs/                         (prose specification files)
```

### File Purposes

**`memento.md`** — A structured lookup table the agent reads at the start of every iteration. Contains:
- Project stack and type
- Backpressure commands (build, test, lint, format)
- Specs index (table mapping each spec to file path, status, and one-line summary)
- Directory structure with one-liner descriptions

This is the agent's map. It reads the memento, knows where everything is, and dives into specific files only when needed. Dense and scannable, not prose. Generated by `sgf` from project state + config.

**`AGENTS.md`** — Hand-authored operational guidance. Contains information that doesn't fit the memento's structured format — code style preferences, runtime notes, special instructions. Linked from CLAUDE.md so Claude Code auto-loads it.

**`CLAUDE.md`** — Entry point for Claude Code. Links to memento.md and AGENTS.md (via `ln -s`).

**`.sgf/config.toml`** — Project-specific configuration:
- `stack` — project type (rust, typescript, tauri, etc.), used by `sgf` to select backpressure templates for memento generation

**`.sgf/prompts/`** — Editable prompt templates for each workflow stage (`spec.md`, `build.md`, `verify.md`, `test-plan.md`). Seeded by `sgf init` from Springfield's built-in templates. Once seeded, the project owns these files — edit them freely to evolve the prompts as you learn what works for the project. To improve defaults for future projects, update the templates in the Springfield repo itself.

**`specs/`** — Prose specification files (one per topic of concern). These are authored documents — written during the spec phase, consumed during builds. Indexed in the memento's specs table.

---

## SGF Commands

```
sgf init                       — scaffold a new project
sgf spec                       — generate specs and implementation plan
sgf build [-a] [iterations]    — run a Ralph loop (interactive or AFK)
sgf verify                     — run verification loop
sgf test-plan                  — run test plan generation loop
sgf status                     — show project state (active loops, pensa summary)
sgf logs <loop-id>             — tail a running loop's output
```

### Deployment Model

**Decentralized**: Springfield is project-aware — it reads `.sgf/` from the current working directory. There is no global registry or central config. Each project is self-contained. To work on multiple projects, run `sgf` from each project directory.

### Sandboxing

All loops run inside Docker sandboxes (same model as Ralph today). The spec phase (`sgf spec`) opens a normal Claude Code session since the human is present and in control.

---

## Workflow Stages

### 1. Spec (`sgf spec`)

Opens a Claude Code session with the spec prompt. The developer provides an outline of what to build, the agent interviews them to fill in gaps, and then generates both deliverables:

1. Write spec files to `specs/`
2. Create implementation plan items via `pn create -t task`, with dependencies and priorities
3. Update `memento.md` with new spec entries
4. Commit and push

The interview and generation happen in a single session. The agent asks clarifying questions as needed, but the goal is always to produce specs and a plan. The prompt instructs the agent to design specs so the result can be end-to-end tested from the command line.

This same workflow applies to adding new features to an existing project — run `sgf spec` again to update specs and create new plan items. Specs are living documents, never sealed/frozen.

### 2. Build (`sgf build`)

Runs a Ralph loop using `.sgf/prompts/build.md` as the prompt. The prompt tells the agent:
1. Read `memento.md` to orient
2. Run `pn ready --json` to find the next unblocked task
3. Claim it with `pn update <id> --claim`
4. Implement it (one task per iteration)
5. Apply full backpressure (build, test, lint — per memento)
6. If issues are discovered: `pn create "description" -t bug`
7. Close the task: `pn close <id> --reason "..."`
8. Commit changes
9. `pn sync`

Each iteration gets fresh context. The pensa database persists state between iterations.

Run interactively first for a few supervised rounds, then switch to AFK mode (`-a`) for autonomous execution.

### 3. Verify (`sgf verify`)

Runs a Ralph loop using `.sgf/prompts/verify.md`. The agent:
1. Reads specs from `memento.md` index
2. Investigates each spec against the actual codebase
3. Marks conformance (matches / partial / missing)
4. Generates a verification report
5. Logs any gaps as pensa issues

### 4. Test Plan (`sgf test-plan`)

Runs a Ralph loop using `.sgf/prompts/test-plan.md`. The agent:
1. Studies specs and codebase
2. Generates a testing plan
3. Ensures tests are automatable (can be run by agents in loops)

### 5. Issue Logging

Not a separate `sgf` command — issues are logged by agents during any stage via `pn create`. The agent can also be instructed to log issues during the discussion phase.

---

## Prompts

Each workflow stage has a corresponding prompt file in `.sgf/prompts/`. These are plain markdown files — edit them directly.

**Seeding**: `sgf init` copies the current prompt templates from Springfield into `.sgf/prompts/`. From that point, the project owns its prompts.

**Editing**: Prompts evolve as you learn what works for a project. Add caveats ("Mac-first builds"), change the workflow ("commit" vs "commit and push"), tune instructions. Edit the files in your editor, read diffs in git — they're just markdown.

**Upstream improvements**: To improve defaults for all future projects, update the templates in the Springfield repo. Existing projects keep their copies and can pull changes manually if desired.

**Backpressure**: Backpressure commands (build, test, lint, format) live in `memento.md`, not in the prompts. The prompts tell the agent to read the memento and apply backpressure — the specific commands are defined once in the memento.

This replaces the duplication seen in buddy-ralph's `prompts/building/` directory where 8 similar files existed with minor variations. Instead of near-duplicate files with caveats sprinkled in, there's one editable copy per project.

---

## Key Design Principles

**Fresh context per iteration**: Each Ralph loop iteration starts with a clean context window. The agent reads the memento and pensa state to orient itself. No accumulated confusion.

**One task per iteration**: The agent picks one unblocked task, implements it fully, applies backpressure, commits, and exits. The loop restarts with fresh context.

**Structured memory over markdown**: Pensa replaces unstructured markdown files for issues and tasks. A single CLI command replaces multi-step file creation. The agent finds this easier and more reliable.

**Editable prompts over duplication**: `sgf init` seeds prompt templates into the project. Each project owns and can evolve its prompts. No near-duplicate files across projects — one editable copy per stage, per project.

**Backpressure drives quality**: Build, test, lint, and format commands (defined in the memento) are applied after every change. Failed validation forces correction before commits.

**Decentralized projects**: Each project is self-contained. No global state, no central server, no coordination between projects. Run `sgf` from the project directory.

**Sandboxed execution**: All autonomous loops run in Docker sandboxes. Human-in-the-loop sessions (spec) run without sandboxes.

---

## Open Questions

- **Build order**: Pensa first (self-contained, agents need it immediately), then sgf init (scaffolding), then sgf spec/build (prompt assembly + ralph integration)?
- **Ralph migration**: Copy ralph's code from buddy-ralph into this workspace, or depend on it externally initially?
- **TUI**: Deferred for now. CLI-first. TUI can be added later as a view layer over the same operations. Desired feel: Neovim-like (modal, keyboard-driven, information-dense, panes for multiple loops).
- **Multi-project monitoring**: Deferred with TUI. For now, multiple terminals.

---

## References

- [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)
- [Beads — graph issue tracker for AI agents](https://github.com/steveyegge/beads)
- [Dolt — version-controlled SQL database](https://github.com/dolthub/dolt)
- [prek — Rust-based git hook manager](https://github.com/j178/prek)
- [Ralph implementation (buddy-ralph)](../buddy-ralph/ralph/)
- [buddy-ralph project structure](../buddy-ralph/) — reference implementation of the manual workflow Springfield codifies
