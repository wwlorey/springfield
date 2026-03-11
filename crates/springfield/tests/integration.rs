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
        ".sgf/prompts/issues.md",
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
        doc["sandbox"]["allowUnsandboxedCommands"], true,
        "sandbox.allowUnsandboxedCommands"
    );
    assert_eq!(
        doc["sandbox"]["network"]["allowLocalBinding"], true,
        "sandbox.network.allowLocalBinding"
    );

    let allow_write = doc["sandbox"]["filesystem"]["allowWrite"]
        .as_array()
        .expect("allowWrite should be an array");
    assert!(
        allow_write.contains(&serde_json::json!("~/.cargo")),
        "allowWrite should include ~/.cargo"
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
        doc1["sandbox"]["filesystem"]["allowWrite"]
            .as_array()
            .unwrap()
            .len(),
        doc2["sandbox"]["filesystem"]["allowWrite"]
            .as_array()
            .unwrap()
            .len(),
        "allowWrite duplicated on rerun"
    );
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
    "filesystem": { "allowWrite": ["~/.npm"] },
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

    // Custom array entries should be preserved
    let allow_write = doc["sandbox"]["filesystem"]["allowWrite"]
        .as_array()
        .unwrap();
    assert!(
        allow_write.contains(&serde_json::json!("~/.npm")),
        "custom allowWrite entry lost"
    );
    assert!(
        allow_write.contains(&serde_json::json!("~/.cargo")),
        "default allowWrite entry not added"
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
fn spec_runs_interactive_via_agent_cmd() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("AGENT_CMD", mock_agent.to_string_lossy().as_ref())
        .env("PROMPT_FILES", "")
        .env("PATH", &mock_path)
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
fn issues_log_runs_interactive_via_agent_cmd() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = sgf_cmd(tmp.path())
        .args(["issues", "log"])
        .env("AGENT_CMD", mock_agent.to_string_lossy().as_ref())
        .env("PROMPT_FILES", "")
        .env("PATH", &mock_path)
        .env_remove("CLAUDECODE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf issues log failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("--verbose"), "should pass --verbose to agent");
    assert!(args.contains("@"), "should pass prompt via @ prefix");
    assert!(
        args.contains(".sgf/prompts/issues.md"),
        "should pass raw issues prompt path"
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Not yet implemented"));
}

#[test]
fn help_flag() {
    let output = Command::new(sgf_bin()).arg("--help").output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"));
    assert!(stdout.contains("build"));
    assert!(stdout.contains("verify"));
    assert!(stdout.contains("test-plan"));
    assert!(stdout.contains("spec"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("logs"));
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
        ".sgf/prompts/issues.md",
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

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock agent that creates a new commit
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
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

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("AGENT_CMD", mock_agent.to_string_lossy().as_ref())
        .env("PROMPT_FILES", "")
        .env("PATH", &mock_path)
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
        stderr.contains("new commits detected"),
        "should log push detection: {stderr}"
    );
    assert!(
        stderr.contains("push ok"),
        "should log push success: {stderr}"
    );
}

#[test]
fn spec_no_push_suppresses_auto_push() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock agent that creates a new commit
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
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

    let output = sgf_cmd(tmp.path())
        .args(["spec", "--no-push"])
        .env("AGENT_CMD", mock_agent.to_string_lossy().as_ref())
        .env("PROMPT_FILES", "")
        .env("PATH", &mock_path)
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

    // Mock agent that does NOT create any commits
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 0\n");

    let output = sgf_cmd(tmp.path())
        .arg("spec")
        .env("AGENT_CMD", mock_agent.to_string_lossy().as_ref())
        .env("PROMPT_FILES", "")
        .env("PATH", &mock_path)
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
fn non_afk_mode_child_inherits_session() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();
    let mock_dir = TempDir::new().unwrap();
    let sid_file = mock_dir.path().join("sid_info.txt");

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

    // Non-AFK: no -a flag
    let output = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("AGENT_CMD", "true")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf build (non-afk) failed: {}",
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

    // In non-AFK mode, no setsid() — child inherits parent's process group.
    assert_ne!(
        pgid, pid,
        "non-AFK child PGID should NOT equal its PID (no setsid), info: {info}"
    );
}

#[test]
fn non_afk_stdin_eof_does_not_trigger_shutdown() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that sleeps then exits 0
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_sleep.sh",
        "#!/bin/bash\ntrap '' INT\nsleep 2\nexit 0\n",
    );

    let ready_file = mock_dir.path().join("sgf_ready");

    // Non-AFK mode: monitor_stdin should be false, so closing stdin
    // should NOT cause shutdown.
    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("AGENT_CMD", "true")
        .env("SGF_READY_FILE", &ready_file)
        .env("PATH", &mock_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let stdin = child.stdin.take().expect("open stdin");

    wait_for_ready(&ready_file);
    drop(stdin); // Close stdin — would trigger Ctrl+D detection if monitor_stdin were true

    // Wait for the process to finish naturally
    let output = child.wait_with_output().expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(0),
        "non-AFK mode should NOT shutdown on stdin EOF, should exit 0 from mock ralph completing"
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
fn non_afk_does_not_pass_afk_flag_to_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
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
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("AGENT_CMD", "true")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(output.status.success());

    let args = fs::read_to_string(&args_file).unwrap();
    let argv: Vec<&str> = args.trim().split_whitespace().collect();
    assert!(
        !argv.contains(&"-a"),
        "non-AFK should NOT pass -a flag to ralph, got: {args}"
    );
}

#[test]
fn non_afk_does_not_create_log_file() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
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
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("AGENT_CMD", "true")
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(output.status.success());

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        !args.contains("--log-file"),
        "non-AFK should NOT pass --log-file to ralph, got: {args}"
    );
}

#[test]
fn non_afk_stdin_passthrough_to_mock_ralph() {
    use std::io::Write;

    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock ralph that reads a line from stdin and echoes it to stdout
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph_echo.sh",
        "#!/bin/bash\nread -r line\necho \"ECHO:$line\"\nexit 0\n",
    );

    let mut child = sgf_cmd(tmp.path())
        .args(["build", "auth"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("SGF_SKIP_PREFLIGHT", "1")
        .env("AGENT_CMD", "true")
        .env("PATH", &mock_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sgf");

    let mut stdin = child.stdin.take().expect("open stdin");

    std::thread::sleep(Duration::from_millis(300));
    stdin.write_all(b"hello_from_test\n").expect("write stdin");
    stdin.flush().expect("flush stdin");
    drop(stdin);

    let output = child.wait_with_output().expect("wait for sgf");

    assert!(
        output.status.success(),
        "sgf should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ECHO:hello_from_test"),
        "non-AFK ralph should receive stdin passthrough and echo it, got stdout: {stdout}"
    );
}
