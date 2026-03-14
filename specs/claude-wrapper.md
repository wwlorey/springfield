# claude-wrapper Specification

CLI wrapper that injects `.sgf/` context files into every Claude Code session. Binary name: `cl`. Replaces the bash `claude-wrapper` script from dotfiles with a Rust binary in the Springfield workspace.

## Overview

`cl` provides:
- **Context resolution**: Resolve MEMENTO.md, BACKPRESSURE.md, and specs/README.md using layered `.sgf/` lookup (local → global)
- **Study injection**: Build `--append-system-prompt "study @<file>;..."` from resolved context files
- **Transparent forwarding**: Pass all arguments through to `claude-wrapper-secret` (opaque downstream binary)

## Design Goals

1. **Single entry point**: All Claude Code invocations go through `cl` — interactive, AFK, standalone
2. **Layered config**: Project-local `.sgf/` overrides global `~/.sgf/` on a file-by-file basis
3. **Opaque downstream**: `cl` knows nothing about `claude-wrapper-secret` or `claude` — it just execs the next binary in the chain
4. **Testable**: Context resolution is a pure function; binary never calls `claude` directly

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

## Context File Resolution

`cl` resolves context files and the spec index on every invocation.

### Context file resolution

Uses a two-tier lookup: check the local project directory first, then fall back to the global home directory.

For each context file:

1. `./.sgf/<file>` — project-local override
2. `~/.sgf/<file>` — global default

The first existing path wins. If neither exists, the file is skipped with a warning to stderr.

| File | Local path | Global path | Required |
|------|-----------|-------------|----------|
| MEMENTO.md | `./.sgf/MEMENTO.md` | `~/.sgf/MEMENTO.md` | No (warn if missing) |
| BACKPRESSURE.md | `./.sgf/BACKPRESSURE.md` | `~/.sgf/BACKPRESSURE.md` | No (warn if missing) |

### Spec index resolution

`cl` calls `fm list --json` to get the current spec index from the forma daemon. The JSON output is formatted as a markdown table (matching the format of `.forma/README.md`) and included in the study instruction.

If `fm list --json` fails (forma daemon not running, `fm` not in PATH), `cl` falls back to reading `./.forma/README.md` if it exists. If neither source is available, the spec index is skipped with a warning to stderr.

### Resolution Function

```rust
pub struct ResolvedContext {
    pub files: Vec<String>,
    pub spec_index: Option<String>,
}

pub fn resolve_context(cwd: &Path, home: &Path) -> ResolvedContext;
```

Pure function for file resolution. Spec index resolution calls `fm list --json` as a subprocess.

## Argument Construction

`cl` builds a single `--append-system-prompt` argument from resolved context files and the spec index, then prepends it to the passthrough args:

```
claude-wrapper-secret \
  --append-system-prompt 'study @<resolved-memento>;study @<resolved-backpressure>;<spec-index-content>' \
  [all original args passed to cl]
```

The spec index content is inlined directly (not a file reference) since it comes from `fm list --json` output rendered as markdown.

If no context files resolve and no spec index is available, the `--append-system-prompt` argument is omitted entirely.

If the caller (e.g. ralph) also passes `--append-system-prompt`, both flags are forwarded. The downstream binary receives multiple `--append-system-prompt` arguments — `cl` does not merge them.

## Downstream Binary

`cl` invokes `claude-wrapper-secret` via `exec` (replaces the process). The binary name is hardcoded. `cl` does not know or care what `claude-wrapper-secret` does — it is an opaque downstream binary that ultimately runs `claude`. This binary is external to the Springfield workspace and is not defined by any spec here. Users provide their own `claude-wrapper-secret` in `$PATH` (e.g., a shell script that sets API keys, selects models, or applies other per-user configuration before calling `claude`).

If `claude-wrapper-secret` is not found in `$PATH`, `cl` prints an error to stderr and exits with code 1.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Context file missing (both tiers) | Warning to stderr, skip the file |
| `fm list --json` fails | Fall back to `./.forma/README.md` file; if also missing, skip with warning |
| `claude-wrapper-secret` not in PATH | Error to stderr, exit 1 |
| Home directory unresolvable | Warning to stderr, skip global lookups |

## Testing

### Unit Tests (`resolve.rs`)

- Local file exists → uses local path
- Local missing, global exists → uses global path
- Both missing → skipped, not in result
- All files missing → empty result
- Mixed: some local, some global → correct per-file resolution

### Integration Tests (`tests/integration.rs`)

- `cl` never invokes `claude` directly — mock `claude-wrapper-secret`, verify it receives the call
- `cl` never invokes `claude` binary — assert the binary name in the exec call is `claude-wrapper-secret`
- Context files appear in `--append-system-prompt` argument
- Local override takes precedence over global
- Missing context files are skipped (no error exit)
- Passthrough args are forwarded unchanged
- Multiple `--append-system-prompt` args coexist (one from `cl`, one from caller)
- Spec index from `fm list --json` appears in `--append-system-prompt`
- Spec index falls back to `.forma/README.md` when `fm` is unavailable
- Both spec index sources unavailable → skipped with warning, no error exit

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

## Related Specifications

- [ralph](ralph.md) — Iterative agent runner, invokes `cl` directly
- [forma](forma.md) — Specification management, `cl` injects `.forma/README.md` as context
- [springfield](springfield.md) — CLI entry point, invokes `cl` for interactive sessions
