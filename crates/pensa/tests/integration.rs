use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

fn pn_bin() -> String {
    env!("CARGO_BIN_EXE_pn").to_string()
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
fn remote_host_pn_daemon_refuses_auto_start() {
    let port = portpicker::pick_unused_port().expect("no free port");
    let output = Command::new(pn_bin())
        .env("PN_DAEMON", format!("http://localhost:{port}"))
        .env_remove("PN_DAEMON_HOST")
        .args(["list", "--json"])
        .output()
        .expect("run pn list with PN_DAEMON pointing to unreachable");
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

    let output = Command::new(pn_bin())
        .env("PN_DAEMON_HOST", "remote.example.com")
        .env_remove("PN_DAEMON")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with PN_DAEMON_HOST=remote.example.com");
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
    let output = Command::new(pn_bin())
        .env("PN_DAEMON_HOST", "localhost")
        .env_remove("PN_DAEMON")
        .args(["list", "--json"])
        .output()
        .expect("run pn list with PN_DAEMON_HOST=localhost");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("remote host configured"),
        "localhost should not be treated as remote, got: {stderr}"
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

// --- daemon.url tests ---

#[test]
fn daemon_url_file_refuses_auto_start() {
    let port = portpicker::pick_unused_port().expect("no free port");
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();
    std::fs::write(
        pensa_dir.join("daemon.url"),
        format!("http://localhost:{port}"),
    )
    .unwrap();

    let output = Command::new(pn_bin())
        .env_remove("PN_DAEMON")
        .env_remove("PN_DAEMON_HOST")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with daemon.url");
    assert!(
        !output.status.success(),
        "should fail when daemon.url points to unreachable daemon"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon unreachable") && stderr.contains("remote host configured"),
        "should treat daemon.url as remote, got: {stderr}"
    );
}

#[test]
fn empty_daemon_url_is_ignored() {
    let dir = TempDir::new().expect("create temp dir");
    let pensa_dir = dir.path().join(".pensa");
    std::fs::create_dir_all(&pensa_dir).unwrap();
    std::fs::write(pensa_dir.join("daemon.url"), "  \n").unwrap();

    let output = Command::new(pn_bin())
        .env_remove("PN_DAEMON")
        .env_remove("PN_DAEMON_HOST")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with empty daemon.url");
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

    let output = Command::new(pn_bin())
        .env_remove("PN_DAEMON")
        .env_remove("PN_DAEMON_HOST")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with stale daemon.project");

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

    let output = Command::new(pn_bin())
        .env_remove("PN_DAEMON")
        .env_remove("PN_DAEMON_HOST")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with matching daemon.project");

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

    let output = Command::new(pn_bin())
        .env_remove("PN_DAEMON")
        .env_remove("PN_DAEMON_HOST")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list without daemon.project");

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
