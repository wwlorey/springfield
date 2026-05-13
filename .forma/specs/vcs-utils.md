# vcs-utils Specification

Shared VCS utilities — git HEAD detection, auto-push

| Field | Value |
|-------|-------|
| Src | `crates/vcs-utils/` |
| Status | draft |

## Overview

`vcs-utils` provides:
- **`git_head()`** — Returns the current HEAD commit hash
- **`has_unpushed_commits()`** — Checks if the local branch has commits not yet on its upstream (internal helper)
- **`auto_push_if_changed()`** — Pushes only if HEAD moved since a recorded snapshot AND there are unpushed commits, with caller-controlled output

## Architecture

```
crates/vcs-utils/
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
- `auto_push_if_changed()` with changed HEAD but commits already pushed emits nothing

### Integration Tests

- Create a temp git repo with a local bare remote, make a commit, call `auto_push_if_changed` with the old HEAD, verify the push lands on the remote
- Create a temp git repo with a local bare remote, make a commit, push it manually, call `auto_push_if_changed` with the old HEAD — verify no message is emitted (already pushed)

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

fn has_unpushed_commits() -> bool {
    Command::new("git")
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .is_none_or(|count| count > 0)
}

/// If HEAD has changed since `head_before` and there are unpushed commits, run `git push`.
/// Messages are emitted via `emit`. Silent on success.
/// Push failures are non-fatal — reported through `emit` and execution continues.
pub fn auto_push_if_changed(head_before: &str, emit: impl Fn(&str)) {
    let head_after = git_head();
    if let Some(ref after) = head_after
        && after \\!= head_before
        && has_unpushed_commits()
    {
        emit("New commits detected, pushing...");
        match Command::new("git").arg("push").output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                emit(&format\\!("push failed (non-fatal): {}", stderr.trim()));
            }
            Err(e) => {
                emit(&format\\!("push failed (non-fatal): {e}"));
            }
        }
    }
}
```

### Behavior

- **`git_head()`**: Runs `git rev-parse HEAD`. Returns `None` on any failure (not a repo, git not installed, etc.). No output.
- **`has_unpushed_commits()`**: Runs `git rev-list --count @{u}..HEAD`. Returns `true` if count > 0. If the upstream cannot be determined (detached HEAD, no tracking branch, no remote), defaults to `true` (safe fallback — attempts push). Internal, not public API.
- **`auto_push_if_changed()`**: Compares current HEAD against `head_before`. If different AND there are unpushed commits (via `has_unpushed_commits()`), emits "New commits detected, pushing..." via the `emit` callback, then runs `git push`. Silent on success. On failure, emits the error message through `emit` and returns (non-fatal). Uses `.output()` (not `.status()`) to capture stderr for error reporting. If commits have already been pushed (e.g., by the agent or a hook during the run), the push is skipped entirely.

### Caller Integration

**springfield** (`crates/springfield/src/orchestrate.rs` and `crates/springfield/src/cursus/runner.rs`):
```rust
vcs_utils::auto_push_if_changed(&head_before, |msg| style::print_warning(msg));
```
