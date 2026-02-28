use std::process::{Child, Command};
use std::time::Duration;
use tempfile::TempDir;

fn pn_bin() -> String {
    env!("CARGO_BIN_EXE_pn").to_string()
}

struct DaemonGuard {
    child: Child,
    port: u16,
    _dir: TempDir,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon() -> DaemonGuard {
    let dir = TempDir::new().expect("create temp dir");
    let port = portpicker::pick_unused_port().expect("no free port");

    let child = Command::new(pn_bin())
        .args(["daemon", "--port", &port.to_string(), "--project-dir"])
        .arg(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait for daemon to be ready
    let base = format!("http://localhost:{port}");
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if reqwest::blocking::get(format!("{base}/status")).is_ok() {
            return DaemonGuard {
                child,
                port,
                _dir: dir,
            };
        }
    }
    panic!("daemon did not become ready within 5 seconds");
}

fn pn(guard: &DaemonGuard) -> Command {
    let mut cmd = Command::new(pn_bin());
    cmd.env("PN_DAEMON", format!("http://localhost:{}", guard.port));
    cmd.env("PN_ACTOR", "test-agent");
    cmd
}

fn run_json(cmd: &mut Command) -> (bool, serde_json::Value) {
    cmd.arg("--json");
    let output = cmd.output().expect("run pn command");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse JSON: {e}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

fn run_ok_json(cmd: &mut Command) -> serde_json::Value {
    let (ok, val) = run_json(cmd);
    assert!(ok, "expected success, got failure. value: {val}");
    val
}

fn run_fail(cmd: &mut Command) -> String {
    let output = cmd.output().expect("run pn command");
    assert!(
        !output.status.success(),
        "expected failure, got success. stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn extract_id(val: &serde_json::Value) -> String {
    val["id"]
        .as_str()
        .expect("issue should have id")
        .to_string()
}

#[test]
fn crud_lifecycle() {
    let daemon = start_daemon();

    // Create
    let issue = run_ok_json(pn(&daemon).args(["create", "login crash", "-t", "bug", "-p", "p0"]));
    assert_eq!(issue["status"], "open");
    assert_eq!(issue["priority"], "p0");
    assert_eq!(issue["issue_type"], "bug");
    assert_eq!(issue["title"], "login crash");
    let id = extract_id(&issue);

    // Show
    let detail = run_ok_json(pn(&daemon).args(["show", &id]));
    assert_eq!(detail["id"], id);
    assert_eq!(detail["title"], "login crash");

    // Update
    let updated = run_ok_json(pn(&daemon).args(["update", &id, "--priority", "p1"]));
    assert_eq!(updated["priority"], "p1");

    // Close
    let closed = run_ok_json(pn(&daemon).args(["close", &id, "--reason", "fixed"]));
    assert_eq!(closed["status"], "closed");

    // Reopen
    let reopened = run_ok_json(pn(&daemon).args(["reopen", &id]));
    assert_eq!(reopened["status"], "open");

    // Close again
    let closed2 = run_ok_json(pn(&daemon).args(["close", &id]));
    assert_eq!(closed2["status"], "closed");
}

#[test]
fn claim_semantics() {
    let daemon = start_daemon();

    // Create a task
    let issue =
        run_ok_json(pn(&daemon).args(["create", "implement auth", "-t", "task", "-p", "p1"]));
    let id = extract_id(&issue);

    // Claim with agent-1
    let claimed = run_ok_json(
        pn(&daemon)
            .env("PN_ACTOR", "agent-1")
            .args(["update", &id, "--claim"]),
    );
    assert_eq!(claimed["status"], "in_progress");
    assert_eq!(claimed["assignee"], "agent-1");

    // Second claim with agent-2 should fail
    let stderr = run_fail(
        pn(&daemon)
            .env("PN_ACTOR", "agent-2")
            .args(["update", &id, "--claim", "--json"]),
    );
    assert!(
        stderr.contains("already_claimed") || stderr.contains("agent-1"),
        "expected already_claimed error, got: {stderr}"
    );

    // Release
    let released = run_ok_json(pn(&daemon).args(["release", &id]));
    assert_eq!(released["status"], "open");
    // assignee should be cleared (null or empty)
    assert!(
        released["assignee"].is_null() || released["assignee"] == "",
        "assignee should be cleared after release, got: {}",
        released["assignee"]
    );

    // Now agent-2 can claim
    let claimed2 = run_ok_json(
        pn(&daemon)
            .env("PN_ACTOR", "agent-2")
            .args(["update", &id, "--claim"]),
    );
    assert_eq!(claimed2["status"], "in_progress");
    assert_eq!(claimed2["assignee"], "agent-2");
}

#[test]
fn fixes_auto_close() {
    let daemon = start_daemon();

    // Create a bug
    let bug = run_ok_json(pn(&daemon).args(["create", "crash on login", "-t", "bug", "-p", "p0"]));
    let bug_id = extract_id(&bug);

    // Create a task that fixes the bug
    let task = run_ok_json(pn(&daemon).args([
        "create",
        "fix login crash",
        "-t",
        "task",
        "-p",
        "p1",
        "--fixes",
        &bug_id,
    ]));
    let task_id = extract_id(&task);

    // Close the task
    let _closed_task = run_ok_json(pn(&daemon).args(["close", &task_id, "--force"]));

    // Bug should be auto-closed
    let bug_after = run_ok_json(pn(&daemon).args(["show", &bug_id]));
    assert_eq!(
        bug_after["status"], "closed",
        "bug should be auto-closed when fixing task is closed"
    );
}

#[test]
fn delete_with_force() {
    let daemon = start_daemon();

    // Create issue and add a comment
    let issue = run_ok_json(pn(&daemon).args(["create", "temp issue", "-t", "task", "-p", "p2"]));
    let id = extract_id(&issue);

    // Add a comment to make it require force
    let _comment = run_ok_json(pn(&daemon).args(["comment", "add", &id, "some observation"]));

    // Delete without force should fail
    let stderr = run_fail(pn(&daemon).args(["delete", &id, "--json"]));
    assert!(
        stderr.contains("delete_requires_force") || stderr.contains("force"),
        "expected delete_requires_force error, got: {stderr}"
    );

    // Delete with force should succeed
    let output = pn(&daemon)
        .args(["delete", &id, "--force", "--json"])
        .output()
        .expect("run delete --force");
    assert!(output.status.success(), "delete --force should succeed");

    // Show should fail with not found
    let stderr = run_fail(pn(&daemon).args(["show", &id, "--json"]));
    assert!(
        stderr.contains("not_found"),
        "expected not_found after delete, got: {stderr}"
    );
}

#[test]
fn daemon_status_reachable() {
    let daemon = start_daemon();

    let output = pn(&daemon)
        .args(["daemon", "status"])
        .output()
        .expect("run daemon status");
    assert!(
        output.status.success(),
        "daemon status should succeed when daemon is running"
    );
}

#[test]
fn daemon_status_unreachable() {
    // Use a port that no daemon is listening on
    let port = portpicker::pick_unused_port().expect("no free port");
    let output = Command::new(pn_bin())
        .env("PN_DAEMON", format!("http://localhost:{port}"))
        .args(["daemon", "status"])
        .output()
        .expect("run daemon status");
    assert!(
        !output.status.success(),
        "daemon status should fail when daemon is not running"
    );
}

#[test]
fn pn_where() {
    // `pn where` should work without a running daemon
    let output = Command::new(pn_bin())
        .args(["where"])
        .output()
        .expect("run pn where");
    assert!(output.status.success(), "pn where should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".pensa"),
        "pn where should print .pensa path, got: {stdout}"
    );
}
