use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sgf_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sgf"))
}

fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd
}

fn git_init(root: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(root)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(root)
        .stdout(Stdio::null())
        .status()
        .unwrap();
}

fn git_add_commit(root: &Path, msg: &str) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", msg, "--allow-empty"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
}

/// Create a temp dir with git init + initial commit.
fn setup_test_dir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    git_init(tmp.path());
    fs::write(tmp.path().join("README.md"), "# test\n").unwrap();
    git_add_commit(tmp.path(), "initial");
    tmp
}

/// Create an executable shell script at `dir/name` with the given content.
fn create_mock_script(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Create a separate temp dir with mock `pn` (exits 0 for everything) and
/// return (TempDir, PATH string with mock dir prepended).
fn setup_mock_pn() -> (TempDir, String) {
    let mock_dir = TempDir::new().unwrap();
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (mock_dir, path)
}

/// Run `sgf init` in the given dir, then commit all generated files.
fn sgf_init_and_commit(dir: &Path) {
    let output = sgf_cmd(dir).arg("init").output().unwrap();
    assert!(output.status.success(), "sgf init failed");
    git_add_commit(dir, "sgf init");
}

/// Write a config.toml into .sgf/prompts/ with the given TOML content and commit.
fn write_config_toml(dir: &Path, content: &str) {
    let config_path = dir.join(".sgf/prompts/config.toml");
    fs::write(&config_path, content).unwrap();
    git_add_commit(dir, "add config.toml");
}

/// Create a spec file at specs/<stem>.md and commit it.
fn create_spec_and_commit(dir: &Path, stem: &str) {
    fs::create_dir_all(dir.join("specs")).unwrap();
    fs::write(
        dir.join(format!("specs/{stem}.md")),
        format!("# {stem} spec\n"),
    )
    .unwrap();
    git_add_commit(dir, &format!("add spec {stem}"));
}

// ===========================================================================
// sgf init
// ===========================================================================

#[test]
fn init_creates_all_directories() {
    let tmp = setup_test_dir();
    let output = sgf_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(output.status.success());

    let expected_dirs = [
        ".pensa",
        ".sgf",
        ".sgf/logs",
        ".sgf/run",
        ".sgf/prompts",
        "specs",
    ];
    for dir in expected_dirs {
        assert!(tmp.path().join(dir).is_dir(), "directory missing: {dir}");
    }
}

#[test]
fn init_creates_all_files() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let expected_files = [
        "BACKPRESSURE.md",
        ".sgf/prompts/spec.md",
        ".sgf/prompts/build.md",
        ".sgf/prompts/verify.md",
        ".sgf/prompts/test-plan.md",
        ".sgf/prompts/test.md",
        ".sgf/prompts/issues-log.md",
        ".sgf/prompts/install.md",
        ".sgf/prompts/config.toml",
        "specs/README.md",
        ".claude/settings.json",
        ".pre-commit-config.yaml",
        ".gitignore",
    ];
    for f in expected_files {
        assert!(tmp.path().join(f).is_file(), "file missing: {f}");
    }

    // CLAUDE.md should exist as a symlink
    let claude_md = tmp.path().join("CLAUDE.md");
    assert!(
        claude_md
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink(),
        "CLAUDE.md should be a symlink"
    );
}

#[test]
fn init_file_contents() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    // CLAUDE.md should be a symlink to AGENTS.md
    let claude_md = tmp.path().join("CLAUDE.md");
    let target = fs::read_link(&claude_md).unwrap();
    assert_eq!(target.to_str().unwrap(), "AGENTS.md");

    // .sgf/MEMENTO.md and .sgf/PENSA.md should NOT exist
    assert!(
        !tmp.path().join(".sgf/MEMENTO.md").exists(),
        ".sgf/MEMENTO.md should NOT be created"
    );
    assert!(
        !tmp.path().join(".sgf/PENSA.md").exists(),
        ".sgf/PENSA.md should NOT be created"
    );

    // .claude/settings.json
    let settings = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&settings).unwrap();
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    for rule in [
        "Edit .sgf/**",
        "Write .sgf/**",
        "Bash rm .sgf/**",
        "Bash mv .sgf/**",
    ] {
        assert!(
            deny.contains(&serde_json::Value::String(rule.to_string())),
            "missing deny rule: {rule}"
        );
    }

    // .gitignore
    let gitignore = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(!gitignore.contains(".pensa/db.sqlite"));
    assert!(gitignore.contains(".sgf/logs/"));
}

#[test]
fn init_idempotent() {
    let tmp = setup_test_dir();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    // Modify a prompt template
    let build_path = tmp.path().join(".sgf/prompts/build.md");
    fs::write(&build_path, "custom build prompt").unwrap();

    // Snapshot config files
    let gitignore1 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    let settings1 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    // Modified template should persist (not overwritten)
    assert_eq!(
        fs::read_to_string(&build_path).unwrap(),
        "custom build prompt"
    );

    // No duplicate gitignore lines
    let gitignore2 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert_eq!(gitignore1, gitignore2, ".gitignore changed on second run");

    // No duplicate deny rules
    let settings2 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    assert_eq!(settings1, settings2, "settings.json changed on second run");

    // No duplicate pre-commit hooks
    let precommit = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
    assert_eq!(
        precommit.matches("pensa-export").count(),
        1,
        "pensa-export duplicated"
    );
}

#[test]
fn init_merges_existing_gitignore() {
    let tmp = setup_test_dir();
    fs::write(
        tmp.path().join(".gitignore"),
        "# My Project\nmy-secret.key\n",
    )
    .unwrap();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(content.contains("my-secret.key"), "custom entry lost");
    assert!(
        !content.contains(".pensa/db.sqlite"),
        "db.sqlite should not be in gitignore"
    );
    assert!(content.contains(".sgf/logs/"), "sgf entry missing");
}

#[test]
fn init_merges_existing_settings_json() {
    let tmp = setup_test_dir();
    fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    fs::write(
        tmp.path().join(".claude/settings.json"),
        r#"{"permissions":{"deny":["Bash rm -rf /"]}}"#,
    )
    .unwrap();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert!(deny.contains(&serde_json::Value::String("Bash rm -rf /".to_string())));
    assert!(deny.contains(&serde_json::Value::String("Edit .sgf/**".to_string())));
    assert_eq!(deny.len(), 9, "expected 1 custom + 8 sgf rules");
}

// ===========================================================================
// Sandbox config scaffolding (E2E via sgf binary)
// ===========================================================================

#[test]
fn init_sandbox_config_scaffolded() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(doc["sandbox"]["enabled"], true, "sandbox.enabled");
    assert_eq!(
        doc["sandbox"]["autoAllowBashIfSandboxed"], true,
        "sandbox.autoAllowBashIfSandboxed"
    );
    assert_eq!(
        doc["sandbox"]["network"]["allowLocalBinding"], true,
        "sandbox.network.allowLocalBinding"
    );
    assert!(
        doc["sandbox"]["filesystem"].is_null(),
        "sandbox.filesystem should not be scaffolded"
    );
    assert!(
        doc["sandbox"]["allowUnsandboxedCommands"].is_null(),
        "sandbox.allowUnsandboxedCommands should not be scaffolded"
    );

    let domains = doc["sandbox"]["network"]["allowedDomains"]
        .as_array()
        .expect("allowedDomains should be an array");
    for expected in ["localhost", "github.com", "crates.io"] {
        assert!(
            domains.contains(&serde_json::json!(expected)),
            "allowedDomains missing: {expected}"
        );
    }
}

#[test]
fn init_sandbox_no_duplicates_on_rerun() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let first = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc1: serde_json::Value = serde_json::from_str(&first).unwrap();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let second = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc2: serde_json::Value = serde_json::from_str(&second).unwrap();

    assert_eq!(
        doc1["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap()
            .len(),
        doc2["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap()
            .len(),
        "allowedDomains duplicated on rerun"
    );
    assert_eq!(first, second, "settings.json changed on rerun");
}

#[test]
fn init_sandbox_preserves_custom_config() {
    let tmp = setup_test_dir();
    fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    fs::write(
        tmp.path().join(".claude/settings.json"),
        r#"{
  "sandbox": {
    "enabled": false,
    "network": { "allowedDomains": ["custom.example.com"], "allowLocalBinding": false }
  }
}"#,
    )
    .unwrap();

    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Scalars already present should NOT be overwritten
    assert_eq!(
        doc["sandbox"]["enabled"], false,
        "existing enabled=false should be preserved"
    );
    assert_eq!(
        doc["sandbox"]["network"]["allowLocalBinding"], false,
        "existing allowLocalBinding=false should be preserved"
    );

    let domains = doc["sandbox"]["network"]["allowedDomains"]
        .as_array()
        .unwrap();
    assert!(
        domains.contains(&serde_json::json!("custom.example.com")),
        "custom domain lost"
    );
    assert!(
        domains.contains(&serde_json::json!("localhost")),
        "default domain not added"
    );
    assert!(
        domains.contains(&serde_json::json!("github.com")),
        "default domain not added"
    );
    assert!(
        domains.contains(&serde_json::json!("crates.io")),
        "default domain not added"
    );
}

// ===========================================================================
// Prompt validation (library-level, called from integration test context)
// ===========================================================================

#[test]
fn prompt_validate_existing_template() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(
        tmp.path().join(".sgf/prompts/build.md"),
        "Build the spec now.",
    )
    .unwrap();

    let result = springfield::prompt::validate(tmp.path(), "build", None).unwrap();
    assert!(result.ends_with(".sgf/prompts/build.md"));
}

#[test]
fn prompt_validate_missing_template() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

    let err = springfield::prompt::validate(tmp.path(), "nonexistent", None).unwrap_err();
    assert!(err.to_string().contains("template not found"));
}

#[test]
fn prompt_validate_spec_exists() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::create_dir_all(tmp.path().join("specs")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "Build prompt.").unwrap();
    fs::write(tmp.path().join("specs/auth.md"), "# Auth spec").unwrap();

    let result = springfield::prompt::validate(tmp.path(), "build", Some("auth")).unwrap();
    assert!(result.ends_with(".sgf/prompts/build.md"));
}

#[test]
fn prompt_validate_spec_missing() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "Build prompt.").unwrap();

    let err = springfield::prompt::validate(tmp.path(), "build", Some("auth")).unwrap_err();
    assert!(
        err.to_string().contains("spec not found: specs/auth.md"),
        "should report missing spec: {}",
        err
    );
}

#[test]
fn prompt_validate_returns_raw_path() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    let content = "No variables here, just plain text.";
    fs::write(tmp.path().join(".sgf/prompts/verify.md"), content).unwrap();

    let result = springfield::prompt::validate(tmp.path(), "verify", None).unwrap();
    let read_back = fs::read_to_string(&result).unwrap();
    assert_eq!(read_back, content);
}

// ===========================================================================
// Loop orchestration (mocked ralph via CLI)
// ===========================================================================

#[test]
fn build_invokes_ralph_with_correct_flags() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"afk\"\niterations = 30\nauto_push = true\n",
    );
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that logs all args
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("-a"), "missing --afk flag");
    assert!(args.contains("--loop-id"), "missing --loop-id");
    assert!(
        args.contains("--auto-push true"),
        "missing --auto-push true"
    );
    assert!(
        !args.contains("--max-iterations"),
        "should not pass --max-iterations"
    );
}

#[test]
fn build_creates_and_cleans_pid_file() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that checks for PID file existence during execution
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("pid_state.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\nls \"${{PWD}}/.sgf/run/\"*.pid > \"{}\" 2>&1\nexit 0\n",
            state_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(output.status.success());

    // PID file should have existed while ralph was running
    let pid_state = fs::read_to_string(&state_file).unwrap();
    assert!(
        pid_state.contains(".pid"),
        "PID file should exist during ralph execution"
    );

    // PID file should be cleaned up after ralph exits
    let run_dir = tmp.path().join(".sgf/run");
    let remaining_pids: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "pid"))
        .collect();
    assert!(remaining_pids.is_empty(), "PID file not cleaned up");
}

#[test]
fn afk_passes_log_file_to_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(output.status.success());

    let args_content = fs::read_to_string(mock_dir.path().join("ralph_args.txt")).unwrap();
    assert!(
        args_content.contains("--log-file"),
        "should pass --log-file to ralph, got: {args_content}"
    );
    assert!(
        args_content.contains(".sgf/logs/"),
        "log-file path should be in .sgf/logs/, got: {args_content}"
    );
}

#[test]
fn spec_runs_interactive_via_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf spec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("--verbose"), "should pass --verbose to agent");
    assert!(args.contains("@"), "should pass prompt via @ prefix");
    assert!(
        args.contains(".sgf/prompts/spec.md"),
        "should pass raw spec prompt path"
    );
}

#[test]
fn issues_log_runs_interactive_via_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["issues-log"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf issues-log failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("--verbose"), "should pass --verbose to agent");
    assert!(args.contains("@"), "should pass prompt via @ prefix");
    assert!(
        args.contains(".sgf/prompts/issues-log.md"),
        "should pass raw issues-log prompt path"
    );
}

// ===========================================================================
// Recovery
// ===========================================================================

#[test]
fn recovery_cleans_stale_state() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that records the state of the working directory
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("recovery_state.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f untracked_dirty.txt ]; then\n",
                "  echo 'DIRTY' > \"{state}\"\n",
                "else\n",
                "  echo 'CLEAN' > \"{state}\"\n",
                "fi\n",
                "exit 0\n",
            ),
            state = state_file.display()
        ),
    );

    // Create dirty state: stale PID file + untracked file
    fs::write(tmp.path().join(".sgf/run/stale-loop.pid"), "4000000").unwrap();
    fs::write(tmp.path().join("untracked_dirty.txt"), "dirty").unwrap();
    // Modify a tracked file
    fs::write(tmp.path().join("README.md"), "modified").unwrap();

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Recovery should have cleaned state BEFORE ralph ran
    let state = fs::read_to_string(&state_file).unwrap();
    assert!(
        state.trim() == "CLEAN",
        "recovery should clean state before ralph: got {state}"
    );

    // Stale PID file should be removed (sgf creates a new one, then removes it)
    assert!(
        !tmp.path().join(".sgf/run/stale-loop.pid").exists(),
        "stale PID file should be removed"
    );
}

#[test]
fn recovery_skips_when_live_pid() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that records working dir state
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("recovery_state.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f untracked_dirty.txt ]; then\n",
                "  echo 'DIRTY' > \"{state}\"\n",
                "else\n",
                "  echo 'CLEAN' > \"{state}\"\n",
                "fi\n",
                "exit 0\n",
            ),
            state = state_file.display()
        ),
    );

    // Create a PID file with our own PID (alive) to block recovery
    fs::write(
        tmp.path().join(".sgf/run/live-loop.pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    // Create dirty state that recovery would clean
    fs::write(tmp.path().join("untracked_dirty.txt"), "dirty").unwrap();

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Recovery should NOT have run (live PID exists), so dirty state persists
    let state = fs::read_to_string(&state_file).unwrap();
    assert!(
        state.trim() == "DIRTY",
        "recovery should skip when live PID exists: got {state}"
    );

    // Live PID file should still exist (not our loop's PID file)
    assert!(tmp.path().join(".sgf/run/live-loop.pid").exists());
}

// ===========================================================================
// Utility commands
// ===========================================================================

#[test]
fn logs_exits_1_for_missing() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = sgf_cmd(tmp.path())
        .args(["logs", "nonexistent"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "should have error message");
}

#[test]
fn status_prints_placeholder() {
    let tmp = setup_test_dir();

    let output = sgf_cmd(tmp.path()).arg("status").output().unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not yet implemented"));
}

#[test]
fn help_flag() {
    let output = Command::new(sgf_bin()).arg("--help").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"), "help should list init built-in");
    assert!(
        stdout.contains("status"),
        "help should list status built-in"
    );
    assert!(stdout.contains("logs"), "help should list logs built-in");
}

// ===========================================================================
// ===========================================================================
// Spec-parity integration tests (implementation plan)
// ===========================================================================

#[test]
fn init_specs_readme_heading() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let content = fs::read_to_string(tmp.path().join("specs/README.md")).unwrap();
    assert!(
        content.starts_with("# Specifications"),
        "specs/README.md should start with '# Specifications', got: {}",
        content.lines().next().unwrap_or("")
    );
    assert!(
        !content.starts_with("# Specs\n"),
        "specs/README.md should NOT use '# Specs'"
    );
}

#[test]
fn templates_no_read_memento_directive() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    let templates = [
        ".sgf/prompts/spec.md",
        ".sgf/prompts/build.md",
        ".sgf/prompts/verify.md",
        ".sgf/prompts/test-plan.md",
        ".sgf/prompts/test.md",
        ".sgf/prompts/issues-log.md",
    ];

    for tmpl in templates {
        let content = fs::read_to_string(tmp.path().join(tmpl)).unwrap();
        assert!(
            !content.contains("Read `memento.md`"),
            "{tmpl} should NOT contain 'Read `memento.md`' (sgf handles injection)"
        );
    }
}

#[test]
fn templates_reference_uppercase_filenames() {
    let tmp = setup_test_dir();
    sgf_cmd(tmp.path()).arg("init").output().unwrap();

    // test.md should reference root BACKPRESSURE.md (uppercase, not inside .sgf/)
    let test = fs::read_to_string(tmp.path().join(".sgf/prompts/test.md")).unwrap();
    assert!(
        test.contains("BACKPRESSURE.md"),
        "test.md should reference BACKPRESSURE.md (uppercase)"
    );
    assert!(
        !test.contains("backpressure.md"),
        "test.md should NOT reference backpressure.md (lowercase)"
    );
    // test.md should use pn claim workflow, not reference .sgf/PENSA.md
    assert!(
        test.contains("pn claim workflow"),
        "test.md should reference pn claim workflow"
    );
    assert!(
        !test.contains(".sgf/PENSA.md"),
        "test.md should NOT reference .sgf/PENSA.md"
    );
}

#[test]
fn end_to_end_build_passes_raw_path_and_spec() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Create spec file (validate requires it)
    fs::create_dir_all(tmp.path().join("specs")).unwrap();
    fs::write(tmp.path().join("specs/auth.md"), "# Auth spec").unwrap();
    git_add_commit(tmp.path(), "add spec");

    // Mock ralph that logs args and env
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let env_file = mock_dir.path().join("ralph_env.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\necho \"SGF_SPEC=$SGF_SPEC\" > \"{}\"\nexit 0\n",
            args_file.display(),
            env_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Ralph should receive raw prompt path (not assembled)
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass raw prompt path, got: {args}"
    );
    assert!(
        !args.contains(".assembled"),
        "should NOT pass assembled path, got: {args}"
    );
    assert!(
        args.contains("--spec auth"),
        "should pass --spec to ralph, got: {args}"
    );

    // Ralph should receive SGF_SPEC env var
    let env = fs::read_to_string(&env_file).unwrap();
    assert!(
        env.contains("SGF_SPEC=auth"),
        "should set SGF_SPEC env var, got: {env}"
    );
}

// ===========================================================================
// End-to-end: sgf init with root-level BACKPRESSURE.md
// ===========================================================================

#[test]
fn init_end_to_end_root_backpressure() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // (1) BACKPRESSURE.md exists at root with expected template content
    let bp_path = tmp.path().join("BACKPRESSURE.md");
    assert!(
        bp_path.is_file(),
        "BACKPRESSURE.md should exist at project root"
    );
    let bp_content = fs::read_to_string(&bp_path).unwrap();
    assert!(
        bp_content.contains("# Backpressure"),
        "BACKPRESSURE.md should contain expected heading"
    );
    assert!(
        bp_content.contains("cargo build"),
        "BACKPRESSURE.md should contain Rust build commands"
    );
    assert!(
        bp_content.contains("cargo test"),
        "BACKPRESSURE.md should contain Rust test commands"
    );
    assert!(
        bp_content.contains("cargo clippy"),
        "BACKPRESSURE.md should contain Rust lint commands"
    );

    // (2) .sgf/BACKPRESSURE.md does NOT exist
    assert!(
        !tmp.path().join(".sgf/BACKPRESSURE.md").exists(),
        "BACKPRESSURE.md should NOT exist inside .sgf/"
    );

    // (3) .sgf/MEMENTO.md and .sgf/PENSA.md should NOT exist
    assert!(
        !tmp.path().join(".sgf/MEMENTO.md").exists(),
        ".sgf/MEMENTO.md should NOT be created"
    );
    assert!(
        !tmp.path().join(".sgf/PENSA.md").exists(),
        ".sgf/PENSA.md should NOT be created"
    );

    // (4) .claude/settings.json deny rules do NOT cover root BACKPRESSURE.md
    let settings = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&settings).unwrap();
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    // All deny rules target .sgf/** — root BACKPRESSURE.md is not blocked
    for rule in deny {
        let rule_str = rule.as_str().unwrap();
        assert!(
            !rule_str.contains("BACKPRESSURE"),
            "deny rules should not block root BACKPRESSURE.md, found: {rule_str}"
        );
    }
    // Confirm .sgf/** rules exist (they protect .sgf/ contents, not root files)
    assert!(
        deny.contains(&serde_json::Value::String("Edit .sgf/**".to_string())),
        "deny rules should protect .sgf/ contents"
    );
}

// ===========================================================================
// End-to-end: sgf init --force restores templates matching spec
// ===========================================================================

#[test]
fn init_force_restores_templates_matching_spec() {
    let tmp = setup_test_dir();

    let output = sgf_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(output.status.success(), "sgf init failed");

    // Remove pre-commit hook so git commits succeed in the test env
    let _ = fs::remove_file(tmp.path().join(".git/hooks/pre-commit"));

    git_add_commit(tmp.path(), "sgf init");

    // Modify templates with stale content
    fs::write(
        tmp.path().join(".sgf/prompts/build.md"),
        "stale build prompt with old task wording\n",
    )
    .unwrap();
    fs::write(tmp.path().join("BACKPRESSURE.md"), "stale backpressure\n").unwrap();
    git_add_commit(tmp.path(), "stale templates");

    // Run sgf init --force, piping "y" to confirm
    let output = sgf_cmd(tmp.path())
        .args(["init", "--force"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"y\n").unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(
        output.status.success(),
        "sgf init --force failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // (1) .sgf/PENSA.md should NOT exist
    assert!(
        !tmp.path().join(".sgf/PENSA.md").exists(),
        ".sgf/PENSA.md should NOT be created by force init"
    );

    // (2) .sgf/prompts/build.md restored — uses 'issue' terminology
    let build = fs::read_to_string(tmp.path().join(".sgf/prompts/build.md")).unwrap();
    assert!(
        build.contains("issue"),
        "build.md should use 'issue' terminology"
    );
    assert!(
        !build.contains("stale"),
        "build.md should have been overwritten (no stale content)"
    );

    // (3) BACKPRESSURE.md at root restored with real content
    let bp = fs::read_to_string(tmp.path().join("BACKPRESSURE.md")).unwrap();
    assert!(
        bp.contains("# Backpressure"),
        "BACKPRESSURE.md should be restored with template content"
    );
    assert!(
        bp.contains("cargo build"),
        "BACKPRESSURE.md should contain cargo build command"
    );

    // (4) specs/README.md should NOT be overwritten by --force
    let specs_readme = fs::read_to_string(tmp.path().join("specs/README.md")).unwrap();
    assert!(
        specs_readme.contains("# Specifications"),
        "specs/README.md should still exist"
    );

    // (5) Config files should still be intact after force
    let settings = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&settings).unwrap();
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert!(
        deny.contains(&serde_json::Value::String("Edit .sgf/**".to_string())),
        "deny rules should still be present after force init"
    );
}

// ===========================================================================
// End-to-end: sgf build without spec argument
// ===========================================================================

#[test]
fn build_without_spec_omits_spec_flag_and_env() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let env_file = mock_dir.path().join("ralph_env.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\necho \"SGF_SPEC=${{SGF_SPEC:-}}\" > \"{}\"\nexit 0\n",
            args_file.display(),
            env_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build without spec should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        !args.contains("--spec"),
        "should NOT pass --spec flag when no spec given, got: {args}"
    );
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should still pass raw prompt path, got: {args}"
    );

    let env = fs::read_to_string(&env_file).unwrap();
    assert!(
        env.contains("SGF_SPEC="),
        "SGF_SPEC line should exist in env capture"
    );
    assert!(
        !env.contains("SGF_SPEC=a") && !env.contains("SGF_SPEC=b"),
        "SGF_SPEC should be empty when no spec given, got: {env}"
    );
    // More precise: SGF_SPEC should be unset (empty after :-})
    let spec_val = env
        .lines()
        .find(|l| l.starts_with("SGF_SPEC="))
        .unwrap_or("SGF_SPEC=");
    assert_eq!(
        spec_val, "SGF_SPEC=",
        "SGF_SPEC should be empty/unset, got: {spec_val}"
    );
}

#[test]
fn build_with_spec_still_passes_spec_flag_and_env() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let env_file = mock_dir.path().join("ralph_env.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\necho \"SGF_SPEC=${{SGF_SPEC:-}}\" > \"{}\"\nexit 0\n",
            args_file.display(),
            env_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build with spec should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--spec auth"),
        "should pass --spec auth, got: {args}"
    );

    let env = fs::read_to_string(&env_file).unwrap();
    assert!(
        env.contains("SGF_SPEC=auth"),
        "should set SGF_SPEC=auth, got: {env}"
    );
}

// ---------------------------------------------------------------------------
// Spec validation (CLI-level)
// ---------------------------------------------------------------------------

#[test]
fn build_nonexistent_spec_fails_with_error() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let output = sgf_cmd(tmp.path())
        .args(["build", "nonexistent", "-a"])
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 for missing spec"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("spec not found"),
        "stderr should contain 'spec not found': {stderr}"
    );
    assert!(
        stderr.contains("specs/nonexistent.md"),
        "stderr should name the missing spec file: {stderr}"
    );
}

#[test]
fn build_valid_spec_proceeds() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build with valid spec should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ===========================================================================
// sgf spec auto-push after interactive session
// ===========================================================================

/// Set up a bare remote and configure the local repo to push to it.
fn setup_bare_remote(local: &Path) -> TempDir {
    let bare = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "--bare"])
        .current_dir(bare.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    Command::new("git")
        .args(["remote", "add", "origin", &bare.path().to_string_lossy()])
        .current_dir(local)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    // Push initial state so the remote has a branch to track
    Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(local)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    bare
}

/// Read HEAD from a bare remote repo.
fn bare_remote_head(bare: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(bare)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn spec_auto_pushes_after_session_with_new_commits() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[spec]\nmode = \"interactive\"\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock cl that creates a new commit
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo 'agent was here' > \"{root}/agent_output.txt\"\n",
                "cd \"{root}\"\n",
                "git add .\n",
                "git commit -m 'agent commit' --allow-empty\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf spec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_ne!(
        head_before, head_after,
        "remote HEAD should have advanced after auto-push"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("New commits detected"),
        "should log push detection: {stderr}"
    );
}

#[test]
fn spec_no_push_suppresses_auto_push() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock cl that creates a new commit
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo 'agent was here' > \"{root}/agent_output.txt\"\n",
                "cd \"{root}\"\n",
                "git add .\n",
                "git commit -m 'agent commit' --allow-empty\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["spec", "--no-push"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf spec --no-push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_eq!(
        head_before, head_after,
        "remote HEAD should NOT advance with --no-push"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("new commits detected"),
        "should NOT attempt push with --no-push: {stderr}"
    );
}

#[test]
fn spec_no_push_when_head_unchanged() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock cl that does NOT create any commits
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf spec (no-op agent) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_eq!(
        head_before, head_after,
        "remote HEAD should NOT advance when agent makes no commits"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("new commits detected"),
        "should NOT attempt push when HEAD unchanged: {stderr}"
    );
}

// ===========================================================================
// sgf doc auto-push after interactive session
// ===========================================================================

#[test]
fn doc_auto_pushes_after_session_with_new_commits() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[doc]\nmode = \"interactive\"\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo 'agent was here' > \"{root}/agent_output.txt\"\n",
                "cd \"{root}\"\n",
                "git add .\n",
                "git commit -m 'agent commit' --allow-empty\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("doc")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf doc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_ne!(
        head_before, head_after,
        "remote HEAD should have advanced after auto-push"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("New commits detected"),
        "should log push detection: {stderr}"
    );
}

#[test]
fn doc_runs_interactive_via_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("doc")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf doc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("--verbose"), "should pass --verbose to agent");
    assert!(args.contains("@"), "should pass prompt via @ prefix");
    assert!(
        args.contains(".sgf/prompts/doc.md"),
        "should pass raw doc prompt path, got: {args}"
    );
}

#[test]
fn doc_no_push_suppresses_auto_push() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo 'agent was here' > \"{root}/agent_output.txt\"\n",
                "cd \"{root}\"\n",
                "git add .\n",
                "git commit -m 'agent commit' --allow-empty\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["doc", "--no-push"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf doc --no-push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_eq!(
        head_before, head_after,
        "remote HEAD should NOT advance with --no-push"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("new commits detected"),
        "should NOT attempt push with --no-push: {stderr}"
    );
}

#[test]
fn doc_no_push_when_head_unchanged() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("doc")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf doc (no-op agent) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let head_after = bare_remote_head(bare.path());
    assert_eq!(
        head_before, head_after,
        "remote HEAD should NOT advance when agent makes no commits"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("new commits detected"),
        "should NOT attempt push when HEAD unchanged: {stderr}"
    );
}

// ===========================================================================
// Signal handling (shutdown controller integration)
// ===========================================================================

fn create_slow_mock_ralph(dir: &Path) -> PathBuf {
    create_mock_script(
        dir,
        "mock_ralph_slow.sh",
        "#!/bin/bash\ntrap '' INT\nfor i in $(seq 1 50); do sleep 0.1; done\nexit 2\n",
    )
}

/// Wait for the sgf process to signal readiness via `SGF_READY_FILE`.
/// Polls every 25ms, panics after 5 seconds.
fn wait_for_ready(ready_path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if ready_path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "sgf did not signal readiness within 5s (expected file: {})",
        ready_path.display()
    );
}

#[test]
fn double_ctrl_c_exits_130() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_slow_mock_ralph(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C"
    );
}

#[test]
fn single_ctrl_c_continues_after_timeout() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_quick.sh",
        "#!/bin/bash\ntrap '' INT\nsleep 3\nexit 0\n",
    );
    let ready_file = mock_dir.path().join("sgf_ready");

    let child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send single SIGINT");

    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(0),
        "should exit 0 (ralph completed) after single Ctrl+C timeout, not 130"
    );
}

#[test]
fn sigterm_exits_immediately() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_slow_mock_ralph(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).expect("send SIGTERM");

    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on SIGTERM"
    );
}

#[test]
fn confirmation_message_on_first_ctrl_c() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_slow_mock_ralph(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child.wait_with_output().expect("wait for sgf");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Press Ctrl-C again to exit"),
        "should show double-press prompt on stderr, got:\n{stderr}"
    );
}

#[test]
fn double_ctrl_d_exits_130() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_slow_mock_ralph(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    // Closing a piped stdin causes continuous EOF (read returns 0), which
    // simulates a rapid double Ctrl+D press.
    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_MONITOR_STDIN", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let stdin = child.stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    drop(stdin);

    std::thread::sleep(Duration::from_millis(1000));
    let output = match child.try_wait() {
        Ok(Some(_)) => child.wait_with_output().expect("wait for sgf"),
        _ => {
            nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)
                .expect("send cleanup SIGTERM");
            child.wait_with_output().expect("wait for sgf")
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+D, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Press Ctrl-D again to exit"),
        "should show Ctrl-D prompt on stderr, got:\n{stderr}"
    );
}

#[test]
fn mixed_ctrl_c_then_ctrl_d_no_shutdown() {
    use std::io::Write;

    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_mixed.sh",
        "#!/bin/bash\ntrap '' INT\nfor i in $(seq 1 50); do sleep 0.1; done\nexit 0\n",
    );
    let ready_file = mock_dir.path().join("sgf_ready");

    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_MONITOR_STDIN", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let mut stdin = child.stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);

    // Send Ctrl+C (SIGINT) — starts the Ctrl+C confirmation window.
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send SIGINT");
    std::thread::sleep(Duration::from_millis(200));

    // Send non-EOF data to stdin. This verifies that stdin activity from a
    // different channel (not Ctrl+D EOF) does not satisfy the Ctrl+C
    // double-press requirement.
    let _ = stdin.write_all(b"x\n");
    let _ = stdin.flush();

    // Wait past the 2-second timeout so the Ctrl+C pending state resets.
    std::thread::sleep(Duration::from_millis(2500));

    // The process should still be alive — mixed input did not cause shutdown.
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "process should still be running after Ctrl+C + non-EOF stdin"
    );

    // Clean up.
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).expect("send cleanup SIGTERM");
    drop(stdin);

    let output = child.wait_with_output().expect("wait for sgf");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Press Ctrl-C again to exit"),
        "should have shown Ctrl-C prompt, got:\n{stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 from SIGTERM cleanup, stderr:\n{stderr}"
    );
}

#[test]
fn double_ctrl_c_kills_entire_process_tree() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let grandchild_pid_file = mock_dir.path().join("grandchild.pid");
    let ready_file = mock_dir.path().join("sgf_ready");

    // Mock ralph that spawns a long-running grandchild, writes the grandchild's
    // PID to a file, then sleeps. The grandchild sleeps forever — if it's still
    // alive after sgf exits, the process tree was NOT fully killed.
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_tree.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "trap '' INT\n",
                "sleep 3600 &\n",
                "echo $! > \"{}\"\n",
                "for i in $(seq 1 500); do sleep 0.1; done\n",
                "exit 2\n",
            ),
            grandchild_pid_file.display()
        ),
    );

    let child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);

    // Wait for the grandchild PID file to be written.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !grandchild_pid_file.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "grandchild PID file was not created within 5s"
        );
        std::thread::sleep(Duration::from_millis(25));
    }

    let grandchild_pid_str = fs::read_to_string(&grandchild_pid_file)
        .expect("read grandchild PID file")
        .trim()
        .to_string();
    let grandchild_pid: i32 = grandchild_pid_str
        .parse()
        .unwrap_or_else(|_| panic!("invalid grandchild PID: {grandchild_pid_str}"));

    // Verify the grandchild is alive before we send signals.
    assert_eq!(
        unsafe { libc::kill(grandchild_pid, 0) },
        0,
        "grandchild (pid {grandchild_pid}) should be alive before SIGINT"
    );

    // Send double SIGINT (Ctrl+C) to sgf.
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C"
    );

    // Give a moment for process group cleanup to propagate.
    std::thread::sleep(Duration::from_millis(200));

    // Verify the grandchild is dead — kill(pid, 0) returns -1 with ESRCH
    // when the process doesn't exist.
    let alive = unsafe { libc::kill(grandchild_pid, 0) };
    assert_eq!(
        alive,
        -1,
        "grandchild (pid {grandchild_pid}) should be dead after double Ctrl+C, \
         but kill(pid, 0) returned {alive} (errno: {})",
        std::io::Error::last_os_error()
    );
}

// ===========================================================================
// Mode-dependent shutdown behavior
// ===========================================================================

#[test]
fn afk_mode_child_gets_new_session() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let sid_file = mock_dir.path().join("sid_info.txt");

    // Mock ralph that checks if it became a session leader.
    // setsid() creates a new session AND new process group where PGID == PID.
    // We get bash's own PID ($$) and its PGID via python os.getpgid(parent).
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_sid.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "MY_PID=$$\n",
                "MY_PGID=$(python3 -c \"import os; print(os.getpgid($MY_PID))\")\n",
                "echo \"pid=$MY_PID pgid=$MY_PGID\" > \"{}\"\n",
                "exit 0\n",
            ),
            sid_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build -a failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let info = fs::read_to_string(&sid_file).unwrap();
    let get_val = |key: &str| -> String {
        info.split_whitespace()
            .find(|s| s.starts_with(&format!("{key}=")))
            .unwrap_or("")
            .split('=')
            .nth(1)
            .unwrap_or("")
            .to_string()
    };
    let pid = get_val("pid");
    let pgid = get_val("pgid");

    // In AFK mode with setsid(), the child becomes a session leader
    // and process group leader: PGID == PID.
    assert_eq!(
        pgid, pid,
        "AFK child PGID should equal its PID (setsid), info: {info}"
    );
}

#[test]
fn default_build_without_config_runs_interactive() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    // Without config.toml, build defaults to interactive mode (cl, not ralph)
    let output = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build (interactive default) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--verbose"),
        "interactive mode should pass --verbose to agent"
    );
    assert!(
        args.contains("@"),
        "interactive mode should pass prompt via @ prefix"
    );
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass raw build prompt path"
    );
}

#[test]
fn interactive_mode_stdin_eof_does_not_trigger_shutdown() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock cl that sleeps then exits 0
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        "#!/bin/bash\ntrap '' INT\nsleep 2\nexit 0\n",
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    // Interactive mode (default without config): monitor_stdin should be false,
    // so closing stdin should NOT cause shutdown.
    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let stdin = child.stdin.take().expect("open stdin");

    std::thread::sleep(Duration::from_millis(500));
    drop(stdin); // Close stdin — should NOT trigger shutdown in interactive mode

    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(0),
        "interactive mode should NOT shutdown on stdin EOF"
    );
}

#[test]
fn afk_monitor_stdin_eof_triggers_shutdown() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_slow_mock_ralph(mock_dir.path());

    let ready_file = mock_dir.path().join("sgf_ready");

    // AFK mode with SGF_MONITOR_STDIN=1: closing piped stdin triggers EOF shutdown.
    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("SGF_MONITOR_STDIN", "1")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let stdin = child.stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    wait_for_ready(&ready_file);
    drop(stdin); // EOF triggers double-Ctrl+D detection

    std::thread::sleep(Duration::from_millis(1000));
    let output = match child.try_wait() {
        Ok(Some(_)) => child.wait_with_output().expect("wait for sgf"),
        _ => {
            nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)
                .expect("send cleanup SIGTERM");
            child.wait_with_output().expect("wait for sgf")
        }
    };

    assert_eq!(
        output.status.code(),
        Some(130),
        "AFK mode with monitor_stdin should exit 130 on stdin EOF"
    );
}

#[test]
fn config_afk_mode_invokes_ralph_with_afk_flag() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"afk\"\niterations = 30\nauto_push = false\n",
    );
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build (afk via config) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    let argv: Vec<&str> = args.trim().split_whitespace().collect();
    assert!(
        argv.contains(&"-a"),
        "AFK mode from config should pass -a flag to ralph, got: {args}"
    );
}

#[test]
fn interactive_override_does_not_invoke_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"afk\"\niterations = 30\nauto_push = false\n",
    );
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    // -i overrides config mode=afk to interactive
    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-i"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build -i failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--verbose"),
        "-i should force interactive mode using cl"
    );
}

#[test]
fn alias_resolves_to_prompt() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nalias = \"b\"\nmode = \"afk\"\nauto_push = false\n",
    );

    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["b", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf b (alias for build) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "alias 'b' should resolve to build.md, got: {args}"
    );
}

// ---------------------------------------------------------------------------
// Styled console output integration tests
// ---------------------------------------------------------------------------

#[test]
fn no_color_badge_falls_back_to_plain_prefix() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf:"),
        "NO_COLOR=1 should produce plain 'sgf:' prefix, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("\x1b["),
        "NO_COLOR=1 should produce no ANSI escape codes, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("╭─────╮"),
        "NO_COLOR=1 should not produce box top border, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("╰─────╯"),
        "NO_COLOR=1 should not produce box bottom border, got stderr: {stderr}"
    );
}

#[test]
fn colored_output_contains_bold_badge() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("╭─────╮"),
        "colored output should contain box top border ╭─────╮, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1m sgf \x1b[0m"),
        "colored output should contain bold sgf label on middle line, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("╰─────╯"),
        "colored output should contain box bottom border ╰─────╯, got stderr: {stderr}"
    );
}

#[test]
fn exit_0_uses_success_styling() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("loop complete"),
        "exit 0 should print 'loop complete' message, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1;32m"),
        "exit 0 should use green (success) styling, got stderr: {stderr}"
    );
}

#[test]
fn exit_1_uses_error_styling() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 1\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("ralph exited with error"),
        "exit 1 should print error message, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1;31m"),
        "exit 1 should use red (error) styling, got stderr: {stderr}"
    );
}

#[test]
fn exit_2_uses_warning_styling() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 2\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("iterations exhausted"),
        "exit 2 should print 'iterations exhausted' message, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1;33m"),
        "exit 2 should use yellow (warning) styling, got stderr: {stderr}"
    );
}

#[test]
fn no_color_exit_messages_use_plain_prefix() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let mock_dir = TempDir::new().unwrap();

    // Test exit 0 (success) with NO_COLOR
    let mock_ralph_0 =
        create_mock_script(mock_dir.path(), "mock_ralph_0.sh", "#!/bin/sh\nexit 0\n");

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph_0)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf: loop complete"),
        "NO_COLOR exit 0 should use 'sgf:' prefix with success message, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("\x1b["),
        "NO_COLOR exit 0 should have no ANSI codes, got stderr: {stderr}"
    );

    // Test exit 1 (error) with NO_COLOR
    let mock_ralph_1 =
        create_mock_script(mock_dir.path(), "mock_ralph_1.sh", "#!/bin/sh\nexit 1\n");

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph_1)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf: ralph exited with error"),
        "NO_COLOR exit 1 should use 'sgf:' prefix with error message, got stderr: {stderr}"
    );

    // Test exit 2 (warning) with NO_COLOR
    let mock_ralph_2 =
        create_mock_script(mock_dir.path(), "mock_ralph_2.sh", "#!/bin/sh\nexit 2\n");

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph_2)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path_with_cl)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sgf");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf: iterations exhausted"),
        "NO_COLOR exit 2 should use 'sgf:' prefix with warning message, got stderr: {stderr}"
    );
}

// ===========================================================================
// Unified command dispatch integration tests
// ===========================================================================

#[test]
fn install_runs_afk_with_one_iteration_via_mock_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[install]\nalias = \"i\"\nmode = \"afk\"\niterations = 1\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .arg("install")
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("-a"), "install should run in AFK mode");
    assert!(
        args.contains(".sgf/prompts/install.md"),
        "should pass install prompt path, got: {args}"
    );
    // iterations = 1 means "1" appears as positional arg before the prompt path
    assert!(args.contains(" 1 "), "should pass 1 iteration, got: {args}");
}

#[test]
fn alias_i_resolves_to_install() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[install]\nalias = \"i\"\nmode = \"afk\"\niterations = 1\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .arg("i")
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf i (alias for install) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(".sgf/prompts/install.md"),
        "alias 'i' should resolve to install prompt, got: {args}"
    );
}

#[test]
fn build_auth_uses_config_defaults() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nalias = \"b\"\nmode = \"interactive\"\niterations = 30\nauto_push = true\n",
    );
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build auth (interactive default) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Interactive mode invokes cl, not ralph
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--verbose"),
        "interactive mode should pass --verbose to agent"
    );
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass raw build prompt path, got: {args}"
    );
}

#[test]
fn build_dash_a_overrides_mode_to_afk() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    // Config says interactive, but CLI -a should override to AFK
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"interactive\"\niterations = 30\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build -a failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // -a should invoke ralph (AFK mode), not cl
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("-a"), "should pass -a to ralph");
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass build prompt path, got: {args}"
    );
}

#[test]
fn build_dash_a_dash_i_errors() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = sgf_cmd(tmp.path())
        .args(["build", "-a", "-i"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "sgf build -a -i should fail");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mutually exclusive"),
        "should report mutual exclusivity error, got: {stderr}"
    );
}

#[test]
fn build_dash_n_overrides_iterations() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"afk\"\niterations = 30\nauto_push = true\n",
    );

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "-n", "5"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build -n 5 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    // iterations=5 should override config's 30
    assert!(
        args.contains(" 5 "),
        "should pass 5 iterations (not 30), got: {args}"
    );
    assert!(
        !args.contains(" 30 "),
        "should NOT pass default 30 iterations, got: {args}"
    );
}

#[test]
fn unknown_command_errors_with_clear_message() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = sgf_cmd(tmp.path()).arg("nonexistent-cmd").output().unwrap();

    assert!(!output.status.success(), "sgf nonexistent-cmd should fail");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown command"),
        "should report unknown command, got: {stderr}"
    );
    assert!(
        stderr.contains("nonexistent-cmd"),
        "should name the unrecognized command, got: {stderr}"
    );
}

#[test]
fn custom_prompt_without_config_entry_uses_fallback_defaults() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Create a custom prompt file with no config.toml entry
    fs::write(
        tmp.path().join(".sgf/prompts/deploy.md"),
        "Deploy the project.",
    )
    .unwrap();
    git_add_commit(tmp.path(), "add custom deploy prompt");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Fallback defaults: interactive mode, 1 iteration
    // So it should invoke cl (interactive), not ralph
    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_path);

    let output = sgf_cmd(tmp.path())
        .arg("deploy")
        .env("PATH", &mock_path_with_cl)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf deploy (custom prompt, no config) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(".sgf/prompts/deploy.md"),
        "should pass custom deploy prompt path, got: {args}"
    );
    assert!(
        args.contains("--verbose"),
        "fallback interactive mode should pass --verbose, got: {args}"
    );
}

#[test]
fn config_toml_duplicate_alias_errors_at_parse_time() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Write config with duplicate alias "x" for both build and install
    write_config_toml(
        tmp.path(),
        "[build]\nalias = \"x\"\n\n[install]\nalias = \"x\"\n",
    );

    // Use alias "x" so resolve_command goes through config::load (which validates)
    let output = sgf_cmd(tmp.path())
        .arg("x")
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &mock_path)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "duplicate alias should cause failure"
    );
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate alias"),
        "should report duplicate alias error, got: {stderr}"
    );
}

// ===========================================================================
// End-to-end: sgf → ralph → cl → mock claude-wrapper-secret
// ===========================================================================

fn target_bin_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_BIN_EXE_sgf"));
    dir.pop();
    dir
}

#[test]
fn e2e_sgf_ralph_cl_context_files_and_spec_in_append_system_prompt() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_config_toml(
        tmp.path(),
        "[build]\nmode = \"afk\"\niterations = 1\nauto_push = false\n",
    );
    create_spec_and_commit(tmp.path(), "auth");

    // Create context files that cl should detect
    fs::create_dir_all(tmp.path().join(".sgf")).unwrap();
    fs::write(tmp.path().join(".sgf/MEMENTO.md"), "memento content").unwrap();
    fs::write(
        tmp.path().join(".sgf/BACKPRESSURE.md"),
        "backpressure content",
    )
    .unwrap();
    fs::write(tmp.path().join("specs/README.md"), "specs readme content").unwrap();
    git_add_commit(tmp.path(), "add context files");

    let bin_dir = target_bin_dir();
    let ralph_bin = bin_dir.join("ralph");
    assert!(
        ralph_bin.exists(),
        "ralph binary not found at {}; run `cargo build --workspace` first",
        ralph_bin.display()
    );
    assert!(
        bin_dir.join("cl").exists(),
        "cl binary not found at {}; run `cargo build --workspace` first",
        bin_dir.join("cl").display()
    );

    // Mock dir: claude-wrapper-secret (captures args) + pn (no-op)
    let mock_dir = TempDir::new().unwrap();
    let captured_args = mock_dir.path().join("captured_args.txt");

    create_mock_script(
        mock_dir.path(),
        "claude-wrapper-secret",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$@\" > \"{captured}\"\n",
                "echo '{{\"type\":\"result\",\"result\":\"done\"}}'\n",
                "touch \"$PWD/.ralph-complete\"\n",
            ),
            captured = captured_args.display(),
        ),
    );
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");

    // PATH: mock_dir (claude-wrapper-secret, pn) → bin_dir (cl) → system
    let path = format!(
        "{}:{}:{}",
        mock_dir.path().display(),
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default(),
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &ralph_bin)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("PATH", &path)
        .env("HOME", tmp.path().to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sgf build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let args = fs::read_to_string(&captured_args).expect("read captured args from mock agent");
    let lines: Vec<&str> = args.lines().collect();

    // cl should inject --append-system-prompt with MEMENTO, BACKPRESSURE, specs/README
    assert!(
        args.contains("--append-system-prompt"),
        "missing --append-system-prompt in captured args:\n{args}"
    );
    assert!(
        args.contains("MEMENTO.md"),
        "missing MEMENTO.md in captured args:\n{args}"
    );
    assert!(
        args.contains("BACKPRESSURE.md"),
        "missing BACKPRESSURE.md in captured args:\n{args}"
    );
    assert!(
        args.contains("specs/README.md"),
        "missing specs/README.md in captured args:\n{args}"
    );

    // ralph should inject --append-system-prompt with study @./specs/auth.md
    assert!(
        args.contains("specs/auth.md"),
        "missing specs/auth.md (from --spec) in captured args:\n{args}"
    );

    // At least 2 --append-system-prompt args: one from cl, one from ralph
    let asp_count = lines
        .iter()
        .filter(|l| **l == "--append-system-prompt")
        .count();
    assert!(
        asp_count >= 2,
        "expected >= 2 --append-system-prompt args, got {asp_count}:\n{args}"
    );

    // ralph should pass AFK-mode flags
    assert!(lines.contains(&"--verbose"), "missing --verbose:\n{args}");
    assert!(lines.contains(&"--print"), "missing --print:\n{args}");
    assert!(
        lines.contains(&"stream-json"),
        "missing stream-json output format:\n{args}"
    );
    assert!(
        lines.contains(&"--dangerously-skip-permissions"),
        "missing --dangerously-skip-permissions:\n{args}"
    );
}
