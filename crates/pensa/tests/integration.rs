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

// --- Phase 16: Advanced Scenarios ---

fn ids_in_array(arr: &serde_json::Value) -> Vec<String> {
    arr.as_array()
        .expect("expected array")
        .iter()
        .map(|v| extract_id(v))
        .collect()
}

#[test]
fn deps_and_ready() {
    let daemon = start_daemon();

    let task_a = run_ok_json(pn(&daemon).args(["create", "task alpha", "-t", "task", "-p", "p1"]));
    let id_a = extract_id(&task_a);

    let task_b = run_ok_json(pn(&daemon).args(["create", "task beta", "-t", "task", "-p", "p1"]));
    let id_b = extract_id(&task_b);

    // B depends on A
    run_ok_json(pn(&daemon).args(["dep", "add", &id_b, &id_a]));

    // Ready: A present, B absent (blocked)
    let ready = run_ok_json(pn(&daemon).args(["ready"]));
    let ready_ids = ids_in_array(&ready);
    assert!(ready_ids.contains(&id_a), "A should be ready");
    assert!(
        !ready_ids.contains(&id_b),
        "B should NOT be ready (blocked)"
    );

    // Blocked: B listed
    let blocked = run_ok_json(pn(&daemon).args(["blocked"]));
    let blocked_ids = ids_in_array(&blocked);
    assert!(blocked_ids.contains(&id_b), "B should be blocked");

    // Close A → B should become ready
    run_ok_json(pn(&daemon).args(["close", &id_a]));
    let ready2 = run_ok_json(pn(&daemon).args(["ready"]));
    let ready2_ids = ids_in_array(&ready2);
    assert!(
        ready2_ids.contains(&id_b),
        "B should be ready after A closed"
    );

    // Dep list for B
    let deps = run_ok_json(pn(&daemon).args(["dep", "list", &id_b]));
    let dep_ids = ids_in_array(&deps);
    assert!(dep_ids.contains(&id_a), "dep list for B should include A");
}

#[test]
fn cycle_detection() {
    let daemon = start_daemon();

    let a = run_ok_json(pn(&daemon).args(["create", "cycle-a", "-t", "task", "-p", "p2"]));
    let id_a = extract_id(&a);
    let b = run_ok_json(pn(&daemon).args(["create", "cycle-b", "-t", "task", "-p", "p2"]));
    let id_b = extract_id(&b);
    let c = run_ok_json(pn(&daemon).args(["create", "cycle-c", "-t", "task", "-p", "p2"]));
    let id_c = extract_id(&c);

    // A→B→C chain
    run_ok_json(pn(&daemon).args(["dep", "add", &id_b, &id_a]));
    run_ok_json(pn(&daemon).args(["dep", "add", &id_c, &id_b]));

    // Adding A depends on C would create cycle: C→B→A→C
    let stderr = run_fail(pn(&daemon).args(["dep", "add", &id_a, &id_c, "--json"]));
    assert!(
        stderr.contains("cycle_detected"),
        "expected cycle_detected error, got: {stderr}"
    );

    // No cycles should exist (the bad dep was rejected)
    let cycles = run_ok_json(pn(&daemon).args(["dep", "cycles"]));
    let arr = cycles.as_array().expect("cycles should be array");
    assert!(arr.is_empty(), "no cycles should exist, got: {cycles}");
}

#[test]
fn ready_excludes_bugs() {
    let daemon = start_daemon();

    // Create a bug
    let bug = run_ok_json(pn(&daemon).args(["create", "ui glitch", "-t", "bug", "-p", "p1"]));
    let bug_id = extract_id(&bug);

    // Create a task
    let task = run_ok_json(pn(&daemon).args(["create", "add button", "-t", "task", "-p", "p1"]));
    let task_id = extract_id(&task);

    let ready = run_ok_json(pn(&daemon).args(["ready"]));
    let ready_ids = ids_in_array(&ready);
    assert!(
        !ready_ids.contains(&bug_id),
        "bugs should NOT appear in ready"
    );
    assert!(ready_ids.contains(&task_id), "tasks should appear in ready");
}

#[test]
fn export_import_roundtrip() {
    let daemon = start_daemon();

    // Create issues with deps and comments
    let i1 = run_ok_json(pn(&daemon).args(["create", "setup db", "-t", "task", "-p", "p0"]));
    let id1 = extract_id(&i1);

    let i2 = run_ok_json(pn(&daemon).args(["create", "add auth", "-t", "task", "-p", "p1"]));
    let id2 = extract_id(&i2);

    let i3 = run_ok_json(pn(&daemon).args(["create", "font bug", "-t", "bug", "-p", "p2"]));
    let id3 = extract_id(&i3);

    // Dep: i2 depends on i1
    run_ok_json(pn(&daemon).args(["dep", "add", &id2, &id1]));

    // Comments
    run_ok_json(pn(&daemon).args(["comment", "add", &id1, "started schema work"]));
    run_ok_json(pn(&daemon).args(["comment", "add", &id3, "repro steps: open page"]));

    // Snapshot pre-export state
    let pre_list = run_ok_json(pn(&daemon).args(["list"]));
    let pre_count = pre_list.as_array().unwrap().len();

    // Export
    let export_result = run_ok_json(pn(&daemon).args(["export"]));
    assert_eq!(export_result["status"], "ok");
    assert_eq!(export_result["issues"].as_u64().unwrap(), pre_count as u64);
    assert_eq!(export_result["deps"].as_u64().unwrap(), 1);
    assert_eq!(export_result["comments"].as_u64().unwrap(), 2);

    // Import (rebuilds from JSONL)
    let import_result = run_ok_json(pn(&daemon).args(["import"]));
    assert_eq!(import_result["status"], "ok");
    assert_eq!(import_result["issues"].as_u64().unwrap(), pre_count as u64);

    // Verify data intact
    let post_list = run_ok_json(pn(&daemon).args(["list"]));
    assert_eq!(
        post_list.as_array().unwrap().len(),
        pre_count,
        "issue count should match after import"
    );

    // Verify specific issue
    let detail = run_ok_json(pn(&daemon).args(["show", &id1]));
    assert_eq!(detail["title"], "setup db");

    // Verify deps survived
    let deps = run_ok_json(pn(&daemon).args(["dep", "list", &id2]));
    let dep_ids = ids_in_array(&deps);
    assert!(dep_ids.contains(&id1), "dep should survive export/import");

    // Verify comments survived
    let comments = run_ok_json(pn(&daemon).args(["comment", "list", &id1]));
    let arr = comments.as_array().unwrap();
    assert_eq!(arr.len(), 1, "comment should survive export/import");
    assert_eq!(arr[0]["text"], "started schema work");
}

#[test]
fn doctor_detects_and_fixes() {
    let daemon = start_daemon();

    // Create tasks and claim them
    let t1 = run_ok_json(pn(&daemon).args(["create", "stale-1", "-t", "task", "-p", "p2"]));
    let id1 = extract_id(&t1);
    let t2 = run_ok_json(pn(&daemon).args(["create", "stale-2", "-t", "task", "-p", "p2"]));
    let id2 = extract_id(&t2);

    run_ok_json(pn(&daemon).args(["update", &id1, "--claim"]));
    run_ok_json(pn(&daemon).args(["update", &id2, "--claim"]));

    // Doctor should detect stale claims
    let report = run_ok_json(pn(&daemon).args(["doctor"]));
    let findings = report["findings"].as_array().expect("findings array");
    assert!(!findings.is_empty(), "doctor should report stale claims");

    // Doctor --fix should release them
    let fix_report = run_ok_json(pn(&daemon).args(["doctor", "--fix"]));
    assert!(
        fix_report["fixed"].as_bool().unwrap_or(false)
            || fix_report["fixes_applied"].as_u64().unwrap_or(0) > 0
            || fix_report["findings"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false),
        "doctor --fix should apply fixes"
    );

    // All previously claimed issues should now be open
    let list = run_ok_json(pn(&daemon).args(["list", "--status", "open"]));
    let open_ids = ids_in_array(&list);
    assert!(open_ids.contains(&id1), "stale-1 should be open after fix");
    assert!(open_ids.contains(&id2), "stale-2 should be open after fix");
}

#[test]
fn concurrent_claims() {
    let daemon = start_daemon();

    let task = run_ok_json(pn(&daemon).args(["create", "race target", "-t", "task", "-p", "p1"]));
    let id = extract_id(&task);

    // Spawn two claim attempts simultaneously
    let mut cmd1 = pn(&daemon);
    cmd1.env("PN_ACTOR", "racer-1")
        .args(["update", &id, "--claim", "--json"]);
    let mut cmd2 = pn(&daemon);
    cmd2.env("PN_ACTOR", "racer-2")
        .args(["update", &id, "--claim", "--json"]);

    let child1 = cmd1.spawn().expect("spawn racer-1");
    let child2 = cmd2.spawn().expect("spawn racer-2");

    let out1 = child1.wait_with_output().expect("wait racer-1");
    let out2 = child2.wait_with_output().expect("wait racer-2");

    let successes = [&out1, &out2].iter().filter(|o| o.status.success()).count();
    let failures = 2 - successes;

    assert_eq!(
        successes, 1,
        "exactly one claim should succeed (got {successes} successes)"
    );
    assert_eq!(
        failures, 1,
        "exactly one claim should fail (got {failures} failures)"
    );
}

#[test]
fn search_issues() {
    let daemon = start_daemon();

    run_ok_json(pn(&daemon).args([
        "create",
        "login crash on empty password",
        "-t",
        "bug",
        "-p",
        "p0",
    ]));
    run_ok_json(pn(&daemon).args(["create", "signup form validation", "-t", "task", "-p", "p2"]));
    run_ok_json(pn(&daemon).args([
        "create",
        "logout button placement",
        "-t",
        "task",
        "-p",
        "p3",
    ]));

    // Search for "login" — should match 1
    let results = run_ok_json(pn(&daemon).args(["search", "login"]));
    let arr = results.as_array().unwrap();
    assert_eq!(arr.len(), 1, "search 'login' should match 1 issue");
    assert_eq!(arr[0]["title"], "login crash on empty password");

    // Case-insensitive: "LOGIN" should also match
    let results2 = run_ok_json(pn(&daemon).args(["search", "LOGIN"]));
    let arr2 = results2.as_array().unwrap();
    assert_eq!(
        arr2.len(),
        1,
        "search 'LOGIN' should match case-insensitively"
    );

    // Search for "log" — should match login + logout
    let results3 = run_ok_json(pn(&daemon).args(["search", "log"]));
    let arr3 = results3.as_array().unwrap();
    assert_eq!(arr3.len(), 2, "search 'log' should match login and logout");
}

#[test]
fn comments_add_and_list() {
    let daemon = start_daemon();

    let issue = run_ok_json(pn(&daemon).args(["create", "comment test", "-t", "task", "-p", "p2"]));
    let id = extract_id(&issue);

    // Add comment
    let comment = run_ok_json(pn(&daemon).args(["comment", "add", &id, "first observation"]));
    assert_eq!(comment["text"], "first observation");
    assert!(comment["id"].is_string(), "comment should have an id");
    assert_eq!(comment["issue_id"], id);

    // Add another
    run_ok_json(pn(&daemon).args(["comment", "add", &id, "second thought"]));

    // List comments
    let comments = run_ok_json(pn(&daemon).args(["comment", "list", &id]));
    let arr = comments.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["text"], "first observation");
    assert_eq!(arr[1]["text"], "second thought");
}

#[test]
fn history_events() {
    let daemon = start_daemon();

    let issue = run_ok_json(pn(&daemon).args(["create", "history test", "-t", "task", "-p", "p2"]));
    let id = extract_id(&issue);

    // Mutate: update, then close
    run_ok_json(pn(&daemon).args(["update", &id, "--priority", "p0"]));
    run_ok_json(pn(&daemon).args(["close", &id, "--reason", "done"]));

    let events = run_ok_json(pn(&daemon).args(["history", &id]));
    let arr = events.as_array().expect("history should be array");
    assert!(
        arr.len() >= 3,
        "should have at least 3 events (create, update, close), got {}",
        arr.len()
    );

    // Newest first
    assert_eq!(
        arr[0]["event_type"], "closed",
        "first event should be 'closed' (newest first)"
    );
}

#[test]
fn json_output_shapes() {
    let daemon = start_daemon();

    // create → single issue object
    let created = run_ok_json(pn(&daemon).args(["create", "shape test", "-t", "task", "-p", "p2"]));
    assert!(created["id"].is_string());
    assert!(created["title"].is_string());
    assert!(created["status"].is_string());
    assert!(created["priority"].is_string());
    assert!(created["issue_type"].is_string());
    assert!(created["created_at"].is_string());
    assert!(created["updated_at"].is_string());
    let id = extract_id(&created);

    // show → issue detail (has deps + comments arrays)
    let detail = run_ok_json(pn(&daemon).args(["show", &id]));
    assert!(detail["deps"].is_array(), "show should have deps array");
    assert!(
        detail["comments"].is_array(),
        "show should have comments array"
    );

    // list → array
    let list = run_ok_json(pn(&daemon).args(["list"]));
    assert!(list.is_array(), "list should return array");

    // ready → array (may be empty)
    let ready = run_ok_json(pn(&daemon).args(["ready"]));
    assert!(ready.is_array(), "ready should return array");

    // blocked → array
    let blocked = run_ok_json(pn(&daemon).args(["blocked"]));
    assert!(blocked.is_array(), "blocked should return array");

    // search → array
    let search = run_ok_json(pn(&daemon).args(["search", "shape"]));
    assert!(search.is_array(), "search should return array");

    // count → {"count": N}
    let count = run_ok_json(pn(&daemon).args(["count"]));
    assert!(
        count["count"].is_number(),
        "count should have numeric 'count' field"
    );

    // count with grouping → {"total": N, "groups": [...]}
    let count_grouped = run_ok_json(pn(&daemon).args(["count", "--by-status"]));
    assert!(
        count_grouped["total"].is_number(),
        "grouped count should have 'total'"
    );
    assert!(
        count_grouped["groups"].is_array(),
        "grouped count should have 'groups' array"
    );

    // status → array of status entries (one per issue_type)
    let status = run_ok_json(pn(&daemon).args(["status"]));
    assert!(
        status.is_array(),
        "status should return an array of status entries"
    );

    // history → array of event objects
    let history = run_ok_json(pn(&daemon).args(["history", &id]));
    assert!(history.is_array(), "history should return array");

    // comment add → single comment object
    let comment = run_ok_json(pn(&daemon).args(["comment", "add", &id, "shape check"]));
    assert!(comment["id"].is_string());
    assert!(comment["text"].is_string());

    // comment list → array
    let comments = run_ok_json(pn(&daemon).args(["comment", "list", &id]));
    assert!(comments.is_array(), "comment list should return array");

    // dep cycles → array of arrays
    let cycles = run_ok_json(pn(&daemon).args(["dep", "cycles"]));
    assert!(cycles.is_array(), "dep cycles should return array");

    // update → single issue object
    let updated = run_ok_json(pn(&daemon).args(["update", &id, "--title", "renamed"]));
    assert_eq!(updated["title"], "renamed");

    // close → single issue object
    let closed = run_ok_json(pn(&daemon).args(["close", &id]));
    assert_eq!(closed["status"], "closed");

    // reopen → single issue object
    let reopened = run_ok_json(pn(&daemon).args(["reopen", &id]));
    assert_eq!(reopened["status"], "open");

    // release (after claim)
    run_ok_json(pn(&daemon).args(["update", &id, "--claim"]));
    let released = run_ok_json(pn(&daemon).args(["release", &id]));
    assert_eq!(released["status"], "open");

    // dep add → {"status": "added", "issue_id", "depends_on_id"}
    let i2 = run_ok_json(pn(&daemon).args(["create", "dep target", "-t", "task", "-p", "p2"]));
    let id2 = extract_id(&i2);
    let dep_added = run_ok_json(pn(&daemon).args(["dep", "add", &id, &id2]));
    assert_eq!(dep_added["status"], "added");
    assert!(dep_added["issue_id"].is_string());
    assert!(dep_added["depends_on_id"].is_string());

    // dep list → array of issue objects
    let dep_list = run_ok_json(pn(&daemon).args(["dep", "list", &id]));
    assert!(dep_list.is_array(), "dep list should return array");

    // dep tree → flat array of tree nodes with depth
    let dep_tree = run_ok_json(pn(&daemon).args(["dep", "tree", &id]));
    assert!(dep_tree.is_array(), "dep tree should return array");

    // dep remove → {"status": "removed", ...}
    let dep_removed = run_ok_json(pn(&daemon).args(["dep", "remove", &id, &id2]));
    assert_eq!(dep_removed["status"], "removed");

    // export → {"status": "ok", "issues": N, "deps": N, "comments": N}
    let exported = run_ok_json(pn(&daemon).args(["export"]));
    assert_eq!(exported["status"], "ok");
    assert!(exported["issues"].is_number());
    assert!(exported["deps"].is_number());
    assert!(exported["comments"].is_number());

    // import → same shape
    let imported = run_ok_json(pn(&daemon).args(["import"]));
    assert_eq!(imported["status"], "ok");

    // doctor → report object with findings
    let doctor = run_ok_json(pn(&daemon).args(["doctor"]));
    assert!(
        doctor["findings"].is_array(),
        "doctor should have findings array"
    );
}

#[test]
fn human_readable_output() {
    let daemon = start_daemon();

    run_ok_json(pn(&daemon).args(["create", "human output test", "-t", "task", "-p", "p1"]));

    // Run list WITHOUT --json
    let output = pn(&daemon)
        .args(["list"])
        .output()
        .expect("run pn list (human)");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain text, not be valid JSON
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "human output should NOT be valid JSON"
    );
    assert!(
        stdout.contains("human output test"),
        "human output should contain the issue title, got: {stdout}"
    );
}

#[test]
fn dep_tree_structure() {
    let daemon = start_daemon();

    let a = run_ok_json(pn(&daemon).args(["create", "tree-root", "-t", "task", "-p", "p1"]));
    let id_a = extract_id(&a);
    let b = run_ok_json(pn(&daemon).args(["create", "tree-child", "-t", "task", "-p", "p2"]));
    let id_b = extract_id(&b);
    let c = run_ok_json(pn(&daemon).args(["create", "tree-grandchild", "-t", "task", "-p", "p2"]));
    let id_c = extract_id(&c);

    // B depends on A, C depends on B
    run_ok_json(pn(&daemon).args(["dep", "add", &id_b, &id_a]));
    run_ok_json(pn(&daemon).args(["dep", "add", &id_c, &id_b]));

    // dep tree A --direction down: should show B and C
    let tree_down = run_ok_json(pn(&daemon).args(["dep", "tree", &id_a, "--direction", "down"]));
    let arr = tree_down.as_array().unwrap();
    let tree_ids: Vec<String> = arr.iter().map(|n| extract_id(n)).collect();
    assert!(
        tree_ids.contains(&id_b),
        "tree down from A should include B"
    );
    assert!(
        tree_ids.contains(&id_c),
        "tree down from A should include C"
    );

    // Check depth fields
    for node in arr {
        assert!(
            node["depth"].is_number(),
            "tree node should have depth field"
        );
    }

    // dep tree C --direction up: should show B and A
    let tree_up = run_ok_json(pn(&daemon).args(["dep", "tree", &id_c, "--direction", "up"]));
    let arr_up = tree_up.as_array().unwrap();
    let tree_up_ids: Vec<String> = arr_up.iter().map(|n| extract_id(n)).collect();
    assert!(
        tree_up_ids.contains(&id_b),
        "tree up from C should include B"
    );
    assert!(
        tree_up_ids.contains(&id_a),
        "tree up from C should include A"
    );
}
