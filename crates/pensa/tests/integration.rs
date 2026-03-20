use std::process::Command;
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

struct ProcessSemaphore {
    mutex: Mutex<usize>,
    condvar: Condvar,
    max: usize,
}

impl ProcessSemaphore {
    fn new(max: usize) -> Self {
        Self {
            mutex: Mutex::new(0),
            condvar: Condvar::new(),
            max,
        }
    }

    fn acquire(&self) -> SemaphoreGuard<'_> {
        let mut count = self.mutex.lock().unwrap();
        while *count >= self.max {
            count = self.condvar.wait(count).unwrap();
        }
        *count += 1;
        SemaphoreGuard { sem: self }
    }
}

struct SemaphoreGuard<'a> {
    sem: &'a ProcessSemaphore,
}

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.sem.mutex.lock().unwrap();
        *count -= 1;
        self.sem.condvar.notify_one();
    }
}

static PN_PERMITS: LazyLock<ProcessSemaphore> = LazyLock::new(|| {
    let max = std::env::var("SGF_TEST_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    ProcessSemaphore::new(max)
});

fn pn_bin() -> String {
    env!("CARGO_BIN_EXE_pn").to_string()
}

fn run_pn(cmd: &mut Command) -> std::process::Output {
    let _permit = PN_PERMITS.acquire();
    cmd.output().expect("failed to run pn")
}

#[test]
fn daemon_status_unreachable() {
    // Use a port that no daemon is listening on
    let port = portpicker::pick_unused_port().expect("no free port");
    let output = run_pn(
        Command::new(pn_bin())
            .env("PN_DAEMON", format!("http://localhost:{port}"))
            .env_remove("PN_DAEMON_HOST")
            .args(["daemon", "status"]),
    );
    assert!(
        !output.status.success(),
        "daemon status should fail when daemon is not running"
    );
}

#[test]
fn remote_host_pn_daemon_refuses_auto_start() {
    let port = portpicker::pick_unused_port().expect("no free port");
    let output = run_pn(
        Command::new(pn_bin())
            .env("PN_DAEMON", format!("http://localhost:{port}"))
            .env_remove("PN_DAEMON_HOST")
            .args(["list", "--json"]),
    );
    assert!(
        !output.status.success(),
        "should fail when PN_DAEMON points to unreachable daemon"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon unreachable") && stderr.contains("remote host configured"),
        "should mention remote host in error, got: {stderr}"
    );
}

#[test]
fn remote_host_pn_daemon_host_refuses_auto_start() {
    let port = portpicker::pick_unused_port().expect("no free port");
    let dir = TempDir::new().expect("create temp dir");
    let port_file = dir.path().join(".pensa");
    std::fs::create_dir_all(&port_file).unwrap();
    std::fs::write(port_file.join("daemon.port"), port.to_string()).unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env("PN_DAEMON_HOST", "remote.example.com")
            .env_remove("PN_DAEMON")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );
    assert!(
        !output.status.success(),
        "should fail when PN_DAEMON_HOST is remote and daemon unreachable"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon unreachable") && stderr.contains("remote host configured"),
        "should mention remote host in error, got: {stderr}"
    );
}

#[test]
fn localhost_daemon_host_allows_auto_start_attempt() {
    let output = run_pn(
        Command::new(pn_bin())
            .env("PN_DAEMON_HOST", "localhost")
            .env_remove("PN_DAEMON")
            .args(["list", "--json"]),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("remote host configured"),
        "localhost should not be treated as remote, got: {stderr}"
    );
}

#[test]
fn pn_where() {
    // `pn where` should work without a running daemon
    let output = run_pn(Command::new(pn_bin()).args(["where"]));
    assert!(output.status.success(), "pn where should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".pensa"),
        "pn where should print .pensa path, got: {stdout}"
    );
}

// --- daemon.url tests ---

#[test]
fn daemon_url_file_remote_refuses_auto_start() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();
    std::fs::write(
        pensa_dir.join("daemon.url"),
        "http://remote.example.com:9999",
    )
    .unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );
    assert!(
        !output.status.success(),
        "should fail when daemon.url points to unreachable remote daemon"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon unreachable") && stderr.contains("remote host configured"),
        "should treat remote daemon.url as remote, got: {stderr}"
    );
}

#[test]
fn daemon_url_file_localhost_allows_auto_start() {
    let port = portpicker::pick_unused_port().expect("no free port");
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();
    std::fs::write(
        pensa_dir.join("daemon.url"),
        format!("http://localhost:{port}"),
    )
    .unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("remote host configured"),
        "localhost daemon.url should not be treated as remote, got: {stderr}"
    );
}

#[test]
fn empty_daemon_url_is_ignored() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();
    std::fs::write(pensa_dir.join("daemon.url"), "  \n").unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("remote host configured"),
        "empty daemon.url should be ignored, got: {stderr}"
    );
}

// --- stale daemon detection tests ---

#[test]
fn stale_daemon_project_clears_port_file() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();

    // Write a port file pointing at a port nothing listens on
    let port = portpicker::pick_unused_port().expect("no free port");
    std::fs::write(pensa_dir.join("daemon.port"), port.to_string()).unwrap();
    // Write a project file pointing to a different (non-existent) directory
    std::fs::write(
        pensa_dir.join("daemon.project"),
        "/tmp/old-project-that-was-renamed",
    )
    .unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("stale daemon detected"),
        "should detect stale daemon, got: {stderr}"
    );
    // Port file should have been removed
    assert!(
        !pensa_dir.join("daemon.port").exists()
            || std::fs::read_to_string(pensa_dir.join("daemon.port"))
                .map(|s| s.trim() != port.to_string())
                .unwrap_or(true),
        "stale port file should be cleared"
    );
    // Old project file should have been removed
    assert!(
        !pensa_dir.join("daemon.project").exists()
            || std::fs::read_to_string(pensa_dir.join("daemon.project"))
                .map(|s| s.trim() != "/tmp/old-project-that-was-renamed")
                .unwrap_or(true),
        "stale project file should be cleared"
    );
}

#[test]
fn matching_daemon_project_is_not_stale() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();

    let canonical = dir.path().canonicalize().unwrap();
    let port = portpicker::pick_unused_port().expect("no free port");
    std::fs::write(pensa_dir.join("daemon.port"), port.to_string()).unwrap();
    std::fs::write(
        pensa_dir.join("daemon.project"),
        canonical.to_string_lossy().as_bytes(),
    )
    .unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("stale daemon detected"),
        "should not detect stale daemon when project matches, got: {stderr}"
    );
}

#[test]
fn missing_daemon_project_is_not_stale() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();

    // Only write port file, no project file (legacy daemon)
    let port = portpicker::pick_unused_port().expect("no free port");
    std::fs::write(pensa_dir.join("daemon.port"), port.to_string()).unwrap();

    let output = run_pn(
        Command::new(pn_bin())
            .env_remove("PN_DAEMON")
            .env_remove("PN_DAEMON_HOST")
            .current_dir(dir.path())
            .args(["list", "--json"]),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("stale daemon detected"),
        "missing daemon.project should not trigger stale detection, got: {stderr}"
    );
}

// --- Forma spec validation tests ---

struct DualDaemon {
    pensa_port: u16,
    forma_port: u16,
    client: reqwest::blocking::Client,
    _dir: TempDir,
}

impl DualDaemon {
    fn start() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        let pensa_port = portpicker::pick_unused_port().expect("no free port");
        let forma_port = portpicker::pick_unused_port().expect("no free port");
        let project_dir = dir.path().to_path_buf();

        let pd = project_dir.clone();
        let pensa_data = dir.path().join("pensa-data");
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(pensa::daemon::start_with_data_dir(
                pensa_port,
                pd,
                Some(pensa_data),
            ));
        });

        let pd = project_dir.clone();
        let forma_data = dir.path().join("forma-data");
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(forma::daemon::start_with_data_dir(
                forma_port,
                pd,
                Some(forma_data),
            ));
        });

        let client = reqwest::blocking::Client::new();
        let pensa_base = format!("http://localhost:{pensa_port}");
        let forma_base = format!("http://localhost:{forma_port}");

        for _ in 0..50 {
            let p_ok = client.get(format!("{pensa_base}/status")).send().is_ok();
            let f_ok = client.get(format!("{forma_base}/status")).send().is_ok();
            if p_ok && f_ok {
                // Write forma port so pensa can discover it via the file
                let forma_dir = dir.path().join(".forma");
                let _ = std::fs::create_dir_all(&forma_dir);
                std::fs::write(forma_dir.join("daemon.port"), forma_port.to_string()).unwrap();

                return DualDaemon {
                    pensa_port,
                    forma_port,
                    client,
                    _dir: dir,
                };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("daemons did not start in time");
    }

    fn pensa_url(&self, path: &str) -> String {
        format!("http://localhost:{}{}", self.pensa_port, path)
    }

    fn forma_url(&self, path: &str) -> String {
        format!("http://localhost:{}{}", self.forma_port, path)
    }
}

struct PensaOnlyDaemon {
    port: u16,
    client: reqwest::blocking::Client,
    _dir: TempDir,
}

impl PensaOnlyDaemon {
    fn start() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        let port = portpicker::pick_unused_port().expect("no free port");
        let project_dir = dir.path().to_path_buf();
        let data_dir = dir.path().join("pensa-data");

        // Write a .forma/daemon.port pointing to a port where nothing is
        // listening, so pensa reliably gets a connection error
        // (FormaUnavailable) instead of falling back to a hash-derived
        // port that might collide with another test's forma daemon.
        // Port 1 is privileged and guaranteed to refuse connections.
        let forma_dir = dir.path().join(".forma");
        let _ = std::fs::create_dir_all(&forma_dir);
        std::fs::write(forma_dir.join("daemon.port"), "1").unwrap();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(pensa::daemon::start_with_data_dir(
                port,
                project_dir,
                Some(data_dir),
            ));
        });

        let client = reqwest::blocking::Client::new();
        let base = format!("http://localhost:{port}");
        for _ in 0..50 {
            if client.get(format!("{base}/status")).send().is_ok() {
                return PensaOnlyDaemon {
                    port,
                    client,
                    _dir: dir,
                };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("pensa daemon did not start in time");
    }

    fn url(&self, path: &str) -> String {
        format!("http://localhost:{}{}", self.port, path)
    }
}

#[test]
fn forma_spec_validation_accepts_valid_spec() {
    let d = DualDaemon::start();

    // Create a spec in forma
    let resp = d
        .client
        .post(d.forma_url("/specs"))
        .json(&serde_json::json!({
            "stem": "auth",
            "src": "crates/auth/",
            "purpose": "Authentication"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "forma create should succeed");

    // Create an issue in pensa with --spec auth
    let resp = d
        .client
        .post(d.pensa_url("/issues"))
        .json(&serde_json::json!({
            "title": "Implement login",
            "issue_type": "task",
            "spec": "auth"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "pensa create with valid spec should succeed"
    );
    let body: Value = resp.json().unwrap();
    assert_eq!(body["spec"], "auth");
}

#[test]
fn forma_spec_validation_rejects_nonexistent_spec() {
    let d = DualDaemon::start();

    let resp = d
        .client
        .post(d.pensa_url("/issues"))
        .json(&serde_json::json!({
            "title": "Implement login",
            "issue_type": "task",
            "spec": "nonexistent"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "spec_not_found");
    assert!(body["error"].as_str().unwrap().contains("nonexistent"));
}

#[test]
fn forma_unavailable_rejects_spec_create() {
    let d = PensaOnlyDaemon::start();

    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Implement login",
            "issue_type": "task",
            "spec": "anything"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 503);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "forma_unavailable");
}

#[test]
fn no_spec_bypasses_forma_validation() {
    let d = PensaOnlyDaemon::start();

    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Fix a bug",
            "issue_type": "bug"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "pensa create without spec should succeed without forma"
    );
}

#[test]
fn forma_spec_validation_on_update() {
    let d = DualDaemon::start();

    // Create a spec in forma
    d.client
        .post(d.forma_url("/specs"))
        .json(&serde_json::json!({
            "stem": "ralph",
            "src": "crates/ralph/",
            "purpose": "Iterative runner"
        }))
        .send()
        .unwrap();

    // Create issue without spec
    let resp = d
        .client
        .post(d.pensa_url("/issues"))
        .json(&serde_json::json!({
            "title": "Some task",
            "issue_type": "task"
        }))
        .send()
        .unwrap();
    let issue: Value = resp.json().unwrap();
    let id = issue["id"].as_str().unwrap();

    // Update with valid spec
    let resp = d
        .client
        .patch(d.pensa_url(&format!("/issues/{id}")))
        .json(&serde_json::json!({"spec": "ralph"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "update with valid spec should succeed");

    // Update with invalid spec
    let resp = d
        .client
        .patch(d.pensa_url(&format!("/issues/{id}")))
        .json(&serde_json::json!({"spec": "bogus"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "spec_not_found");
}

// --- Src-ref / doc-ref HTTP tests ---

#[test]
fn src_ref_crud_via_http() {
    let d = PensaOnlyDaemon::start();

    // Create an issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Ref test issue",
            "issue_type": "task"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let issue: Value = resp.json().unwrap();
    let id = issue["id"].as_str().unwrap();

    // Add a src-ref
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/src-refs")))
        .json(&serde_json::json!({
            "path": "crates/pensa/src/db.rs",
            "reason": "schema migrations",
            "actor": "test"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let sr: Value = resp.json().unwrap();
    assert_eq!(sr["path"], "crates/pensa/src/db.rs");
    assert_eq!(sr["reason"], "schema migrations");
    assert_eq!(sr["issue_id"], id);
    let sr_id = sr["id"].as_str().unwrap().to_string();

    // Add another without reason
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/src-refs")))
        .json(&serde_json::json!({
            "path": "crates/pensa/src/types.rs",
            "actor": "test"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let sr2: Value = resp.json().unwrap();
    assert!(sr2["reason"].is_null() || sr2.get("reason").is_none());

    // List src-refs
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}/src-refs")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: Vec<Value> = resp.json().unwrap();
    assert_eq!(list.len(), 2);

    // Show issue includes src_refs
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["src_refs"].as_array().unwrap().len(), 2);

    // Remove a src-ref
    let resp = d
        .client
        .delete(d.url(&format!("/src-refs/{sr_id}")))
        .header("x-pensa-actor", "test")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Verify only one remains
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}/src-refs")))
        .send()
        .unwrap();
    let list: Vec<Value> = resp.json().unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn doc_ref_crud_via_http() {
    let d = PensaOnlyDaemon::start();

    // Create an issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Doc ref test",
            "issue_type": "task"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let issue: Value = resp.json().unwrap();
    let id = issue["id"].as_str().unwrap();

    // Add a doc-ref
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/doc-refs")))
        .json(&serde_json::json!({
            "path": "specs/pensa.md",
            "reason": "update schema section",
            "actor": "test"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let dr: Value = resp.json().unwrap();
    assert_eq!(dr["path"], "specs/pensa.md");
    assert_eq!(dr["reason"], "update schema section");
    let dr_id = dr["id"].as_str().unwrap().to_string();

    // List doc-refs
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}/doc-refs")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: Vec<Value> = resp.json().unwrap();
    assert_eq!(list.len(), 1);

    // Show issue includes doc_refs
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["doc_refs"].as_array().unwrap().len(), 1);

    // Remove
    let resp = d
        .client
        .delete(d.url(&format!("/doc-refs/{dr_id}")))
        .header("x-pensa-actor", "test")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 204);

    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}/doc-refs")))
        .send()
        .unwrap();
    let list: Vec<Value> = resp.json().unwrap();
    assert!(list.is_empty());
}

#[test]
fn ref_on_nonexistent_issue_returns_404() {
    let d = PensaOnlyDaemon::start();

    let resp = d
        .client
        .post(d.url("/issues/pn-00000000/src-refs"))
        .json(&serde_json::json!({
            "path": "foo.rs",
            "actor": "test"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 404);

    let resp = d
        .client
        .post(d.url("/issues/pn-00000000/doc-refs"))
        .json(&serde_json::json!({
            "path": "foo.md",
            "actor": "test"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[test]
fn crud_lifecycle() {
    let d = PensaOnlyDaemon::start();

    // 1. Create an issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Implement widget",
            "issue_type": "task",
            "priority": "p2",
            "description": "Build the widget feature",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "create should succeed");
    let issue: Value = resp.json().unwrap();
    let id = issue["id"].as_str().unwrap();
    assert_eq!(issue["title"], "Implement widget");
    assert_eq!(issue["issue_type"], "task");
    assert_eq!(issue["priority"], "p2");
    assert_eq!(issue["description"], "Build the widget feature");
    assert_eq!(issue["status"], "open");

    // 2. Show the issue and verify all fields
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["id"], id);
    assert_eq!(detail["title"], "Implement widget");
    assert_eq!(detail["status"], "open");
    assert_eq!(detail["issue_type"], "task");
    assert_eq!(detail["priority"], "p2");
    assert_eq!(detail["description"], "Build the widget feature");
    assert!(detail["assignee"].is_null());
    assert!(detail["closed_at"].is_null());
    assert!(detail["close_reason"].is_null());

    // 3. Update fields: title, priority, description
    let resp = d
        .client
        .patch(d.url(&format!("/issues/{id}")))
        .json(&serde_json::json!({
            "title": "Implement widget v2",
            "priority": "p1",
            "description": "Build the improved widget feature",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "update should succeed");
    let updated: Value = resp.json().unwrap();
    assert_eq!(updated["title"], "Implement widget v2");
    assert_eq!(updated["priority"], "p1");
    assert_eq!(updated["description"], "Build the improved widget feature");
    assert_eq!(
        updated["status"], "open",
        "status should remain open after field update"
    );

    // Verify via show
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["title"], "Implement widget v2");
    assert_eq!(detail["priority"], "p1");

    // 4. Close the issue
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/close")))
        .json(&serde_json::json!({
            "reason": "Feature complete",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "close should succeed");
    let closed: Value = resp.json().unwrap();
    assert_eq!(closed["status"], "closed");

    // Verify closed state via show
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["status"], "closed");
    assert!(detail["closed_at"].is_string(), "closed_at should be set");
    assert_eq!(detail["close_reason"], "Feature complete");

    // 5. Reopen the issue
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/reopen")))
        .json(&serde_json::json!({
            "reason": "Found edge case",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "reopen should succeed");
    let reopened: Value = resp.json().unwrap();
    assert_eq!(reopened["status"], "open");

    // Verify reopened state via show
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["status"], "open");
    assert!(
        detail["closed_at"].is_null(),
        "closed_at should be cleared after reopen"
    );

    // 6. Close again
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/close")))
        .json(&serde_json::json!({
            "reason": "Edge case fixed",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "second close should succeed");
    let closed2: Value = resp.json().unwrap();
    assert_eq!(closed2["status"], "closed");

    // Final verification
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["status"], "closed");
    assert_eq!(detail["close_reason"], "Edge case fixed");
    assert!(detail["closed_at"].is_string());
}

#[test]
fn remove_nonexistent_ref_returns_404() {
    let d = PensaOnlyDaemon::start();

    let resp = d
        .client
        .delete(d.url("/src-refs/pn-00000000"))
        .header("x-pensa-actor", "test")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 404);

    let resp = d
        .client
        .delete(d.url("/doc-refs/pn-00000000"))
        .header("x-pensa-actor", "test")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[test]
fn e2e_pensa_forma_tight_coupling() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_port = portpicker::pick_unused_port().expect("no free port");
    let forma_port = portpicker::pick_unused_port().expect("no free port");
    let project_dir = dir.path().to_path_buf();

    let pd = project_dir.clone();
    let pensa_data = dir.path().join("pensa-data");
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(pensa::daemon::start_with_data_dir(
            pensa_port,
            pd,
            Some(pensa_data),
        ));
    });

    let pd = project_dir.clone();
    let forma_data = dir.path().join("forma-data");
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(forma::daemon::start_with_data_dir(
            forma_port,
            pd,
            Some(forma_data),
        ));
    });

    let client = reqwest::blocking::Client::new();
    let pensa_base = format!("http://localhost:{pensa_port}");
    let forma_base = format!("http://localhost:{forma_port}");

    let mut ready = false;
    for _ in 0..50 {
        let p_ok = client.get(format!("{pensa_base}/status")).send().is_ok();
        let f_ok = client.get(format!("{forma_base}/status")).send().is_ok();
        if p_ok && f_ok {
            let forma_dir = dir.path().join(".forma");
            let _ = std::fs::create_dir_all(&forma_dir);
            std::fs::write(forma_dir.join("daemon.port"), forma_port.to_string()).unwrap();
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(ready, "daemons did not start in time");

    // Create a spec in forma
    let resp = client
        .post(format!("{forma_base}/specs"))
        .json(&serde_json::json!({
            "stem": "auth",
            "src": "crates/auth/",
            "purpose": "Authentication module"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "forma spec creation should succeed");

    // pn create --spec valid-stem succeeds
    let resp = client
        .post(format!("{pensa_base}/issues"))
        .json(&serde_json::json!({
            "title": "Implement auth flow",
            "issue_type": "task",
            "spec": "auth"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "create with valid spec should succeed");
    let body: Value = resp.json().unwrap();
    assert_eq!(body["spec"], "auth");

    // pn create --spec nonexistent fails with spec_not_found
    let resp = client
        .post(format!("{pensa_base}/issues"))
        .json(&serde_json::json!({
            "title": "Something",
            "issue_type": "task",
            "spec": "nonexistent"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        422,
        "create with nonexistent spec should fail"
    );
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "spec_not_found");

    // Simulate stopping forma: redirect port file to a dead port
    let dead_port = portpicker::pick_unused_port().expect("no free port");
    std::fs::write(dir.path().join(".forma/daemon.port"), dead_port.to_string()).unwrap();

    // pn create --spec any fails with forma_unavailable
    let resp = client
        .post(format!("{pensa_base}/issues"))
        .json(&serde_json::json!({
            "title": "Another task",
            "issue_type": "task",
            "spec": "anything"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        503,
        "create with spec should fail when forma is unreachable"
    );
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "forma_unavailable");

    // pn create without --spec succeeds regardless of forma state
    let resp = client
        .post(format!("{pensa_base}/issues"))
        .json(&serde_json::json!({
            "title": "No-spec task",
            "issue_type": "bug"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        201,
        "create without spec should succeed regardless of forma state"
    );
}

#[test]
fn ready_includes_unplanned_bugs() {
    let d = PensaOnlyDaemon::start();

    // Create a bug with no fix tasks
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Crash on empty input",
            "issue_type": "bug",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "bug creation should succeed");
    let bug: Value = resp.json().unwrap();
    let bug_id = bug["id"].as_str().unwrap().to_string();

    // Query ready — the unplanned bug (no fix tasks) should appear
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        ready_ids.contains(&bug_id.as_str()),
        "unplanned bug should appear in ready list, got: {ready_ids:?}"
    );
}

#[test]
fn ready_excludes_planned_bugs() {
    let d = PensaOnlyDaemon::start();

    // Create a bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Login crash on empty password",
            "issue_type": "bug",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "bug creation should succeed");
    let bug: Value = resp.json().unwrap();
    let bug_id = bug["id"].as_str().unwrap().to_string();

    // Bug should appear in ready (unplanned — no fix tasks)
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        ready_ids.contains(&bug_id.as_str()),
        "unplanned bug should appear in ready list before fix task is created, got: {ready_ids:?}"
    );

    // Create a fix task with --fixes pointing to the bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Add empty password validation",
            "issue_type": "task",
            "fixes": bug_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "fix task creation should succeed");

    // Bug should no longer appear in ready (now planned)
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        !ready_ids.contains(&bug_id.as_str()),
        "planned bug should NOT appear in ready list, got: {ready_ids:?}"
    );
}

#[test]
fn cycle_detection_rejects_circular_dependencies() {
    let d = PensaOnlyDaemon::start();

    // Create two issues
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Task A",
            "issue_type": "task",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let a: Value = resp.json().unwrap();
    let a_id = a["id"].as_str().unwrap().to_string();

    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Task B",
            "issue_type": "task",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let b: Value = resp.json().unwrap();
    let b_id = b["id"].as_str().unwrap().to_string();

    // A depends on B — should succeed
    let resp = d
        .client
        .post(d.url("/deps"))
        .json(&serde_json::json!({
            "issue_id": a_id,
            "depends_on_id": b_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "first dep should succeed");

    // B depends on A — would form a cycle, should be rejected
    let resp = d
        .client
        .post(d.url("/deps"))
        .json(&serde_json::json!({
            "issue_id": b_id,
            "depends_on_id": a_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        409,
        "circular dependency should be rejected with 409 Conflict"
    );
    let body: Value = resp.json().unwrap();
    assert_eq!(
        body["code"], "cycle_detected",
        "error code should be cycle_detected, got: {body}"
    );
}

#[test]
fn single_fix_auto_close() {
    let d = PensaOnlyDaemon::start();

    // Create a bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Widget crashes on nil input",
            "issue_type": "bug",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "bug creation should succeed");
    let bug: Value = resp.json().unwrap();
    let bug_id = bug["id"].as_str().unwrap().to_string();
    assert_eq!(bug["status"], "open");

    // Create a fix task linked to the bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Handle nil input in widget",
            "issue_type": "task",
            "fixes": bug_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "fix task creation should succeed");
    let fix_task: Value = resp.json().unwrap();
    let fix_id = fix_task["id"].as_str().unwrap().to_string();

    // Close the fix task
    let resp = d
        .client
        .post(d.url(&format!("/issues/{fix_id}/close")))
        .json(&serde_json::json!({
            "reason": "Nil guard added",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "closing fix task should succeed");

    // Verify the bug was auto-closed with reason "fixed"
    let resp = d
        .client
        .get(d.url(&format!("/issues/{bug_id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bug_detail: Value = resp.json().unwrap();
    assert_eq!(
        bug_detail["status"], "closed",
        "bug should be auto-closed after its only fix task is closed"
    );
    assert_eq!(
        bug_detail["close_reason"], "fixed",
        "auto-closed bug should have close_reason 'fixed'"
    );
    assert!(
        bug_detail["closed_at"].is_string(),
        "closed_at should be set on auto-closed bug"
    );
}

#[test]
fn claim_semantics_full_flow() {
    let d = PensaOnlyDaemon::start();

    // 1. Create an issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Implement auth",
            "issue_type": "task",
            "actor": "alice"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let issue: Value = resp.json().unwrap();
    let id = issue["id"].as_str().unwrap().to_string();
    assert_eq!(issue["status"], "open");
    assert!(issue["assignee"].is_null());

    // 2. Claim it — should succeed, status becomes in_progress, assignee set
    let resp = d
        .client
        .patch(d.url(&format!("/issues/{id}")))
        .json(&serde_json::json!({
            "claim": true,
            "actor": "alice"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "claim should succeed");
    let claimed: Value = resp.json().unwrap();
    assert_eq!(
        claimed["status"], "in_progress",
        "claimed issue should be in_progress"
    );
    assert_eq!(
        claimed["assignee"], "alice",
        "claimed issue should have assignee set"
    );

    // 3. Attempt second claim — should fail with already_claimed
    let resp = d
        .client
        .patch(d.url(&format!("/issues/{id}")))
        .json(&serde_json::json!({
            "claim": true,
            "actor": "bob"
        }))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        409,
        "second claim should fail with 409 Conflict"
    );
    let err: Value = resp.json().unwrap();
    assert_eq!(
        err["code"], "already_claimed",
        "error code should be already_claimed"
    );
    let err_msg = err["error"].as_str().unwrap();
    assert!(
        err_msg.contains("alice"),
        "error should report who holds the claim, got: {err_msg}"
    );

    // 4. Release — should succeed, back to open with no assignee
    let resp = d
        .client
        .post(d.url(&format!("/issues/{id}/release")))
        .header("x-pensa-actor", "alice")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "release should succeed");
    let released: Value = resp.json().unwrap();
    assert_eq!(
        released["status"], "open",
        "released issue should be back to open"
    );
    assert!(
        released["assignee"].is_null(),
        "released issue should have no assignee"
    );

    // Verify via show
    let resp = d
        .client
        .get(d.url(&format!("/issues/{id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["status"], "open");
    assert!(detail["assignee"].is_null());

    // 5. Claim again — should succeed
    let resp = d
        .client
        .patch(d.url(&format!("/issues/{id}")))
        .json(&serde_json::json!({
            "claim": true,
            "actor": "bob"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "re-claim after release should succeed");
    let reclaimed: Value = resp.json().unwrap();
    assert_eq!(
        reclaimed["status"], "in_progress",
        "re-claimed issue should be in_progress"
    );
    assert_eq!(
        reclaimed["assignee"], "bob",
        "re-claimed issue should have new assignee"
    );
}

#[test]
fn ready_type_filter_excludes_bugs() {
    let d = PensaOnlyDaemon::start();

    // Create a bug (no fix tasks)
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Segfault on startup",
            "issue_type": "bug",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201, "bug creation should succeed");
    let bug: Value = resp.json().unwrap();
    let bug_id = bug["id"].as_str().unwrap().to_string();

    // Unfiltered ready should include the bug
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        ready_ids.contains(&bug_id.as_str()),
        "bug should appear in unfiltered ready list, got: {ready_ids:?}"
    );

    // Filtered ready with type=task should exclude the bug
    let resp = d
        .client
        .get(d.url("/issues/ready?type=task"))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ready_tasks: Vec<Value> = resp.json().unwrap();
    let task_ids: Vec<&str> = ready_tasks
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(
        !task_ids.contains(&bug_id.as_str()),
        "bug should NOT appear in ready list filtered by type=task, got: {task_ids:?}"
    );
}

#[test]
fn multi_fix_all_or_nothing_auto_close() {
    let d = PensaOnlyDaemon::start();

    // Create a bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Data corruption on concurrent writes",
            "issue_type": "bug",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let bug: Value = resp.json().unwrap();
    let bug_id = bug["id"].as_str().unwrap().to_string();

    // Create two fix tasks linked to the bug
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Add write lock to data store",
            "issue_type": "task",
            "fixes": bug_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let fix1: Value = resp.json().unwrap();
    let fix1_id = fix1["id"].as_str().unwrap().to_string();

    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Add integrity check on read",
            "issue_type": "task",
            "fixes": bug_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let fix2: Value = resp.json().unwrap();
    let fix2_id = fix2["id"].as_str().unwrap().to_string();

    // Close the first fix task
    let resp = d
        .client
        .post(d.url(&format!("/issues/{fix1_id}/close")))
        .json(&serde_json::json!({
            "reason": "Write lock implemented",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify bug is still open (second fix task remains)
    let resp = d
        .client
        .get(d.url(&format!("/issues/{bug_id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bug_detail: Value = resp.json().unwrap();
    assert_eq!(
        bug_detail["status"], "open",
        "bug should remain open while a fix task is still unclosed"
    );
    assert!(
        bug_detail["closed_at"].is_null(),
        "closed_at should be null while bug is still open"
    );

    // Close the second fix task
    let resp = d
        .client
        .post(d.url(&format!("/issues/{fix2_id}/close")))
        .json(&serde_json::json!({
            "reason": "Integrity check added",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the bug was auto-closed with reason "fixed"
    let resp = d
        .client
        .get(d.url(&format!("/issues/{bug_id}")))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bug_detail: Value = resp.json().unwrap();
    assert_eq!(
        bug_detail["status"], "closed",
        "bug should be auto-closed after all fix tasks are closed"
    );
    assert_eq!(
        bug_detail["close_reason"], "fixed",
        "auto-closed bug should have close_reason 'fixed'"
    );
    assert!(
        bug_detail["closed_at"].is_string(),
        "closed_at should be set on auto-closed bug"
    );
}

#[test]
fn dependency_flow_with_ready_and_blocked() {
    let d = PensaOnlyDaemon::start();

    // Create parent issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Parent task",
            "issue_type": "task",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let parent: Value = resp.json().unwrap();
    let parent_id = parent["id"].as_str().unwrap().to_string();

    // Create child issue
    let resp = d
        .client
        .post(d.url("/issues"))
        .json(&serde_json::json!({
            "title": "Child task",
            "issue_type": "task",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);
    let child: Value = resp.json().unwrap();
    let child_id = child["id"].as_str().unwrap().to_string();

    // Add dependency: child depends on parent
    let resp = d
        .client
        .post(d.url("/deps"))
        .json(&serde_json::json!({
            "issue_id": child_id,
            "depends_on_id": parent_id,
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200, "adding dependency should succeed");

    // Verify child is excluded from ready list
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        ready_ids.contains(&parent_id.as_str()),
        "parent should be in ready list"
    );
    assert!(
        !ready_ids.contains(&child_id.as_str()),
        "child should NOT be in ready list while parent is open"
    );

    // Verify child appears in blocked list
    let resp = d.client.get(d.url("/issues/blocked")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let blocked: Vec<Value> = resp.json().unwrap();
    let blocked_ids: Vec<&str> = blocked.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        blocked_ids.contains(&child_id.as_str()),
        "child should appear in blocked list"
    );
    assert!(
        !blocked_ids.contains(&parent_id.as_str()),
        "parent should NOT appear in blocked list"
    );

    // Close the parent issue
    let resp = d
        .client
        .post(d.url(&format!("/issues/{parent_id}/close")))
        .json(&serde_json::json!({
            "reason": "Parent work completed",
            "actor": "tester"
        }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify child now appears in ready list
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        ready_ids.contains(&child_id.as_str()),
        "child should now be in ready list after parent is closed"
    );

    // Verify child no longer appears in blocked list
    let resp = d.client.get(d.url("/issues/blocked")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let blocked: Vec<Value> = resp.json().unwrap();
    let blocked_ids: Vec<&str> = blocked.iter().filter_map(|i| i["id"].as_str()).collect();
    assert!(
        !blocked_ids.contains(&child_id.as_str()),
        "child should no longer be in blocked list after parent is closed"
    );
}

#[test]
fn doctor_fix_releases_in_progress_claims() {
    let d = PensaOnlyDaemon::start();

    // Create three issues
    let mut ids = Vec::new();
    for title in ["Alpha task", "Beta task", "Gamma task"] {
        let resp = d
            .client
            .post(d.url("/issues"))
            .json(&serde_json::json!({
                "title": title,
                "issue_type": "task"
            }))
            .send()
            .unwrap();
        assert_eq!(resp.status(), 201);
        let issue: Value = resp.json().unwrap();
        ids.push(issue["id"].as_str().unwrap().to_string());
    }

    // Claim the first two (sets them to in_progress)
    for id in &ids[..2] {
        let resp = d
            .client
            .patch(d.url(&format!("/issues/{id}")))
            .json(&serde_json::json!({
                "claim": true,
                "actor": "agent-1"
            }))
            .send()
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    // Verify the two are in_progress and the third is open
    for (i, id) in ids.iter().enumerate() {
        let resp = d
            .client
            .get(d.url(&format!("/issues/{id}")))
            .send()
            .unwrap();
        let issue: Value = resp.json().unwrap();
        if i < 2 {
            assert_eq!(issue["status"], "in_progress");
            assert_eq!(issue["assignee"], "agent-1");
        } else {
            assert_eq!(issue["status"], "open");
            assert!(issue["assignee"].is_null());
        }
    }

    // Run doctor --fix
    let resp = d.client.post(d.url("/doctor?fix=true")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();

    // Verify findings detected the stale claims
    let findings = report["findings"]
        .as_array()
        .expect("findings should be array");
    assert_eq!(findings.len(), 2, "should find 2 stale in_progress claims");
    assert!(
        findings.iter().all(|f| f["check"] == "stale_claim"),
        "all findings should be stale_claim"
    );

    // Verify fixes were applied
    let fixes = report["fixes_applied"]
        .as_array()
        .expect("fixes_applied should be array");
    assert!(!fixes.is_empty(), "fixes should have been applied");

    // Verify all issues are now open with no assignee
    for id in &ids {
        let resp = d
            .client
            .get(d.url(&format!("/issues/{id}")))
            .send()
            .unwrap();
        let issue: Value = resp.json().unwrap();
        assert_eq!(
            issue["status"], "open",
            "issue {id} should be open after doctor --fix"
        );
        assert!(
            issue["assignee"].is_null(),
            "issue {id} should have no assignee after doctor --fix"
        );
    }

    // Verify all three now appear in ready list
    let resp = d.client.get(d.url("/issues/ready")).send().unwrap();
    assert_eq!(resp.status(), 200);
    let ready: Vec<Value> = resp.json().unwrap();
    let ready_ids: Vec<&str> = ready.iter().filter_map(|i| i["id"].as_str()).collect();
    for id in &ids {
        assert!(
            ready_ids.contains(&id.as_str()),
            "issue {id} should be in ready list after doctor --fix"
        );
    }
}
