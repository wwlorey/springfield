use std::process::Command;
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
        .env("PN_DAEMON_HOST", "host.docker.internal")
        .env_remove("PN_DAEMON")
        .current_dir(dir.path())
        .args(["list", "--json"])
        .output()
        .expect("run pn list with PN_DAEMON_HOST=host.docker.internal");
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
