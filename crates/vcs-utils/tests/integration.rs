use std::process::Command;
use std::sync::Mutex;

static CWD_LOCK: Mutex<()> = Mutex::new(());

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
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let remote_dir = tmp.path().join("remote.git");
    std::fs::create_dir(&remote_dir).unwrap();
    git(&remote_dir, &["init", "--bare"]);

    let local_dir = tmp.path().join("local");
    std::fs::create_dir(&local_dir).unwrap();
    git(&local_dir, &["init"]);
    git(
        &local_dir,
        &["remote", "add", "origin", remote_dir.to_str().unwrap()],
    );

    std::fs::write(local_dir.join("file.txt"), "initial").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "initial"]);
    git(&local_dir, &["push", "-u", "origin", "master"]);

    let head_before = git(&local_dir, &["rev-parse", "HEAD"]);

    std::fs::write(local_dir.join("file.txt"), "updated").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "update"]);

    let head_after = git(&local_dir, &["rev-parse", "HEAD"]);
    assert_ne!(head_before, head_after);

    let messages = std::cell::RefCell::new(Vec::new());

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&local_dir).unwrap();

    vcs_utils::auto_push_if_changed(&head_before, |msg| {
        messages.borrow_mut().push(msg.to_string())
    });

    std::env::set_current_dir(&original_dir).unwrap();

    let msgs = messages.borrow();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0], "New commits detected, pushing...");

    let remote_head = git(&remote_dir, &["rev-parse", "HEAD"]);
    assert_eq!(remote_head, head_after);
}

#[test]
fn auto_push_skips_when_already_pushed() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let remote_dir = tmp.path().join("remote.git");
    std::fs::create_dir(&remote_dir).unwrap();
    git(&remote_dir, &["init", "--bare"]);

    let local_dir = tmp.path().join("local");
    std::fs::create_dir(&local_dir).unwrap();
    git(&local_dir, &["init"]);
    git(
        &local_dir,
        &["remote", "add", "origin", remote_dir.to_str().unwrap()],
    );

    std::fs::write(local_dir.join("file.txt"), "initial").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "initial"]);
    git(&local_dir, &["push", "-u", "origin", "master"]);

    let head_before = git(&local_dir, &["rev-parse", "HEAD"]);

    std::fs::write(local_dir.join("file.txt"), "updated").unwrap();
    git(&local_dir, &["add", "."]);
    git(&local_dir, &["commit", "-m", "update"]);
    git(&local_dir, &["push"]);

    let messages = std::cell::RefCell::new(Vec::new());

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&local_dir).unwrap();

    vcs_utils::auto_push_if_changed(&head_before, |msg| {
        messages.borrow_mut().push(msg.to_string())
    });

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(
        messages.borrow().is_empty(),
        "should not push when commits are already on remote"
    );
}

#[test]
fn unchanged_head_emits_nothing() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    std::fs::write(repo.join("f.txt"), "x").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);

    let head = git(&repo, &["rev-parse", "HEAD"]);

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&repo).unwrap();

    let messages = std::cell::RefCell::new(Vec::new());
    vcs_utils::auto_push_if_changed(&head, |msg| messages.borrow_mut().push(msg.to_string()));

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(messages.borrow().is_empty());
}

#[test]
fn git_head_returns_some_in_repo() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    std::fs::write(repo.join("f.txt"), "x").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&repo).unwrap();

    let head = vcs_utils::git_head();

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(head.is_some());
    let hash = head.unwrap();
    assert_eq!(hash.len(), 40);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn git_head_returns_none_outside_repo() {
    let _lock = CWD_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let head = vcs_utils::git_head();

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(head.is_none());
}
