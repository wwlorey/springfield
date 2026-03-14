# Springfield Specifications

| Spec | Code | Purpose |
|------|------|---------|
| [pensa](pensa.md) | `crates/pensa/` | Agent persistent memory — SQLite-backed issue/task tracker with `pn` CLI |
| [ralph](ralph.md) | `crates/ralph/` | Iterative Claude Code runner — invokes `cl` (claude-wrapper) |
| [shutdown](shutdown.md) | `crates/shutdown/` | Shared graceful shutdown — double-press Ctrl+C/Ctrl+D detection |
| [springfield](springfield.md) | `crates/springfield/` | CLI entry point — scaffolding, prompt delivery, loop orchestration |
| [claude-wrapper](claude-wrapper.md) | `crates/claude-wrapper/` | Agent wrapper — layered `.sgf/` context injection, `cl` binary |
| [forma](forma.md) | `crates/forma/` | Specification management — SQLite-backed spec tracker with `fm` CLI |
| [vcs-utils](vcs-utils.md) | `crates/vcs-utils/` | Shared VCS utilities — git HEAD detection, auto-push |
