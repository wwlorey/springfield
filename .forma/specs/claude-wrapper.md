# claude-wrapper Specification

Agent wrapper — layered .sgf/ context injection, cl binary

| Field | Value |
|-------|-------|
| Src | `crates/claude-wrapper/` |
| Status | draft |

## Overview

`cl` provides:
- **Context resolution**: Resolve MEMENTO.md and BACKPRESSURE.md using layered `.sgf/` lookup (local → global)
- **Lookbook resolution**: Resolve LOOKBOOK.html from the repo root (no layered lookup)
- **Study injection**: Build `--append-system-prompt "study @<file>;..."` from resolved context files
- **Transparent forwarding**: Pass all arguments through to `claude-wrapper-secret` (opaque downstream binary)

## Architecture

```
cl [any args...]
  → resolve context files (./.sgf/ → ~/.sgf/ layering)
  → build --append-system-prompt with study args
  → exec claude-wrapper-secret [study args] [passthrough args]
```

```
crates/claude-wrapper/
├── src/
│   ├── main.rs       # CLI entry, arg forwarding, exec
│   └── resolve.rs    # Context file resolution with layered lookup
├── tests/
│   └── integration.rs
└── Cargo.toml
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `dirs` (6) | Home directory resolution (`dirs::home_dir()`) |

No async runtime. No clap (no flag parsing — all args are passthrough).

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Context file missing (both tiers) | Warning to stderr, skip the file |
| LOOKBOOK.html missing (repo root) | Note to stderr, skip the file |
| `claude-wrapper-secret` not in PATH | Error to stderr, exit 1 |
| `claude-wrapper-secret` exists but is not executable | Error to stderr, exit 1 (OS returns permission denied on exec) |
| Home directory unresolvable | Warning to stderr, skip global lookups |
| `exec()` syscall fails (any reason) | Error to stderr, exit 1 |

## Testing

### Unit Tests (`resolve.rs`)

- Local file exists → uses local path
- Local missing, global exists → uses global path
- Both missing → skipped, not in result
- All files missing → empty result
- Mixed: some local, some global → correct per-file resolution
- LOOKBOOK.html present at repo root → included last in results
- LOOKBOOK.html absent → skipped, not in result
- LOOKBOOK.html ordering: always after MEMENTO.md and BACKPRESSURE.md

### Integration Tests (`tests/integration.rs`)

- `cl` never invokes `claude` directly — mock `claude-wrapper-secret`, verify it receives the call
- `cl` never invokes `claude` binary — assert the binary name in the exec call is `claude-wrapper-secret`
- Context files appear in `--append-system-prompt` argument
- Local override takes precedence over global
- Missing context files are skipped (no error exit)
- Passthrough args are forwarded unchanged
- Multiple `--append-system-prompt` args coexist (one from `cl`, one from caller)
- LOOKBOOK.html appears in `--append-system-prompt` when present at repo root
- LOOKBOOK.html absent does not cause error or affect other context files

## Design Goals

1. **Single entry point**: All Claude Code invocations go through `cl` — interactive, AFK, standalone
2. **Layered config**: Project-local `.sgf/` overrides global `~/.sgf/` on a file-by-file basis
3. **Opaque downstream**: `cl` knows nothing about `claude-wrapper-secret` or `claude` — it just execs the next binary in the chain
4. **Testable**: Context resolution is a pure function; binary never calls `claude` directly

## Context File Resolution

`cl` resolves context files on every invocation.

### Layered Context Files

Uses a two-tier lookup: check the local project directory first, then fall back to the global home directory.

For each context file:

1. `./.sgf/<file>` — project-local override
2. `~/.sgf/<file>` — global default

The first existing path wins. If neither exists, the file is skipped with a warning to stderr.

| File | Local path | Global path | Required |
|------|-----------|-------------|----------|
| MEMENTO.md | `./.sgf/MEMENTO.md` | `~/.sgf/MEMENTO.md` | No (warn if missing) |
| BACKPRESSURE.md | `./.sgf/BACKPRESSURE.md` | `~/.sgf/BACKPRESSURE.md` | No (warn if missing) |

### Repo-Root Context Files

Checked at the repo root with no layered lookup and no global fallback.

| File | Path | Required | Purpose |
|------|------|----------|---------|
| LOOKBOOK.html | `./LOOKBOOK.html` | No (note if missing) | FE visual design and component source of truth |

If the file does not exist, a brief note is printed to stderr and the file is skipped. This uses "note" severity rather than "warning" because the file is naturally optional (many repos will not have one).

### Resolution Function

```rust
pub fn resolve_context_files(cwd: &Path, home: Option<&Path>) -> Vec<String>;
```

Pure function. Returns a list of absolute file paths. Layered context files are resolved first, followed by repo-root context files. LOOKBOOK.html is always last in the returned list.

## Argument Construction

`cl` builds a single `--append-system-prompt` argument from resolved context files, then prepends it to the passthrough args:

```
claude-wrapper-secret \
  --append-system-prompt 'study @<resolved-memento>;study @<resolved-backpressure>;study @<resolved-lookbook>' \
  [all original args passed to cl]
```

LOOKBOOK.html is always last in the study string.

If no context files resolve, the `--append-system-prompt` argument is omitted entirely.

If the caller (e.g. sgf) also passes `--append-system-prompt`, both flags are forwarded. The downstream binary receives multiple `--append-system-prompt` arguments — `cl` does not merge them.

## Downstream Binary

`cl` invokes `claude-wrapper-secret` via `exec` (replaces the process). The binary name is hardcoded. `cl` does not know or care what `claude-wrapper-secret` does — it is an opaque downstream binary that ultimately runs `claude`. This binary is external to the Springfield workspace and is not defined by any spec here. Users provide their own `claude-wrapper-secret` in `$PATH` (e.g., a shell script that sets API keys, selects models, or applies other per-user configuration before calling `claude`).

If `claude-wrapper-secret` is not found in `$PATH`, `cl` prints an error to stderr and exits with code 1.

## Installation

```
cargo install --path crates/claude-wrapper
```

Binary name `cl` is set via `[[bin]]` in `Cargo.toml`:

```toml
[[bin]]
name = "cl"
path = "src/main.rs"
```
