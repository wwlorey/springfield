use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

struct TestDaemon {
    port: u16,
    client: reqwest::blocking::Client,
    _dir: TempDir,
}

impl TestDaemon {
    fn start() -> Self {
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

        let client = reqwest::blocking::Client::new();
        let base = format!("http://localhost:{port}");

        for _ in 0..50 {
            if client.get(format!("{base}/status")).send().is_ok() {
                return TestDaemon {
                    port,
                    client,
                    _dir: dir,
                };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("daemon did not start in time");
    }

    fn url(&self, path: &str) -> String {
        format!("http://localhost:{}{}", self.port, path)
    }

    fn get(&self, path: &str) -> reqwest::blocking::Response {
        self.client.get(self.url(path)).send().unwrap()
    }

    fn post(&self, path: &str, body: &Value) -> reqwest::blocking::Response {
        self.client.post(self.url(path)).json(body).send().unwrap()
    }

    fn patch(&self, path: &str, body: &Value) -> reqwest::blocking::Response {
        self.client.patch(self.url(path)).json(body).send().unwrap()
    }

    fn put(&self, path: &str, body: &Value) -> reqwest::blocking::Response {
        self.client.put(self.url(path)).json(body).send().unwrap()
    }

    fn delete(&self, path: &str) -> reqwest::blocking::Response {
        self.client.delete(self.url(path)).send().unwrap()
    }
}

#[test]
fn spec_crud_lifecycle() {
    let d = TestDaemon::start();

    // Create
    let resp = d.post(
        "/specs",
        &json!({
            "stem": "auth",
            "crate_path": "crates/auth/",
            "purpose": "Authentication"
        }),
    );
    assert_eq!(resp.status(), 201);
    let spec: Value = resp.json().unwrap();
    assert_eq!(spec["stem"], "auth");
    assert_eq!(spec["status"], "draft");

    // Show
    let resp = d.get("/specs/auth");
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["stem"], "auth");
    let sections = detail["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 5);

    // List
    let resp = d.get("/specs");
    assert_eq!(resp.status(), 200);
    let specs: Vec<Value> = resp.json().unwrap();
    assert_eq!(specs.len(), 1);

    // Update
    let resp = d.patch("/specs/auth", &json!({"status": "stable"}));
    assert_eq!(resp.status(), 200);
    let spec: Value = resp.json().unwrap();
    assert_eq!(spec["status"], "stable");

    // Delete (should fail without force since required sections exist)
    let resp = d.delete("/specs/auth?force=true");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "deleted");

    // Verify deleted
    let resp = d.get("/specs/auth");
    assert_eq!(resp.status(), 404);
}

#[test]
fn spec_not_found() {
    let d = TestDaemon::start();
    let resp = d.get("/specs/nonexistent");
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "not_found");
}

#[test]
fn spec_already_exists() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );
    let resp = d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );
    assert_eq!(resp.status(), 409);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "already_exists");
}

#[test]
fn required_sections_scaffolded() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "test-spec", "crate_path": "crates/test/", "purpose": "Testing"}),
    );

    let resp = d.get("/specs/test-spec/sections");
    assert_eq!(resp.status(), 200);
    let sections: Vec<Value> = resp.json().unwrap();
    assert_eq!(sections.len(), 5);

    let slugs: Vec<&str> = sections
        .iter()
        .map(|s| s["slug"].as_str().unwrap())
        .collect();
    assert_eq!(
        slugs,
        vec![
            "overview",
            "architecture",
            "dependencies",
            "error-handling",
            "testing"
        ]
    );
}

#[test]
fn section_add_set_get_remove() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );

    // Add custom section
    let resp = d.post(
        "/specs/auth/sections",
        &json!({"name": "API Design", "body": "REST endpoints"}),
    );
    assert_eq!(resp.status(), 201);
    let section: Value = resp.json().unwrap();
    assert_eq!(section["slug"], "api-design");
    assert_eq!(section["kind"], "custom");

    // Set section body
    let resp = d.put(
        "/specs/auth/sections/api-design",
        &json!({"body": "Updated REST endpoints"}),
    );
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_eq!(section["body"], "Updated REST endpoints");

    // Get section
    let resp = d.get("/specs/auth/sections/api-design");
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_eq!(section["body"], "Updated REST endpoints");

    // Remove custom section
    let resp = d.delete("/specs/auth/sections/api-design");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "removed");

    // Verify removed
    let resp = d.get("/specs/auth/sections/api-design");
    assert_eq!(resp.status(), 404);
}

#[test]
fn required_section_cannot_be_removed() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );

    let resp = d.delete("/specs/auth/sections/overview");
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "required_section");
}

#[test]
fn section_move() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );
    d.post(
        "/specs/auth/sections",
        &json!({"name": "Custom One", "body": ""}),
    );

    // Move custom-one after overview (position 0)
    let resp = d.post(
        "/specs/auth/sections/custom-one/move",
        &json!({"after": "overview"}),
    );
    assert_eq!(resp.status(), 200);

    // Verify order
    let resp = d.get("/specs/auth/sections");
    let sections: Vec<Value> = resp.json().unwrap();
    let slugs: Vec<&str> = sections
        .iter()
        .map(|s| s["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs[0], "overview");
    assert_eq!(slugs[1], "custom-one");
}

#[test]
fn ref_lifecycle() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "ralph", "crate_path": "crates/ralph/", "purpose": "Runner"}),
    );

    // Add ref
    let resp = d.post("/specs/auth/refs", &json!({"target": "ralph"}));
    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["from"], "auth");
    assert_eq!(body["to"], "ralph");

    // List refs
    let resp = d.get("/specs/auth/refs");
    assert_eq!(resp.status(), 200);
    let refs: Vec<Value> = resp.json().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0]["stem"], "ralph");

    // Remove ref
    let resp = d.delete("/specs/auth/refs/ralph");
    assert_eq!(resp.status(), 200);

    // Verify removed
    let resp = d.get("/specs/auth/refs");
    let refs: Vec<Value> = resp.json().unwrap();
    assert_eq!(refs.len(), 0);
}

#[test]
fn ref_cycle_detection() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "a", "crate_path": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "crate_path": "crates/b/", "purpose": "B"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "c", "crate_path": "crates/c/", "purpose": "C"}),
    );

    d.post("/specs/a/refs", &json!({"target": "b"}));
    d.post("/specs/b/refs", &json!({"target": "c"}));

    // This should create a cycle: c -> a -> b -> c
    let resp = d.post("/specs/c/refs", &json!({"target": "a"}));
    assert_eq!(resp.status(), 409);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "cycle_detected");
}

#[test]
fn ref_tree() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "a", "crate_path": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "crate_path": "crates/b/", "purpose": "B"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "c", "crate_path": "crates/c/", "purpose": "C"}),
    );

    d.post("/specs/a/refs", &json!({"target": "b"}));
    d.post("/specs/b/refs", &json!({"target": "c"}));

    let resp = d.get("/specs/a/refs/tree?direction=down");
    assert_eq!(resp.status(), 200);
    let nodes: Vec<Value> = resp.json().unwrap();
    assert!(nodes.len() >= 3);
    assert_eq!(nodes[0]["stem"], "a");
    assert_eq!(nodes[0]["depth"], 0);
}

#[test]
fn search_specs() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Authentication module"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "ralph", "crate_path": "crates/ralph/", "purpose": "Runner"}),
    );

    let resp = d.get("/specs/search?q=auth");
    assert_eq!(resp.status(), 200);
    let results: Vec<Value> = resp.json().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["stem"], "auth");
}

#[test]
fn count_specs() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "a", "crate_path": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "crate_path": "crates/b/", "purpose": "B"}),
    );

    let resp = d.get("/specs/count");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["total"], 2);

    let resp = d.get("/specs/count?by_status=true");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["total"], 2);
    assert!(body["groups"].is_array());
}

#[test]
fn project_status_endpoint() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );

    let resp = d.get("/status");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["draft"], 1);
}

#[test]
fn spec_history() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}),
    );
    d.patch("/specs/auth", &json!({"status": "stable"}));

    let resp = d.get("/specs/auth/history");
    assert_eq!(resp.status(), 200);
    let events: Vec<Value> = resp.json().unwrap();
    assert!(events.len() >= 2);
    assert_eq!(events[0]["event_type"], "updated");
    assert_eq!(events[1]["event_type"], "created");
}

#[test]
fn actor_from_header() {
    let d = TestDaemon::start();
    let resp = d
        .client
        .post(d.url("/specs"))
        .header("x-forma-actor", "test-agent")
        .json(&json!({"stem": "auth", "crate_path": "crates/auth/", "purpose": "Auth"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = d.get("/specs/auth/history");
    let events: Vec<Value> = resp.json().unwrap();
    assert_eq!(events[0]["actor"], "test-agent");
}

#[test]
fn error_response_shape() {
    let d = TestDaemon::start();
    let resp = d.get("/specs/nonexistent");
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().unwrap();
    assert!(body["error"].is_string());
    assert!(body["code"].is_string());
}

#[test]
fn daemon_writes_port_and_project_files() {
    let d = TestDaemon::start();
    let forma_dir = d._dir.path().join(".forma");

    let port_content = std::fs::read_to_string(forma_dir.join("daemon.port")).unwrap();
    assert_eq!(port_content, d.port.to_string());

    let project_content = std::fs::read_to_string(forma_dir.join("daemon.project")).unwrap();
    assert!(!project_content.is_empty());
}

#[test]
fn ref_cycles_endpoint() {
    let d = TestDaemon::start();

    // No cycles
    let resp = d.get("/refs/cycles");
    assert_eq!(resp.status(), 200);
    let cycles: Vec<Vec<String>> = resp.json().unwrap();
    assert!(cycles.is_empty());
}

#[test]
fn list_specs_filter_by_status() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "a", "crate_path": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "crate_path": "crates/b/", "purpose": "B"}),
    );
    d.patch("/specs/b", &json!({"status": "stable"}));

    let resp = d.get("/specs?status=draft");
    let specs: Vec<Value> = resp.json().unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0]["stem"], "a");

    let resp = d.get("/specs?status=stable");
    let specs: Vec<Value> = resp.json().unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0]["stem"], "b");
}

#[test]
fn delete_spec_without_force_and_empty_sections() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "empty", "crate_path": "crates/empty/", "purpose": "Empty"}),
    );

    // All required sections have empty bodies, so delete without force should work
    let resp = d.delete("/specs/empty");
    assert_eq!(resp.status(), 200);
}

#[test]
fn delete_spec_without_force_with_content_fails() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "full", "crate_path": "crates/full/", "purpose": "Full"}),
    );
    d.put(
        "/specs/full/sections/overview",
        &json!({"body": "Has content"}),
    );

    let resp = d.delete("/specs/full");
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["code"], "validation_failed");
}
