use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn auto_push_pushes_to_remote() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a bare "remote" repo
    let remote_dir = tmp.path().join("remote.git");
    std::fs::create_dir(&remote_dir).unwrap();
    git(&remote_dir, &["init", "--bare"]);

    // Create a local repo that uses the bare repo as its remote
    let local_dir = tmp.path().join("local");
    std::fs::create_dir(&local_dir).unwrap();
    git(&local_dir, &["init"]);
    git(
        &local_dir,
        &["remote", "add", "origin", remote_dir.to_str().unwrap()],
    );

    // Make an initial commit and push to set up tracking
    std::fs::write(local_dir.join("file.txt"), "initial").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "initial"]);
    git(&local_dir, &["push", "-u", "origin", "master"]);

    // Record HEAD before
    let head_before = git(&local_dir, &["rev-parse", "HEAD"]);

    // Make a new commit
    std::fs::write(local_dir.join("file.txt"), "updated").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "update"]);

    let head_after = git(&local_dir, &["rev-parse", "HEAD"]);
    assert_ne!(head_before, head_after);

    // Run auto_push_if_changed from the local dir
    let messages = std::cell::RefCell::new(Vec::new());

    // We need to change directory for git_head() to work in the local repo
    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&local_dir).unwrap();

    vcs_utils::auto_push_if_changed(&head_before, |msg| {
        messages.borrow_mut().push(msg.to_string())
    });

    std::env::set_current_dir(&original_dir).unwrap();

    let msgs = messages.borrow();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0], "New commits detected, pushing...");

    // Verify the push landed on the remote
    let remote_head = git(&remote_dir, &["rev-parse", "HEAD"]);
    assert_eq!(remote_head, head_after);
}
