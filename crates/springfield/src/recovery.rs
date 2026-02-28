use std::io;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::loop_mgmt;

pub fn pre_launch_recovery(root: &Path) -> io::Result<()> {
    let pid_entries = loop_mgmt::list_pid_files(root);

    if pid_entries.is_empty() {
        return Ok(());
    }

    let any_alive = pid_entries
        .iter()
        .any(|(_, pid)| loop_mgmt::is_pid_alive(*pid));

    if any_alive {
        return Ok(());
    }

    // All PIDs are stale — remove PID files and recover
    for (loop_id, _) in &pid_entries {
        loop_mgmt::remove_pid_file(root, loop_id);
    }

    eprintln!("sgf: recovering from stale state...");

    let checkout = Command::new("git")
        .args(["checkout", "--", "."])
        .current_dir(root)
        .status();
    if let Ok(status) = checkout
        && !status.success()
    {
        eprintln!("sgf: warning: git checkout -- . exited with {status}");
    }

    let clean = Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(root)
        .status();
    if let Ok(status) = clean
        && !status.success()
    {
        eprintln!("sgf: warning: git clean -fd exited with {status}");
    }

    let doctor = Command::new("pn")
        .args(["doctor", "--fix"])
        .current_dir(root)
        .status();
    match doctor {
        Ok(status) if !status.success() => {
            eprintln!("sgf: warning: pn doctor --fix exited with {status}");
        }
        Err(e) => {
            eprintln!("sgf: warning: pn doctor --fix failed: {e}");
        }
        _ => {}
    }

    eprintln!("sgf: recovery complete");
    Ok(())
}

pub fn ensure_daemon(root: &Path) -> io::Result<()> {
    if daemon_is_reachable(root) {
        return Ok(());
    }

    eprintln!("sgf: starting pensa daemon...");

    Command::new("pn")
        .args(["daemon", "--project-dir", &root.to_string_lossy()])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to start pn daemon: {e}")))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
        if daemon_is_reachable(root) {
            eprintln!("sgf: pensa daemon ready");
            return Ok(());
        }
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "pensa daemon did not become ready within 5 seconds",
    ))
}

fn daemon_is_reachable(root: &Path) -> bool {
    Command::new("pn")
        .args(["daemon", "status"])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_git_repo(root: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        fs::write(root.join("README.md"), "# test\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
    }

    #[test]
    fn recovery_noop_when_no_pid_files() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/run")).unwrap();
        pre_launch_recovery(tmp.path()).unwrap();
    }

    #[test]
    fn recovery_skips_when_live_pid() {
        let tmp = TempDir::new().unwrap();
        setup_git_repo(tmp.path());

        let run_dir = tmp.path().join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        // Write current process PID — it's alive
        fs::write(
            run_dir.join("test-loop.pid"),
            std::process::id().to_string(),
        )
        .unwrap();

        // Create a dirty file to detect if recovery runs
        fs::write(tmp.path().join("dirty.txt"), "should survive").unwrap();

        pre_launch_recovery(tmp.path()).unwrap();

        // PID file should still exist (not cleaned up)
        assert!(run_dir.join("test-loop.pid").exists());
        // Dirty file should still exist (recovery did not run)
        assert!(tmp.path().join("dirty.txt").exists());
    }

    #[test]
    fn recovery_cleans_stale_state() {
        let tmp = TempDir::new().unwrap();
        setup_git_repo(tmp.path());

        // Create .sgf/run/ and PID file AFTER git setup so they're untracked
        let run_dir = tmp.path().join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(run_dir.join("stale-loop.pid"), "4000000").unwrap();

        // Create dirty state
        fs::write(tmp.path().join("README.md"), "modified").unwrap();
        fs::write(tmp.path().join("untracked.txt"), "should be cleaned").unwrap();

        pre_launch_recovery(tmp.path()).unwrap();

        // Stale PID file should be removed
        assert!(!run_dir.join("stale-loop.pid").exists());
        // Tracked file should be restored
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).unwrap(),
            "# test\n"
        );
        // Untracked file should be cleaned
        assert!(!tmp.path().join("untracked.txt").exists());
    }

    #[test]
    fn recovery_mixed_pids_skips_when_any_alive() {
        let tmp = TempDir::new().unwrap();
        setup_git_repo(tmp.path());

        let run_dir = tmp.path().join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        // One alive (own PID), one dead
        fs::write(
            run_dir.join("alive-loop.pid"),
            std::process::id().to_string(),
        )
        .unwrap();
        fs::write(run_dir.join("dead-loop.pid"), "4000000").unwrap();

        fs::write(tmp.path().join("dirty.txt"), "should survive").unwrap();

        pre_launch_recovery(tmp.path()).unwrap();

        // Both PID files should still exist
        assert!(run_dir.join("alive-loop.pid").exists());
        assert!(run_dir.join("dead-loop.pid").exists());
        // Dirty file should still exist
        assert!(tmp.path().join("dirty.txt").exists());
    }

    #[test]
    fn daemon_not_reachable_without_pn() {
        let tmp = TempDir::new().unwrap();
        // pn daemon status should fail (or pn not found) → not reachable
        assert!(!daemon_is_reachable(tmp.path()));
    }
}
