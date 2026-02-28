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

## Architecture

```
springfield/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── springfield/           — CLI binary (`sgf`), entry point, scaffolding, prompt assembly
│   ├── pensa/                 — agent persistent memory (CLI binary + library)
│   └── ralph/                 — loop runner (standalone binary)
```

**`springfield`** (binary: `sgf`) — The CLI entry point. All developer interaction goes through this binary. It delegates to the other crates internally. Also responsible for project scaffolding.

**`pensa`** (Latin: "tasks", singular: pensum) — A Rust CLI that serves as the agent's persistent structured memory. Replaces markdown-based issue logging and implementation plan tracking. Inspired by [beads](https://github.com/steveyegge/beads) but built in Rust with tighter integration into the Springfield workflow. Stores issues with typed classification, dependencies, priorities, ownership, and status tracking. Uses SQLite locally with JSONL export for git portability. Why not [Dolt](https://github.com/dolthub/dolt)? SQLite + JSONL is simpler: SQLite is tiny, JSONL travels with git (no DoltHub remote needed), and `rusqlite` is mature. Dolt's strengths (table-level merges, branching) matter more in multi-user scenarios.

**`ralph`** — The loop runner. Executes Claude Code iteratively against a prompt file inside Docker sandboxes. Supports interactive mode (terminal passthrough with notification sounds) and AFK mode (NDJSON stream parsing with formatted output). Standalone binary — `sgf` invokes it as a subprocess, assembling prompts and passing them as arguments. Originally developed in the [buddy-ralph](../buddy-ralph/ralph/) project; copied into this workspace as a clean break with full ownership.

### Sandboxing

All sessions run inside Docker Desktop sandboxes (`docker sandbox run`), including human-in-the-loop stages like `sgf spec` and `sgf issues log`.

**Docker over native sandbox**: Claude Code's built-in sandbox (`/sandbox`) provides OS-level filesystem and network isolation, but bundles them together — network access requires per-domain approval, which is incompatible with AFK/unattended operation. Docker provides the filesystem isolation we need (protecting the host machine) while leaving network access unrestricted, which is the right tradeoff for an autonomous runner. The template image overhead is worth it.

**How sandboxes work**: Docker sandboxes are microVMs (not containers). The workspace directory syncs bidirectionally between host and VM at the same absolute path — `/Users/you/project` on the host appears at `/Users/you/project` inside the sandbox. This is file synchronization, not volume mounting. Changes the agent makes sync back to the host; changes on the host sync into the VM.

## References

- [Ralph Wiggum technique](https://github.com/ghuntley/how-to-ralph-wiggum)
- [Beads — graph issue tracker for AI agents](https://github.com/steveyegge/beads)
- [Dolt — version-controlled SQL database](https://github.com/dolthub/dolt)
- [prek — Rust-based git hook manager](https://github.com/j178/prek)
- [Ralph implementation (buddy-ralph)](../buddy-ralph/ralph/)
- [buddy-ralph project structure](../buddy-ralph/) — reference implementation of the manual workflow Springfield codifies
- [Loom specs/README.md](https://github.com/ghuntley/loom/blob/trunk/specs/README.md) — reference format for spec index tables
