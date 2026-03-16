use std::time::Duration;

use tempfile::TempDir;

use forma::client::Client;
use forma::types::FormaError;

struct TestEnv {
    _dir: TempDir,
}

impl TestEnv {
    fn start() -> (Self, Client) {
        let dir = TempDir::new().expect("create temp dir");
        let port = portpicker::pick_unused_port().expect("no free port");
        let project_dir = dir.path().to_path_buf();
        let data_dir = dir.path().join("data");

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(forma::daemon::start_with_data_dir(
                port,
                project_dir,
                Some(data_dir),
            ));
        });

        let base_url = format!("http://localhost:{port}");
        let http = reqwest::blocking::Client::new();
        for _ in 0..50 {
            if http.get(format!("{base_url}/status")).send().is_ok() {
                let client = Client::with_url(base_url);
                return (TestEnv { _dir: dir }, client);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("daemon did not start in time");
    }
}

#[test]
fn client_spec_crud_lifecycle() {
    let (_env, client) = TestEnv::start();

    let spec = client
        .create_spec("auth", Some("crates/auth/"), "Authentication", "tester")
        .unwrap();
    assert_eq!(spec["stem"], "auth");
    assert_eq!(spec["status"], "draft");

    let detail = client.get_spec("auth").unwrap();
    assert_eq!(detail["stem"], "auth");
    assert!(detail["sections"].as_array().unwrap().len() == 5);

    let list = client.list_specs(None).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    let updated = client
        .update_spec("auth", Some("stable"), None, None, "tester")
        .unwrap();
    assert_eq!(updated["status"], "stable");

    let filtered = client.list_specs(Some("draft")).unwrap();
    assert_eq!(filtered.as_array().unwrap().len(), 0);

    client.delete_spec("auth", true).unwrap();

    let err = client.get_spec("auth").unwrap_err();
    assert!(matches!(err, FormaError::NotFound(_)));
}

#[test]
fn client_section_operations() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("test", Some("crates/test/"), "Test", "tester")
        .unwrap();

    let section = client
        .add_section("test", "Custom Section", "body content", None, "tester")
        .unwrap();
    assert_eq!(section["slug"], "custom-section");
    assert_eq!(section["kind"], "custom");

    let updated = client
        .set_section("test", "custom-section", "new body", "tester")
        .unwrap();
    assert_eq!(updated["body"], "new body");

    let got = client.get_section("test", "custom-section").unwrap();
    assert_eq!(got["body"], "new body");

    let sections = client.list_sections("test").unwrap();
    assert_eq!(sections.as_array().unwrap().len(), 6);

    client
        .remove_section("test", "custom-section", "tester")
        .unwrap();

    let err = client.get_section("test", "custom-section").unwrap_err();
    assert!(matches!(err, FormaError::NotFound(_)));
}

#[test]
fn client_section_move() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("test", Some("crates/test/"), "Test", "tester")
        .unwrap();
    client
        .add_section("test", "Extra", "", None, "tester")
        .unwrap();

    client
        .move_section("test", "extra", "overview", "tester")
        .unwrap();

    let sections = client.list_sections("test").unwrap();
    let slugs: Vec<&str> = sections
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs[0], "overview");
    assert_eq!(slugs[1], "extra");
}

#[test]
fn client_required_section_cannot_be_removed() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("test", Some("crates/test/"), "Test", "tester")
        .unwrap();

    let err = client
        .remove_section("test", "overview", "tester")
        .unwrap_err();
    assert!(matches!(err, FormaError::RequiredSection(_)));
}

#[test]
fn client_ref_operations() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("a", Some("crates/a/"), "A", "tester")
        .unwrap();
    client
        .create_spec("b", Some("crates/b/"), "B", "tester")
        .unwrap();

    let result = client.add_ref("a", "b", "tester").unwrap();
    assert_eq!(result["from"], "a");
    assert_eq!(result["to"], "b");

    let refs = client.list_refs("a").unwrap();
    assert_eq!(refs.as_array().unwrap().len(), 1);
    assert_eq!(refs[0]["stem"], "b");

    let tree = client.ref_tree("a", "down").unwrap();
    let nodes = tree.as_array().unwrap();
    assert!(nodes.len() >= 2);
    assert_eq!(nodes[0]["stem"], "a");
    assert_eq!(nodes[0]["depth"], 0);

    let result = client.remove_ref("a", "b", "tester").unwrap();
    assert_eq!(result["status"], "removed");

    let refs = client.list_refs("a").unwrap();
    assert_eq!(refs.as_array().unwrap().len(), 0);
}

#[test]
fn client_ref_cycle_detection() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("a", Some("crates/a/"), "A", "tester")
        .unwrap();
    client
        .create_spec("b", Some("crates/b/"), "B", "tester")
        .unwrap();
    client
        .create_spec("c", Some("crates/c/"), "C", "tester")
        .unwrap();

    client.add_ref("a", "b", "tester").unwrap();
    client.add_ref("b", "c", "tester").unwrap();

    let err = client.add_ref("c", "a", "tester").unwrap_err();
    assert!(matches!(err, FormaError::CycleDetected));

    let cycles = client.ref_cycles().unwrap();
    assert!(cycles.as_array().unwrap().is_empty());
}

#[test]
fn client_search() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec(
            "auth",
            Some("crates/auth/"),
            "Authentication module",
            "tester",
        )
        .unwrap();
    client
        .create_spec("ralph", Some("crates/ralph/"), "Runner", "tester")
        .unwrap();

    let results = client.search_specs("auth").unwrap();
    assert_eq!(results.as_array().unwrap().len(), 1);
    assert_eq!(results[0]["stem"], "auth");
}

#[test]
fn client_count() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("a", Some("crates/a/"), "A", "tester")
        .unwrap();
    client
        .create_spec("b", Some("crates/b/"), "B", "tester")
        .unwrap();

    let count = client.count_specs(false).unwrap();
    assert_eq!(count["total"], 2);

    let by_status = client.count_specs(true).unwrap();
    assert_eq!(by_status["total"], 2);
    assert!(by_status["groups"].is_array());
}

#[test]
fn client_status() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("auth", Some("crates/auth/"), "Auth", "tester")
        .unwrap();

    let status = client.project_status().unwrap();
    assert_eq!(status["draft"], 1);
}

#[test]
fn client_history() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("auth", Some("crates/auth/"), "Auth", "tester")
        .unwrap();
    client
        .update_spec("auth", Some("stable"), None, None, "tester")
        .unwrap();

    let events = client.spec_history("auth").unwrap();
    let arr = events.as_array().unwrap();
    assert!(arr.len() >= 2);
    assert_eq!(arr[0]["event_type"], "updated");
    assert_eq!(arr[1]["event_type"], "created");
}

#[test]
fn client_import() {
    let (_env, client) = TestEnv::start();

    let result = client.import().unwrap();
    assert_eq!(result["specs"], 0);
    assert_eq!(result["sections"], 0);
    assert_eq!(result["refs"], 0);
}

#[test]
fn client_already_exists_error() {
    let (_env, client) = TestEnv::start();

    client
        .create_spec("auth", Some("crates/auth/"), "Auth", "tester")
        .unwrap();

    let err = client
        .create_spec("auth", Some("crates/auth/"), "Auth", "tester")
        .unwrap_err();
    assert!(matches!(err, FormaError::AlreadyExists(_)));
}
