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

    fn project_dir(&self) -> &std::path::Path {
        self._dir.path()
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

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.client.post(self.url("/shutdown")).send();
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
            "src": "crates/auth/",
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
    );
    let resp = d.post(
        "/specs",
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
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
        &json!({"stem": "test-spec", "src": "crates/test/", "purpose": "Testing"}),
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
    );
    d.post(
        "/specs/auth/sections",
        &json!({"name": "Custom One", "body": ""}),
    );

    // Move custom-one after overview (position 0)
    let resp = d.patch(
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "ralph", "src": "crates/ralph/", "purpose": "Runner"}),
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
        &json!({"stem": "a", "src": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "src": "crates/b/", "purpose": "B"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "c", "src": "crates/c/", "purpose": "C"}),
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
        &json!({"stem": "a", "src": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "src": "crates/b/", "purpose": "B"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "c", "src": "crates/c/", "purpose": "C"}),
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Authentication module"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "ralph", "src": "crates/ralph/", "purpose": "Runner"}),
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
        &json!({"stem": "a", "src": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "src": "crates/b/", "purpose": "B"}),
    );

    let resp = d.get("/specs/count");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["count"], 2);

    let resp = d.get("/specs/count?by_status=true");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["count"], 2);
    assert!(body["groups"].is_array());
}

#[test]
fn project_status_endpoint() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
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
        &json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}),
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
        .json(&json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = d.get("/specs/auth/history");
    let events: Vec<Value> = resp.json().unwrap();
    assert_eq!(events[0]["actor"], "test-agent");
}

#[test]
fn actor_from_header_on_delete() {
    let d = TestDaemon::start();
    d.client
        .post(d.url("/specs"))
        .header("x-forma-actor", "create-agent")
        .json(&json!({"stem": "auth", "src": "crates/auth/", "purpose": "Auth"}))
        .send()
        .unwrap();

    let resp = d
        .client
        .delete(d.url("/specs/auth?force=true"))
        .header("x-forma-actor", "delete-agent")
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "deleted");
    assert_eq!(body["stem"], "auth");
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
        &json!({"stem": "a", "src": "crates/a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "b", "src": "crates/b/", "purpose": "B"}),
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
        &json!({"stem": "empty", "src": "crates/empty/", "purpose": "Empty"}),
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
        &json!({"stem": "full", "src": "crates/full/", "purpose": "Full"}),
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

#[test]
fn status_transitions_draft_stable_proven() {
    let d = TestDaemon::start();
    let resp = d.post(
        "/specs",
        &json!({"stem": "lifecycle", "src": "crates/lifecycle/", "purpose": "Lifecycle test"}),
    );
    let spec: Value = resp.json().unwrap();
    assert_eq!(spec["status"], "draft");

    let resp = d.patch("/specs/lifecycle", &json!({"status": "stable"}));
    assert_eq!(resp.status(), 200);
    let spec: Value = resp.json().unwrap();
    assert_eq!(spec["status"], "stable");

    let resp = d.patch("/specs/lifecycle", &json!({"status": "proven"}));
    assert_eq!(resp.status(), 200);
    let spec: Value = resp.json().unwrap();
    assert_eq!(spec["status"], "proven");
}

#[test]
fn export_import_round_trip() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "alpha", "src": "crates/alpha/", "purpose": "Alpha spec"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "beta", "src": "crates/beta/", "purpose": "Beta spec"}),
    );
    d.put(
        "/specs/alpha/sections/overview",
        &json!({"body": "Alpha overview content"}),
    );
    d.post(
        "/specs/alpha/sections",
        &json!({"name": "Custom Part", "body": "Custom body here"}),
    );
    d.post("/specs/alpha/refs", &json!({"target": "beta"}));

    // Export
    let resp = d.post("/export", &json!({}));
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().unwrap();
    assert_eq!(result["status"], "ok");
    assert_eq!(result["specs"], 2);
    assert_eq!(result["sections"], 11); // 5 required * 2 + 1 custom
    assert_eq!(result["refs"], 1);

    // Verify JSONL files exist
    let forma_dir = d._dir.path().join(".forma");
    assert!(forma_dir.join("specs.jsonl").exists());
    assert!(forma_dir.join("sections.jsonl").exists());
    assert!(forma_dir.join("refs.jsonl").exists());

    // Import (rebuilds from JSONL)
    let resp = d.post("/import", &json!({}));
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().unwrap();
    assert_eq!(result["specs"], 2);
    assert_eq!(result["sections"], 11);
    assert_eq!(result["refs"], 1);

    // Verify data survived the round-trip
    let resp = d.get("/specs/alpha");
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["stem"], "alpha");
    assert_eq!(detail["purpose"], "Alpha spec");
    let sections = detail["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 6);

    let overview = sections.iter().find(|s| s["slug"] == "overview").unwrap();
    assert_eq!(overview["body"], "Alpha overview content");

    let custom = sections
        .iter()
        .find(|s| s["slug"] == "custom-part")
        .unwrap();
    assert_eq!(custom["body"], "Custom body here");

    let refs = detail["refs"].as_array().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0]["stem"], "beta");
}

#[test]
fn export_generates_markdown() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "myspec", "src": "crates/myspec/", "purpose": "My spec purpose"}),
    );
    d.put(
        "/specs/myspec/sections/overview",
        &json!({"body": "This is the overview."}),
    );

    d.post("/export", &json!({}));

    let forma_dir = d._dir.path().join(".forma");
    let md_path = forma_dir.join("specs/myspec.md");
    assert!(md_path.exists(), "markdown file should be generated");

    let md = std::fs::read_to_string(&md_path).unwrap();
    assert!(md.contains("# myspec Specification"));
    assert!(md.contains("My spec purpose"));
    assert!(md.contains("| Src | `crates/myspec/` |"));
    assert!(md.contains("| Status | draft |"));
    assert!(md.contains("## Overview"));
    assert!(md.contains("This is the overview."));
}

#[test]
fn export_generates_markdown_with_refs() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "src", "src": "crates/src/", "purpose": "Source"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "dep", "src": "crates/dep/", "purpose": "Dependency"}),
    );
    d.post("/specs/src/refs", &json!({"target": "dep"}));

    d.post("/export", &json!({}));

    let md = std::fs::read_to_string(d._dir.path().join(".forma/specs/src.md")).unwrap();
    assert!(md.contains("## Related Specifications"));
    assert!(md.contains("[dep](dep.md)"));
    assert!(md.contains("Dependency"));

    // Spec without refs should not have Related Specifications section
    let dep_md = std::fs::read_to_string(d._dir.path().join(".forma/specs/dep.md")).unwrap();
    assert!(!dep_md.contains("Related Specifications"));
}

#[test]
fn export_generates_readme() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "alpha", "src": "crates/alpha/", "purpose": "Alpha module"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "beta", "src": "crates/beta/", "purpose": "Beta module"}),
    );

    d.post("/export", &json!({}));

    let readme_path = d._dir.path().join(".forma/README.md");
    assert!(readme_path.exists());

    let readme = std::fs::read_to_string(&readme_path).unwrap();
    assert!(readme.contains("# Specifications"));
    assert!(readme.contains("| Spec | Code | Status | Purpose |"));
    assert!(readme.contains("[alpha](specs/alpha.md)"));
    assert!(readme.contains("`crates/alpha/`"));
    assert!(readme.contains("Alpha module"));
    assert!(readme.contains("[beta](specs/beta.md)"));
}

#[test]
fn export_removes_stale_markdown_for_deleted_specs() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "keeper", "src": "crates/keeper/", "purpose": "Stays"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "doomed", "src": "crates/doomed/", "purpose": "Will be deleted"}),
    );

    d.post("/export", &json!({}));

    let specs_dir = d._dir.path().join(".forma/specs");
    assert!(specs_dir.join("keeper.md").exists());
    assert!(specs_dir.join("doomed.md").exists());

    d.delete("/specs/doomed?force=true");

    d.post("/export", &json!({}));

    assert!(
        specs_dir.join("keeper.md").exists(),
        "active spec markdown should remain"
    );
    assert!(
        !specs_dir.join("doomed.md").exists(),
        "deleted spec markdown should be removed"
    );
}

#[test]
fn check_reports_empty_required_sections_as_warnings() {
    let d = TestDaemon::start();

    // Create a spec — its required sections will have empty bodies
    d.post(
        "/specs",
        &json!({"stem": "bare", "src": "crates/bare/", "purpose": "Bare spec"}),
    );
    // Create the src directory so src_paths_exist check passes
    std::fs::create_dir_all(d._dir.path().join("crates/bare")).unwrap();

    let resp = d.get("/check");
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();

    let warnings = report["warnings"].as_array().unwrap();
    let empty_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| w["check"] == "required_sections_nonempty")
        .collect();
    assert_eq!(
        empty_warnings.len(),
        5,
        "all 5 required sections should warn as empty"
    );
}

#[test]
fn check_reports_missing_src_path_as_error() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "ghost", "src": "crates/nonexistent/", "purpose": "Ghost"}),
    );

    let resp = d.get("/check");
    let report: Value = resp.json().unwrap();
    assert!(!report["ok"].as_bool().unwrap());

    let errors = report["errors"].as_array().unwrap();
    let path_errors: Vec<_> = errors
        .iter()
        .filter(|e| e["check"] == "src_paths_exist")
        .collect();
    assert!(!path_errors.is_empty());
}

#[test]
fn check_ok_field_and_shape() {
    let d = TestDaemon::start();

    let resp = d.get("/check");
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();
    assert!(report["ok"].is_boolean());
    assert!(report["errors"].is_array());
    assert!(report["warnings"].is_array());
}

#[test]
fn doctor_reports_sync_drift() {
    let d = TestDaemon::start();

    d.post(
        "/specs",
        &json!({"stem": "drifted", "src": "crates/drifted/", "purpose": "Drift test"}),
    );
    // No export has been done, so JSONL has 0 entries but SQLite has 1 spec

    let resp = d.post("/doctor", &json!({}));
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();

    let findings = report["findings"].as_array().unwrap();
    let drift: Vec<_> = findings
        .iter()
        .filter(|f| f["check"] == "sync_drift")
        .collect();
    assert!(
        !drift.is_empty(),
        "should detect JSONL/SQLite count mismatch"
    );
}

#[test]
fn doctor_shape() {
    let d = TestDaemon::start();

    let resp = d.post("/doctor", &json!({}));
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();
    assert!(report["findings"].is_array());
    assert!(report["fixes_applied"].is_array());
}

#[test]
fn history_tracks_section_events() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "hist", "src": "crates/hist/", "purpose": "History"}),
    );
    d.put("/specs/hist/sections/overview", &json!({"body": "content"}));
    d.post(
        "/specs/hist/sections",
        &json!({"name": "Extra", "body": "extra body"}),
    );

    let resp = d.get("/specs/hist/history");
    let events: Vec<Value> = resp.json().unwrap();
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"section_added"));
    assert!(types.contains(&"section_updated"));
    assert!(types.contains(&"created"));
}

#[test]
fn slug_generation() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "slugtest", "src": "crates/slugtest/", "purpose": "Slug test"}),
    );

    // "Error Handling" -> "error-handling" (already a required section)
    let resp = d.get("/specs/slugtest/sections/error-handling");
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_eq!(section["slug"], "error-handling");
    assert_eq!(section["name"], "Error Handling");

    // Custom section with mixed case
    let resp = d.post(
        "/specs/slugtest/sections",
        &json!({"name": "NDJSON Stream Formatting", "body": ""}),
    );
    assert_eq!(resp.status(), 201);
    let section: Value = resp.json().unwrap();
    assert_eq!(section["slug"], "ndjson-stream-formatting");
}

#[test]
fn section_add_with_after() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "ordered", "src": "crates/ordered/", "purpose": "Ordering test"}),
    );

    // Add a custom section after "overview"
    let resp = d.post(
        "/specs/ordered/sections",
        &json!({"name": "Inserted", "body": "inserted here", "after": "overview"}),
    );
    assert_eq!(resp.status(), 201);

    let resp = d.get("/specs/ordered/sections");
    let sections: Vec<Value> = resp.json().unwrap();
    let slugs: Vec<&str> = sections
        .iter()
        .map(|s| s["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs[0], "overview");
    assert_eq!(slugs[1], "inserted");
}

#[test]
fn json_output_shapes_spec_object() {
    let d = TestDaemon::start();
    let resp = d.post(
        "/specs",
        &json!({"stem": "shapes", "src": "crates/shapes/", "purpose": "Shape test"}),
    );
    let spec: Value = resp.json().unwrap();
    assert!(spec["stem"].is_string());
    assert!(spec["src"].is_string());
    assert!(spec["purpose"].is_string());
    assert!(spec["status"].is_string());
    assert!(spec["created_at"].is_string());
    assert!(spec["updated_at"].is_string());
}

#[test]
fn json_output_shapes_spec_detail() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "detail", "src": "crates/detail/", "purpose": "Detail test"}),
    );

    let resp = d.get("/specs/detail");
    let detail: Value = resp.json().unwrap();
    assert!(detail["stem"].is_string());
    assert!(detail["sections"].is_array());
    assert!(detail["refs"].is_array());

    let section = &detail["sections"][0];
    assert!(section["name"].is_string());
    assert!(section["slug"].is_string());
    assert!(section["kind"].is_string());
    assert!(section["body"].is_string());
    assert!(section["position"].is_number());
}

#[test]
fn json_output_shapes_event_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "evshape", "src": "crates/evshape/", "purpose": "Event shapes"}),
    );

    let resp = d.get("/specs/evshape/history");
    let events: Vec<Value> = resp.json().unwrap();
    assert!(!events.is_empty());
    let ev = &events[0];
    assert!(ev["id"].is_number());
    assert!(ev["spec_stem"].is_string());
    assert!(ev["event_type"].is_string());
    assert!(ev["created_at"].is_string());
}

#[test]
fn ref_tree_up_direction() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "top", "src": "crates/top/", "purpose": "Top"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "bottom", "src": "crates/bottom/", "purpose": "Bottom"}),
    );
    d.post("/specs/top/refs", &json!({"target": "bottom"}));

    let resp = d.get("/specs/bottom/refs/tree?direction=up");
    assert_eq!(resp.status(), 200);
    let nodes: Vec<Value> = resp.json().unwrap();
    assert!(nodes.len() >= 2);
    assert_eq!(nodes[0]["stem"], "bottom");
    assert_eq!(nodes[0]["depth"], 0);
    let stems: Vec<&str> = nodes.iter().map(|n| n["stem"].as_str().unwrap()).collect();
    assert!(stems.contains(&"top"));
}

#[test]
fn search_matches_section_body() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "searchable", "src": "crates/searchable/", "purpose": "Generic"}),
    );
    d.put(
        "/specs/searchable/sections/overview",
        &json!({"body": "Contains unique-search-term-xyz here"}),
    );

    let resp = d.get("/specs/search?q=unique-search-term-xyz");
    assert_eq!(resp.status(), 200);
    let results: Vec<Value> = resp.json().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["stem"], "searchable");
}

fn assert_spec_object(v: &Value) {
    assert!(v["stem"].is_string(), "spec.stem should be string");
    assert!(
        v["src"].is_string() || v["src"].is_null(),
        "spec.src should be string or null"
    );
    assert!(v["purpose"].is_string(), "spec.purpose should be string");
    assert!(v["status"].is_string(), "spec.status should be string");
    assert!(
        v["created_at"].is_string(),
        "spec.created_at should be string"
    );
    assert!(
        v["updated_at"].is_string(),
        "spec.updated_at should be string"
    );
}

fn assert_section_object(v: &Value) {
    assert!(v["name"].is_string(), "section.name should be string");
    assert!(v["slug"].is_string(), "section.slug should be string");
    assert!(v["kind"].is_string(), "section.kind should be string");
    assert!(v["body"].is_string(), "section.body should be string");
    assert!(
        v["position"].is_number(),
        "section.position should be number"
    );
}

fn assert_event_object(v: &Value) {
    assert!(v["id"].is_number(), "event.id should be number");
    assert!(
        v["spec_stem"].is_string(),
        "event.spec_stem should be string"
    );
    assert!(
        v["event_type"].is_string(),
        "event.event_type should be string"
    );
    assert!(
        v["actor"].is_string() || v["actor"].is_null(),
        "event.actor should be string or null"
    );
    assert!(
        v["detail"].is_string() || v["detail"].is_null(),
        "event.detail should be string or null"
    );
    assert!(
        v["created_at"].is_string(),
        "event.created_at should be string"
    );
}

#[test]
fn json_shape_create_returns_spec_object() {
    let d = TestDaemon::start();
    let resp = d.post(
        "/specs",
        &json!({"stem": "jsc", "src": "crates/jsc/", "purpose": "Shape create"}),
    );
    assert_eq!(resp.status(), 201);
    let spec: Value = resp.json().unwrap();
    assert_spec_object(&spec);
    assert_eq!(spec["stem"], "jsc");
    assert_eq!(spec["status"], "draft");
}

#[test]
fn json_shape_update_returns_spec_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsu", "src": "crates/jsu/", "purpose": "Shape update"}),
    );
    let resp = d.patch("/specs/jsu", &json!({"status": "stable"}));
    assert_eq!(resp.status(), 200);
    let spec: Value = resp.json().unwrap();
    assert_spec_object(&spec);
    assert_eq!(spec["status"], "stable");
}

#[test]
fn json_shape_show_returns_spec_detail() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jss", "src": "crates/jss/", "purpose": "Shape show"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jss-ref", "src": "crates/jss-ref/", "purpose": "Ref target"}),
    );
    d.post("/specs/jss/refs", &json!({"target": "jss-ref"}));

    let resp = d.get("/specs/jss");
    assert_eq!(resp.status(), 200);
    let detail: Value = resp.json().unwrap();

    assert_spec_object(&detail);
    assert!(detail["sections"].is_array());
    assert!(detail["refs"].is_array());

    for section in detail["sections"].as_array().unwrap() {
        assert_section_object(section);
    }
    for ref_spec in detail["refs"].as_array().unwrap() {
        assert_spec_object(ref_spec);
    }
}

#[test]
fn json_shape_delete_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsd", "src": "crates/jsd/", "purpose": "Shape delete"}),
    );
    let resp = d.delete("/specs/jsd?force=true");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "deleted");
    assert!(body["stem"].is_string());
    assert_eq!(body["stem"], "jsd");
}

#[test]
fn json_shape_list_returns_spec_object_array() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsl-a", "src": "crates/jsl-a/", "purpose": "List A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jsl-b", "src": "crates/jsl-b/", "purpose": "List B"}),
    );

    let resp = d.get("/specs");
    assert_eq!(resp.status(), 200);
    let specs: Vec<Value> = resp.json().unwrap();
    assert_eq!(specs.len(), 2);
    for spec in &specs {
        assert_spec_object(spec);
    }
}

#[test]
fn json_shape_search_returns_spec_object_array() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssrch", "src": "crates/jssrch/", "purpose": "Shape search"}),
    );

    let resp = d.get("/specs/search?q=jssrch");
    assert_eq!(resp.status(), 200);
    let results: Vec<Value> = resp.json().unwrap();
    assert_eq!(results.len(), 1);
    assert_spec_object(&results[0]);
}

#[test]
fn json_shape_count_ungrouped() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jscount", "src": "crates/jscount/", "purpose": "Count"}),
    );

    let resp = d.get("/specs/count");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert!(body["count"].is_number(), "count.count should be number");
    assert_eq!(body["count"], 1);
}

#[test]
fn json_shape_count_grouped() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jscg-a", "src": "crates/jscg-a/", "purpose": "A"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jscg-b", "src": "crates/jscg-b/", "purpose": "B"}),
    );
    d.patch("/specs/jscg-b", &json!({"status": "stable"}));

    let resp = d.get("/specs/count?by_status=true");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert!(body["count"].is_number());
    assert_eq!(body["count"], 2);
    let groups = body["groups"].as_array().unwrap();
    assert!(!groups.is_empty());
    for group in groups {
        assert!(group["status"].is_string());
        assert!(group["count"].is_number());
    }
}

#[test]
fn json_shape_status_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsstat", "src": "crates/jsstat/", "purpose": "Status"}),
    );

    let resp = d.get("/status");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert!(body.is_object(), "status should be an object");
    assert!(body["draft"].is_number(), "draft count should be number");
    // stable/proven may be absent if no specs have those statuses
    for key in body.as_object().unwrap().keys() {
        assert!(
            ["draft", "stable", "proven"].contains(&key.as_str()),
            "unexpected status key: {key}"
        );
        assert!(body[key].is_number());
    }
}

#[test]
fn json_shape_history_returns_event_array() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jshist", "src": "crates/jshist/", "purpose": "History"}),
    );
    d.patch("/specs/jshist", &json!({"purpose": "Updated"}));

    let resp = d.get("/specs/jshist/history");
    assert_eq!(resp.status(), 200);
    let events: Vec<Value> = resp.json().unwrap();
    assert!(events.len() >= 2);
    for ev in &events {
        assert_event_object(ev);
    }
}

#[test]
fn json_shape_section_add_returns_section_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssadd", "src": "crates/jssadd/", "purpose": "Sec add"}),
    );

    let resp = d.post(
        "/specs/jssadd/sections",
        &json!({"name": "Custom Sec", "body": "content"}),
    );
    assert_eq!(resp.status(), 201);
    let section: Value = resp.json().unwrap();
    assert_section_object(&section);
    assert_eq!(section["slug"], "custom-sec");
    assert_eq!(section["kind"], "custom");
}

#[test]
fn json_shape_section_set_returns_section_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssset", "src": "crates/jssset/", "purpose": "Sec set"}),
    );

    let resp = d.put(
        "/specs/jssset/sections/overview",
        &json!({"body": "New overview"}),
    );
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_section_object(&section);
    assert_eq!(section["body"], "New overview");
}

#[test]
fn json_shape_section_get_returns_section_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssget", "src": "crates/jssget/", "purpose": "Sec get"}),
    );

    let resp = d.get("/specs/jssget/sections/overview");
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_section_object(&section);
    assert_eq!(section["slug"], "overview");
    assert_eq!(section["kind"], "required");
}

#[test]
fn json_shape_section_list_returns_section_array() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsslist", "src": "crates/jsslist/", "purpose": "Sec list"}),
    );

    let resp = d.get("/specs/jsslist/sections");
    assert_eq!(resp.status(), 200);
    let sections: Vec<Value> = resp.json().unwrap();
    assert_eq!(sections.len(), 5);
    for section in &sections {
        assert_section_object(section);
    }
}

#[test]
fn json_shape_section_remove_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssrm", "src": "crates/jssrm/", "purpose": "Sec remove"}),
    );
    d.post(
        "/specs/jssrm/sections",
        &json!({"name": "Removable", "body": "bye"}),
    );

    let resp = d.delete("/specs/jssrm/sections/removable");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "removed");
    assert!(body["spec"].is_string());
    assert_eq!(body["spec"], "jssrm");
    assert!(body["slug"].is_string());
    assert_eq!(body["slug"], "removable");
}

#[test]
fn json_shape_section_move_returns_section_object() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jssmv", "src": "crates/jssmv/", "purpose": "Sec move"}),
    );
    d.post(
        "/specs/jssmv/sections",
        &json!({"name": "Movable", "body": "moving"}),
    );

    let resp = d.patch(
        "/specs/jssmv/sections/movable/move",
        &json!({"after": "overview"}),
    );
    assert_eq!(resp.status(), 200);
    let section: Value = resp.json().unwrap();
    assert_section_object(&section);
    assert_eq!(section["slug"], "movable");
}

#[test]
fn json_shape_ref_add_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsra-from", "src": "crates/jsra-from/", "purpose": "From"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jsra-to", "src": "crates/jsra-to/", "purpose": "To"}),
    );

    let resp = d.post("/specs/jsra-from/refs", &json!({"target": "jsra-to"}));
    assert_eq!(resp.status(), 201);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "added");
    assert!(body["from"].is_string());
    assert_eq!(body["from"], "jsra-from");
    assert!(body["to"].is_string());
    assert_eq!(body["to"], "jsra-to");
}

#[test]
fn json_shape_ref_remove_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsrr-from", "src": "crates/jsrr-from/", "purpose": "From"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jsrr-to", "src": "crates/jsrr-to/", "purpose": "To"}),
    );
    d.post("/specs/jsrr-from/refs", &json!({"target": "jsrr-to"}));

    let resp = d.delete("/specs/jsrr-from/refs/jsrr-to");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "removed");
    assert!(body["from"].is_string());
    assert_eq!(body["from"], "jsrr-from");
    assert!(body["to"].is_string());
    assert_eq!(body["to"], "jsrr-to");
}

#[test]
fn json_shape_ref_list_returns_spec_object_array() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsrl-from", "src": "crates/jsrl-from/", "purpose": "From"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jsrl-to", "src": "crates/jsrl-to/", "purpose": "To"}),
    );
    d.post("/specs/jsrl-from/refs", &json!({"target": "jsrl-to"}));

    let resp = d.get("/specs/jsrl-from/refs");
    assert_eq!(resp.status(), 200);
    let refs: Vec<Value> = resp.json().unwrap();
    assert_eq!(refs.len(), 1);
    assert!(refs[0]["stem"].is_string());
    assert_eq!(refs[0]["stem"], "jsrl-to");
}

#[test]
fn json_shape_ref_tree_nodes() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsrt-a", "src": "crates/jsrt-a/", "purpose": "Root"}),
    );
    d.post(
        "/specs",
        &json!({"stem": "jsrt-b", "src": "crates/jsrt-b/", "purpose": "Child"}),
    );
    d.post("/specs/jsrt-a/refs", &json!({"target": "jsrt-b"}));

    let resp = d.get("/specs/jsrt-a/refs/tree?direction=down");
    assert_eq!(resp.status(), 200);
    let nodes: Vec<Value> = resp.json().unwrap();
    assert!(nodes.len() >= 2);
    for node in &nodes {
        assert!(node["stem"].is_string(), "tree node.stem should be string");
        assert!(
            node["purpose"].is_string(),
            "tree node.purpose should be string"
        );
        assert!(
            node["status"].is_string(),
            "tree node.status should be string"
        );
        assert!(
            node["depth"].is_number(),
            "tree node.depth should be number"
        );
    }
}

#[test]
fn json_shape_ref_cycles_response() {
    let d = TestDaemon::start();

    let resp = d.get("/refs/cycles");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert!(body.is_array());
}

#[test]
fn json_shape_check_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jschk", "src": "crates/jschk/", "purpose": "Check"}),
    );

    let resp = d.get("/check");
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();
    assert!(report["ok"].is_boolean());
    assert!(report["errors"].is_array());
    assert!(report["warnings"].is_array());
    for err in report["errors"].as_array().unwrap() {
        assert!(err["check"].is_string());
        assert!(err["message"].is_string());
    }
    for warn in report["warnings"].as_array().unwrap() {
        assert!(warn["check"].is_string());
        assert!(warn["message"].is_string());
    }
}

#[test]
fn json_shape_doctor_response() {
    let d = TestDaemon::start();

    let resp = d.post("/doctor", &json!({}));
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().unwrap();
    assert!(report["findings"].is_array());
    assert!(report["fixes_applied"].is_array());
    for finding in report["findings"].as_array().unwrap() {
        assert!(finding["check"].is_string());
    }
}

#[test]
fn json_shape_export_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsexp", "src": "crates/jsexp/", "purpose": "Export"}),
    );

    let resp = d.post("/export", &json!({}));
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["specs"].is_number());
    assert!(body["sections"].is_number());
    assert!(body["refs"].is_number());
}

#[test]
fn json_shape_import_response() {
    let d = TestDaemon::start();
    d.post(
        "/specs",
        &json!({"stem": "jsimp", "src": "crates/jsimp/", "purpose": "Import"}),
    );
    d.post("/export", &json!({}));

    let resp = d.post("/import", &json!({}));
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().unwrap();
    assert!(body["specs"].is_number());
    assert!(body["sections"].is_number());
    assert!(body["refs"].is_number());
}

#[test]
fn json_shape_error_response() {
    let d = TestDaemon::start();

    let resp = d.get("/specs/nonexistent-shape-test");
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().unwrap();
    assert!(body["error"].is_string());
    assert!(body["code"].is_string());
    assert_eq!(body["code"], "not_found");
}

// ===========================================================================
// Full lifecycle end-to-end
// ===========================================================================

#[test]
fn full_lifecycle_end_to_end() {
    let d = TestDaemon::start();

    // --- 1. Create specs ---
    let resp = d.post(
        "/specs",
        &json!({"stem": "core", "src": "crates/core/", "purpose": "Core library"}),
    );
    assert_eq!(resp.status(), 201);

    let resp = d.post(
        "/specs",
        &json!({"stem": "api", "src": "crates/api/", "purpose": "REST API layer"}),
    );
    assert_eq!(resp.status(), 201);

    let resp = d.post(
        "/specs",
        &json!({"stem": "cli", "src": "crates/cli/", "purpose": "CLI frontend"}),
    );
    assert_eq!(resp.status(), 201);

    // Verify list returns all 3
    let resp = d.get("/specs");
    let specs: Vec<Value> = resp.json().unwrap();
    assert_eq!(specs.len(), 3);

    // --- 2. Fill required sections ---
    d.put(
        "/specs/core/sections/overview",
        &json!({"body": "Core provides shared types and utilities."}),
    );
    d.put(
        "/specs/core/sections/architecture",
        &json!({"body": "Single crate with modules: types, util, error."}),
    );
    d.put(
        "/specs/core/sections/dependencies",
        &json!({"body": "serde, thiserror"}),
    );
    d.put(
        "/specs/core/sections/error-handling",
        &json!({"body": "All errors use CoreError enum with thiserror derives."}),
    );
    d.put(
        "/specs/core/sections/testing",
        &json!({"body": "Unit tests per module, proptest for serialization round-trips."}),
    );

    d.put(
        "/specs/api/sections/overview",
        &json!({"body": "REST API built on axum."}),
    );

    // --- 3. Add custom sections ---
    let resp = d.post(
        "/specs/core/sections",
        &json!({"name": "Performance Notes", "body": "Target <10ms p99 for all operations.", "after": "testing"}),
    );
    assert_eq!(resp.status(), 201);

    // Verify section count: 5 required + 1 custom = 6
    let resp = d.get("/specs/core/sections");
    let sections: Vec<Value> = resp.json().unwrap();
    assert_eq!(sections.len(), 6);
    let custom = sections
        .iter()
        .find(|s| s["slug"] == "performance-notes")
        .unwrap();
    assert_eq!(custom["body"], "Target <10ms p99 for all operations.");
    assert_eq!(custom["kind"], "custom");

    // --- 4. Add cross-references ---
    let resp = d.post("/specs/api/refs", &json!({"target": "core"}));
    assert_eq!(resp.status(), 201);

    let resp = d.post("/specs/cli/refs", &json!({"target": "core"}));
    assert_eq!(resp.status(), 201);

    let resp = d.post("/specs/cli/refs", &json!({"target": "api"}));
    assert_eq!(resp.status(), 201);

    // Verify refs
    let resp = d.get("/specs/cli/refs");
    let refs: Vec<Value> = resp.json().unwrap();
    assert_eq!(refs.len(), 2);

    // Verify no cycles
    let resp = d.get("/refs/cycles");
    let cycles: Vec<Vec<String>> = resp.json().unwrap();
    assert!(cycles.is_empty(), "should have no reference cycles");

    // Verify ref tree (down from cli)
    let resp = d.get("/specs/cli/refs/tree?direction=down");
    let tree: Vec<Value> = resp.json().unwrap();
    assert!(tree.len() >= 2, "cli ref tree should include core and api");

    // --- 5. Status transitions ---
    d.patch("/specs/core", &json!({"status": "stable"}));
    d.patch("/specs/core", &json!({"status": "proven"}));

    let resp = d.get("/specs/core");
    let detail: Value = resp.json().unwrap();
    assert_eq!(detail["status"], "proven");

    // Verify list filter by status
    let resp = d.get("/specs?status=proven");
    let proven: Vec<Value> = resp.json().unwrap();
    assert_eq!(proven.len(), 1);
    assert_eq!(proven[0]["stem"], "core");

    // --- 6. Search ---
    let resp = d.get("/specs/search?q=REST");
    let results: Vec<Value> = resp.json().unwrap();
    assert!(
        results.iter().any(|r| r["stem"] == "api"),
        "search for 'REST' should find api spec"
    );

    // --- 7. History ---
    let resp = d.get("/specs/core/history");
    let events: Vec<Value> = resp.json().unwrap();
    assert!(
        events.len() >= 3,
        "core should have create + status updates in history"
    );

    // --- 8. Export ---
    // Create src dirs so check passes
    std::fs::create_dir_all(d._dir.path().join("crates/core")).unwrap();
    std::fs::create_dir_all(d._dir.path().join("crates/api")).unwrap();
    std::fs::create_dir_all(d._dir.path().join("crates/cli")).unwrap();

    let resp = d.post("/export", &json!({}));
    assert_eq!(resp.status(), 200);
    let export_result: Value = resp.json().unwrap();
    assert_eq!(export_result["specs"], 3);
    assert_eq!(export_result["sections"], 16); // 5 required * 3 + 1 custom
    assert_eq!(export_result["refs"], 3);

    let forma_dir = d._dir.path().join(".forma");
    assert!(forma_dir.join("specs.jsonl").exists());
    assert!(forma_dir.join("sections.jsonl").exists());
    assert!(forma_dir.join("refs.jsonl").exists());

    // Verify generated markdown
    let core_md = std::fs::read_to_string(forma_dir.join("specs/core.md")).unwrap();
    assert!(core_md.contains("# core Specification"));
    assert!(core_md.contains("Core provides shared types and utilities."));
    assert!(core_md.contains("## Performance Notes"));
    assert!(core_md.contains("| Status | proven |"));

    let cli_md = std::fs::read_to_string(forma_dir.join("specs/cli.md")).unwrap();
    assert!(cli_md.contains("## Related Specifications"));

    // Verify README
    let readme = std::fs::read_to_string(forma_dir.join("README.md")).unwrap();
    assert!(readme.contains("[core](specs/core.md)"));
    assert!(readme.contains("[api](specs/api.md)"));
    assert!(readme.contains("[cli](specs/cli.md)"));

    // --- 9. Import (rebuild from JSONL) ---
    let resp = d.post("/import", &json!({}));
    assert_eq!(resp.status(), 200);
    let import_result: Value = resp.json().unwrap();
    assert_eq!(import_result["specs"], 3);
    assert_eq!(import_result["sections"], 16);
    assert_eq!(import_result["refs"], 3);

    // Verify data survived round-trip
    let resp = d.get("/specs/core");
    let core_after: Value = resp.json().unwrap();
    assert_eq!(core_after["stem"], "core");
    assert_eq!(core_after["purpose"], "Core library");
    assert_eq!(core_after["status"], "proven");

    let sections_after = core_after["sections"].as_array().unwrap();
    assert_eq!(sections_after.len(), 6);
    let overview = sections_after
        .iter()
        .find(|s| s["slug"] == "overview")
        .unwrap();
    assert_eq!(
        overview["body"],
        "Core provides shared types and utilities."
    );
    let perf = sections_after
        .iter()
        .find(|s| s["slug"] == "performance-notes")
        .unwrap();
    assert_eq!(perf["body"], "Target <10ms p99 for all operations.");

    let empty_refs = vec![];
    let refs_after = core_after["refs"].as_array().unwrap_or(&empty_refs);
    // core has no outgoing refs (api and cli reference core, not vice versa)
    assert!(refs_after.is_empty());

    // Verify cli refs survived
    let resp = d.get("/specs/cli");
    let cli_after: Value = resp.json().unwrap();
    let cli_refs = cli_after["refs"].as_array().unwrap();
    assert_eq!(cli_refs.len(), 2);

    // --- 10. Check (validation report) ---
    let resp = d.get("/check");
    assert_eq!(resp.status(), 200);
    let check: Value = resp.json().unwrap();
    assert!(check["ok"].is_boolean());
    assert!(check["errors"].is_array());
    assert!(check["warnings"].is_array());

    // core has all sections filled → no warnings for core
    // api/cli have unfilled required sections → warnings expected
    let warnings = check["warnings"].as_array().unwrap();
    let api_warnings: Vec<_> = warnings
        .iter()
        .filter(|w| {
            w["check"] == "required_sections_nonempty"
                && w["message"].as_str().map_or(false, |m| m.contains("'api'"))
        })
        .collect();
    assert!(
        !api_warnings.is_empty(),
        "api should have empty-section warnings"
    );

    // --- 11. Doctor ---
    let resp = d.post("/doctor", &json!({}));
    assert_eq!(resp.status(), 200);
    let doctor: Value = resp.json().unwrap();
    assert!(doctor["findings"].is_array());
    assert!(doctor["fixes_applied"].is_array());

    // After export+import, no sync drift expected
    let drift: Vec<_> = doctor["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == "sync_drift")
        .collect();
    assert!(
        drift.is_empty(),
        "no sync drift expected after export+import"
    );

    // --- 12. Count ---
    let resp = d.get("/specs/count?by_status=true");
    let count: Value = resp.json().unwrap();
    assert!(count.is_object() || count.is_array());

    // --- 13. Project status ---
    let resp = d.get("/status");
    assert_eq!(resp.status(), 200);
}

#[test]
fn watchdog_tolerates_transient_directory_removal() {
    let data_dir = TempDir::new().expect("create data dir");
    let project_dir = data_dir.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let port = portpicker::pick_unused_port().expect("no free port");
    let pd = project_dir.clone();
    let dd = data_dir.path().join("data");

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        unsafe { std::env::set_var("FM_WATCHDOG_INTERVAL_MS", "200") };
        rt.block_on(forma::daemon::start_with_data_dir(port, pd, Some(dd)));
    });

    let client = reqwest::blocking::Client::new();
    let base = format!("http://localhost:{port}");
    for _ in 0..50 {
        if client.get(format!("{base}/status")).send().is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let daemon_alive = || client.get(format!("{base}/status")).send().is_ok();

    assert!(daemon_alive(), "daemon should be running");

    // Remove project dir — 1 failure, daemon should survive
    std::fs::remove_dir_all(&project_dir).unwrap();
    std::thread::sleep(Duration::from_millis(350));
    assert!(daemon_alive(), "daemon should survive 1 transient failure");

    // Recreate project dir — counter should reset
    std::fs::create_dir_all(&project_dir).unwrap();
    std::thread::sleep(Duration::from_millis(350));
    assert!(
        daemon_alive(),
        "daemon should survive after directory reappears"
    );

    // Remove again — wait long enough for 3+ consecutive failures
    std::fs::remove_dir_all(&project_dir).unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    assert!(
        !daemon_alive(),
        "daemon should shut down after 3 consecutive failures"
    );
}

#[test]
fn daemon_writes_project_file_on_startup() {
    let d = TestDaemon::start();
    let project_file = d.project_dir().join(".forma/daemon.project");
    let contents = std::fs::read_to_string(&project_file).expect("daemon.project should exist");
    let stored = contents.trim();
    assert!(!stored.is_empty(), "daemon.project should not be empty");
    let canonical = d
        .project_dir()
        .canonicalize()
        .expect("canonicalize project dir");
    assert_eq!(
        std::path::Path::new(stored),
        canonical,
        "daemon.project should contain the canonical project directory"
    );
}
