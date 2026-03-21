# Specifications

| Spec | Src | Status | Purpose |
|------|-----|--------|--------|
| [claude-wrapper](specs/claude-wrapper.md) | `crates/claude-wrapper/` | proven | Agent wrapper — layered .sgf/ context injection, cl binary |
| [cursus](specs/cursus.md) | `crates/springfield/` | draft | Pipeline orchestration — declarative TOML-defined multi-iter workflows with context passing, sentinel-based transitions, and stall recovery |
| [forma](specs/forma.md) | `crates/forma/` | proven | Specification management — forma daemon and fm CLI |
| [pensa](specs/pensa.md) | `crates/pensa/` | draft | Agent persistent memory — SQLite-backed issue/task tracker with pn CLI |
| [session-resume](specs/session-resume.md) | `crates/springfield/` | draft | Session resume — persist Claude session IDs and loop config to enable resuming interrupted sessions via sgf resume |
| [shutdown](specs/shutdown.md) | `crates/shutdown/` | draft | Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection with confirmation prompts |
| [springfield](specs/springfield.md) | `crates/springfield/` | draft | CLI entry point — scaffolding, prompt delivery, iteration runner, loop orchestration, recovery, and daemon lifecycle |
| [test-harness](specs/test-harness.md) | `crates/springfield/tests/` | draft | Cross-crate integration test harness — concurrency control, process lifecycle guards, mock infrastructure, and environment isolation |
| [vcs-utils](specs/vcs-utils.md) | `crates/vcs-utils/` | proven | Shared VCS utilities — git HEAD detection, auto-push |
