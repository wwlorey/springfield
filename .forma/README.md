# Specifications

| Spec | Src | Status | Purpose |
|------|-----|--------|--------|
| [claude-wrapper](specs/claude-wrapper.md) | `crates/claude-wrapper/` | stable | Agent wrapper — layered .sgf/ context injection, cl binary |
| [cursus](specs/cursus.md) | `crates/springfield/` | stable | Pipeline orchestration — declarative TOML-defined multi-iter workflows with context passing, sentinel-based transitions, and stall recovery |
| [forma](specs/forma.md) | `crates/forma/` | stable | Specification management — forma daemon and fm CLI |
| [pensa](specs/pensa.md) | `crates/pensa/` | stable | Agent persistent memory — SQLite-backed issue/task tracker with pn CLI |
| [ralph](specs/ralph.md) | `crates/ralph/` | stable | Iterative Claude Code runner — invokes cl (claude-wrapper) with NDJSON formatting, completion detection, and git auto-push |
| [session-resume](specs/session-resume.md) | `crates/springfield/` | stable | Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via sgf resume |
| [shutdown](specs/shutdown.md) | `crates/shutdown/` | proven | Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts |
| [springfield](specs/springfield.md) | `crates/springfield/` | stable | CLI entry point — scaffolding, prompt delivery, loop orchestration, recovery, and daemon lifecycle |
| [test-harness](specs/test-harness.md) | `crates/springfield/tests/` | stable | Integration test harness — shared fixtures, concurrency control, and mock infrastructure for springfield CLI tests |
| [vcs-utils](specs/vcs-utils.md) | `crates/vcs-utils/` | stable | Shared VCS utilities — git HEAD detection, auto-push |
