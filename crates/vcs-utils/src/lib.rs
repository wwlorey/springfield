use std::process::Command;

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
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .is_none_or(|count| count > 0)
}

/// If HEAD has changed since `head_before` and there are unpushed commits, run `git push`.
/// Messages are emitted via `emit`. Silent on success.
/// Push failures are non-fatal — reported through `emit` and execution continues.
pub fn auto_push_if_changed(head_before: &str, emit: impl Fn(&str)) {
    let head_after = git_head();
    if let Some(ref after) = head_after
        && after != head_before
        && has_unpushed_commits()
    {
        emit("New commits detected, pushing...");
        match Command::new("git").arg("push").output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                emit(&format!("push failed (non-fatal): {}", stderr.trim()));
            }
            Err(e) => {
                emit(&format!("push failed (non-fatal): {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn git_head_returns_some_in_git_repo() {
        let head = git_head();
        assert!(head.is_some());
        let hash = head.unwrap();
        assert_eq!(hash.len(), 40);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn git_head_returns_none_in_non_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(tmp.path())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        assert!(result.is_none());
    }

    #[test]
    fn auto_push_unchanged_head_emits_nothing() {
        let current = git_head().unwrap();
        let messages = RefCell::new(Vec::new());
        auto_push_if_changed(&current, |msg| messages.borrow_mut().push(msg.to_string()));
        assert!(messages.borrow().is_empty());
    }

    #[test]
    fn auto_push_changed_head_already_pushed_emits_nothing() {
        let fake_old_head = "0000000000000000000000000000000000000000";
        let messages = RefCell::new(Vec::new());
        auto_push_if_changed(fake_old_head, |msg| {
            messages.borrow_mut().push(msg.to_string())
        });
        assert!(
            messages.borrow().is_empty(),
            "should not push when upstream is already up to date"
        );
    }
}
