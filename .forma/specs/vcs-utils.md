# vcs-utils Specification

Shared VCS utilities — git HEAD detection, auto-push

| Field | Value |
|-------|-------|
| Src | `crates/vcs-utils/` |
| Status | proven |

## Overview

`vcs-utils` provides:
- **`git_head()`** — Returns the current HEAD commit hash
- **`auto_push_if_changed()`** — Pushes if HEAD moved since a recorded snapshot, with caller-controlled output

## Architecture

```
vcs-utils/
├── src/
│   └── lib.rs      # git_head, auto_push_if_changed
├── tests/
│   └── integration.rs
└── Cargo.toml
```

## Dependencies

None. Uses only `std::process::Command`.

Dev dependencies:

| Crate | Purpose |
|-------|---------|
| `tempfile` (3) | Temporary directories for test isolation |

## Error Handling

- **`git_head()`** returns `None` on any failure (not a repo, git not installed, etc.). No output, no custom error types.
- **`auto_push_if_changed()`** reports push failures via the `emit` callback (non-fatal). Push failures do not propagate as errors — execution continues. No custom error types.

## Testing

### Unit Tests

- `git_head()` returns `Some` in a git repo with at least one commit
- `git_head()` returns `None` in a non-git directory
- `auto_push_if_changed()` with unchanged HEAD emits nothing
- `auto_push_if_changed()` with changed HEAD emits "New commits detected, pushing..."

### Integration Test

- Create a temp git repo with a local bare remote, make a commit, call `auto_push_if_changed` with the old HEAD, verify the push lands on the remote

## Public API

```rust
/// Returns the current HEAD commit hash, or `None` if not in a git repo
/// or git is unavailable.
pub fn git_head() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// If HEAD has changed since `head_before`, run `git push`.
/// Messages are emitted via `emit`. Silent on success.
/// Push failures are non-fatal — reported through `emit` and execution continues.
pub fn auto_push_if_changed(head_before: &str, emit: impl Fn(&str)) {
    let head_after = git_head();
    if let Some(ref after) = head_after
        && after \!= head_before
    {
        emit("New commits detected, pushing...");
        match Command::new("git").arg("push").output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                emit(&format\!("push failed (non-fatal): {}", stderr.trim()));
            }
            Err(e) => {
                emit(&format\!("push failed (non-fatal): {e}"));
            }
        }
    }
}
```

### Behavior

- **`git_head()`**: Runs `git rev-parse HEAD`. Returns `None` on any failure (not a repo, git not installed, etc.). No output.
- **`auto_push_if_changed()`**: Compares current HEAD against `head_before`. If different, emits "New commits detected, pushing..." via the `emit` callback, then runs `git push`. Silent on success. On failure, emits the error message through `emit` and returns (non-fatal). Uses `.output()` (not `.status()`) to capture stderr for error reporting.

### Caller Integration

**springfield** (`crates/springfield/src/orchestrate.rs` and `crates/springfield/src/cursus/runner.rs`):
```rust
vcs_utils::auto_push_if_changed(&head_before, |msg| style::print_warning(msg));
```
