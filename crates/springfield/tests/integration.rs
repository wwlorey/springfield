use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use shutdown::{ChildGuard, ProcessSemaphore};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const PROMPT_NAMES: &[&str] = &[
    "build",
    "spec",
    "verify",
    "test-plan",
    "test",
    "issues-log",
    "doc",
    "install",
];

const CONFIG_TOML: &str = "\
[install]
alias = \"i\"
mode = \"afk\"
iterations = 1
auto_push = true

[spec]
alias = \"s\"
mode = \"interactive\"
iterations = 1
auto_push = false

[build]
alias = \"b\"
mode = \"interactive\"
iterations = 30
auto_push = true

[verify]
alias = \"v\"
mode = \"afk\"
iterations = 30
auto_push = true

[test-plan]
mode = \"afk\"
iterations = 30
auto_push = true

[test]
mode = \"afk\"
iterations = 30
auto_push = true

[issues-log]
mode = \"interactive\"
iterations = 1
auto_push = false

[doc]
mode = \"interactive\"
iterations = 1
auto_push = false
";

static FAKE_HOME: LazyLock<TempDir> = LazyLock::new(|| {
    let home = TempDir::new().unwrap();
    let prompts = home.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts).unwrap();
    for name in PROMPT_NAMES {
        fs::write(
            prompts.join(format!("{name}.md")),
            format!("{name} prompt\n"),
        )
        .unwrap();
    }
    fs::write(prompts.join("config.toml"), CONFIG_TOML).unwrap();
    home
});

fn fake_home() -> &'static Path {
    FAKE_HOME.path()
}

fn sgf_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sgf"))
}

static SGF_PERMITS: LazyLock<ProcessSemaphore> =
    LazyLock::new(|| ProcessSemaphore::from_env("SGF_TEST_MAX_CONCURRENT", 8));

static MOCK_BINS: LazyLock<(TempDir, String)> = LazyLock::new(|| {
    let mock_dir = TempDir::new().unwrap();
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (mock_dir, path)
});

fn mock_bin_path() -> &'static str {
    &MOCK_BINS.1
}

fn run_sgf(cmd: &mut Command) -> std::process::Output {
    run_sgf_timeout(cmd, Duration::from_secs(30))
}

fn run_sgf_timeout(cmd: &mut Command, timeout: Duration) -> std::process::Output {
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn sgf")
        .wait_with_output_timeout(timeout)
        .expect("failed to run sgf")
}

fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", mock_bin_path());
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd.env("SGF_TEST_NO_SETSID", "1");
    cmd.env("SGF_TEST_ITER_DELAY_MS", "0");
    cmd.env_remove("PN_DAEMON");
    cmd.env_remove("PN_DAEMON_HOST");
    cmd.env_remove("FM_DAEMON");
    cmd.env_remove("FM_DAEMON_HOST");
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

/// Run `sgf init` in the given dir, then commit all generated files.
fn sgf_init_and_commit(dir: &Path) {
    let output = run_sgf(sgf_cmd(dir).arg("init"));
    assert!(output.status.success(), "sgf init failed");
    git_add_commit(dir, "sgf init");
}

fn write_cursus_toml(dir: &Path, name: &str, content: &str) {
    let cursus_dir = dir.join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(cursus_dir.join(format!("{name}.toml")), content).unwrap();
    git_add_commit(dir, &format!("add {name} cursus toml"));
}

fn setup_default_cursus(dir: &Path) {
    let cursus_dir = dir.join(".sgf/cursus");
    let prompts_dir = dir.join(".sgf/prompts");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::create_dir_all(&prompts_dir).unwrap();

    let commands: &[(&str, &str, &str, &str, u32)] = &[
        ("build", "b", "Build", "afk", 30),
        ("spec", "s", "Spec", "interactive", 1),
        ("verify", "v", "Verify", "afk", 30),
        ("test-plan", "", "Test plan", "afk", 30),
        ("test", "", "Test", "afk", 30),
        ("issues-log", "", "Issues log", "interactive", 1),
        ("doc", "", "Doc", "interactive", 1),
        ("install", "i", "Install", "afk", 1),
    ];

    for &(name, alias, desc, mode, iterations) in commands {
        let alias_line = if alias.is_empty() {
            String::new()
        } else {
            format!("alias = \"{alias}\"\n")
        };
        let auto_push = mode == "afk";
        let toml = format!(
            "description = \"{desc}\"\n\
             {alias_line}\
             auto_push = {auto_push}\n\
             \n\
             [[iter]]\n\
             name = \"{name}\"\n\
             prompt = \"{name}.md\"\n\
             mode = \"{mode}\"\n\
             iterations = {iterations}\n"
        );
        fs::write(cursus_dir.join(format!("{name}.toml")), toml).unwrap();
        fs::write(
            prompts_dir.join(format!("{name}.md")),
            format!("{desc} prompt\n"),
        )
        .unwrap();
    }

    git_add_commit(dir, "add default cursus tomls and prompts");
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
    let output = run_sgf(sgf_cmd(tmp.path()).arg("init"));
    assert!(output.status.success());

    let expected_dirs = [".pensa", ".forma", ".sgf", ".sgf/logs", ".sgf/run"];
    for dir in expected_dirs {
        assert!(tmp.path().join(dir).is_dir(), "directory missing: {dir}");
    }
}

#[test]
fn init_creates_all_files() {
    let tmp = setup_test_dir();
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    let expected_files = [
        ".claude/settings.json",
        ".pre-commit-config.yaml",
        ".gitignore",
        "AGENTS.md",
    ];
    for f in expected_files {
        assert!(tmp.path().join(f).is_file(), "file missing: {f}");
    }

    // Prompts should NOT be scaffolded locally
    assert!(
        !tmp.path().join(".sgf/prompts").exists(),
        ".sgf/prompts/ should NOT be created by init"
    );

    // BACKPRESSURE.md should NOT be scaffolded
    assert!(
        !tmp.path().join("BACKPRESSURE.md").exists(),
        "BACKPRESSURE.md should NOT be created by init"
    );

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
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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
    assert!(gitignore.contains(".pensa/db.sqlite"));
    assert!(gitignore.contains(".sgf/logs/"));
}

#[test]
fn init_idempotent() {
    let tmp = setup_test_dir();

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    // Snapshot config files
    let gitignore1 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    let settings1 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(content.contains("my-secret.key"), "custom entry lost");
    assert!(
        content.contains(".pensa/db.sqlite"),
        "db.sqlite should be in gitignore"
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

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    let first = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc1: serde_json::Value = serde_json::from_str(&first).unwrap();

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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

    run_sgf(sgf_cmd(tmp.path()).arg("init"));

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
    assert!(err.to_string().contains("prompt not found"));
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
// Loop orchestration (mocked agent via CLI)
// ===========================================================================

#[test]
fn build_invokes_agent_with_correct_flags() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 30\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    // Mock agent that logs all args
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--print"),
        "should pass --print in afk mode, got: {args}"
    );
    assert!(
        args.contains("--output-format stream-json"),
        "should pass --output-format stream-json in afk mode, got: {args}"
    );
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions, got: {args}"
    );
    assert!(
        args.contains("--session-id"),
        "should pass --session-id, got: {args}"
    );
}

#[test]
fn build_creates_and_cleans_pid_file() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Mock agent that checks for PID file existence during execution
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("pid_state.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "find \"${{PWD}}/.sgf/run\" -name '*.pid' > \"{}\" 2>&1\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "exit 0\n",
            ),
            state_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // PID file should have existed while agent was running
    let pid_state = fs::read_to_string(&state_file).unwrap();
    assert!(
        pid_state.contains(".pid"),
        "PID file should exist during agent execution"
    );

    // PID file should be cleaned up after agent exits
    let run_dir = tmp.path().join(".sgf/run");
    let mut remaining_pids = Vec::new();
    fn find_pids(dir: &Path, pids: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_pids(&path, pids);
                } else if path.extension().is_some_and(|ext| ext == "pid") {
                    pids.push(path);
                }
            }
        }
    }
    find_pids(&run_dir, &mut remaining_pids);
    assert!(remaining_pids.is_empty(), "PID file not cleaned up");
}

#[test]
fn afk_passes_log_file_to_agent() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/agent_args.txt\"\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(output.status.success());

    let args_content = fs::read_to_string(mock_dir.path().join("agent_args.txt")).unwrap();
    // iter_runner manages log files internally; verify AFK mode markers instead
    assert!(
        args_content.contains("--print"),
        "should pass --print in afk mode, got: {args_content}"
    );
    // Verify a log file was actually created in .sgf/logs/
    let logs_dir = tmp.path().join(".sgf/logs");
    assert!(
        logs_dir.exists(),
        ".sgf/logs/ directory should exist after AFK run"
    );
}

#[test]
fn spec_runs_interactive_via_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("spec")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );
    assert!(
        output.status.success(),
        "sgf spec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions to agent"
    );
    assert!(
        args.contains(".sgf/prompts/spec.md"),
        "should pass raw spec prompt path"
    );
}

#[test]
fn issues_log_runs_interactive_via_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["issues-log"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );
    assert!(
        output.status.success(),
        "sgf issues-log failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions to agent"
    );
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Create a stale run directory with dead PID
    let stale_run_dir = tmp.path().join(".sgf/run/stale-build-20260101T000000");
    fs::create_dir_all(&stale_run_dir).unwrap();
    fs::write(
        stale_run_dir.join("stale-build-20260101T000000.pid"),
        "4000000",
    )
    .unwrap();
    let stale_meta = serde_json::json!({
        "run_id": "stale-build-20260101T000000",
        "cursus": "build",
        "status": "running",
        "current_iter": "build",
        "current_iter_index": 0,
        "iters_completed": [],
        "spec": null,
        "mode_override": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    });
    fs::write(
        stale_run_dir.join("meta.json"),
        serde_json::to_string_pretty(&stale_meta).unwrap(),
    )
    .unwrap();

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Stale run should be marked as interrupted
    let stale_meta_content = fs::read_to_string(stale_run_dir.join("meta.json")).unwrap();
    let stale_meta_json: serde_json::Value = serde_json::from_str(&stale_meta_content).unwrap();
    assert_eq!(
        stale_meta_json["status"].as_str().unwrap(),
        "interrupted",
        "stale run should be marked as interrupted"
    );

    // Stale PID file should be removed
    assert!(
        !stale_run_dir
            .join("stale-build-20260101T000000.pid")
            .exists(),
        "stale PID file should be removed"
    );
}

#[test]
fn recovery_skips_when_live_pid() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Create a live run directory with our own PID
    let live_run_dir = tmp.path().join(".sgf/run/live-build-20260101T000000");
    fs::create_dir_all(&live_run_dir).unwrap();
    fs::write(
        live_run_dir.join("live-build-20260101T000000.pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    let live_meta = serde_json::json!({
        "run_id": "live-build-20260101T000000",
        "cursus": "build",
        "status": "running",
        "current_iter": "build",
        "current_iter_index": 0,
        "iters_completed": [],
        "spec": null,
        "mode_override": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    });
    fs::write(
        live_run_dir.join("meta.json"),
        serde_json::to_string_pretty(&live_meta).unwrap(),
    )
    .unwrap();

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Live run should NOT be marked as interrupted
    let live_meta_content = fs::read_to_string(live_run_dir.join("meta.json")).unwrap();
    let live_meta_json: serde_json::Value = serde_json::from_str(&live_meta_content).unwrap();
    assert_eq!(
        live_meta_json["status"].as_str().unwrap(),
        "running",
        "live run should not be marked as interrupted"
    );

    // Live PID file should still exist
    assert!(
        live_run_dir.join("live-build-20260101T000000.pid").exists(),
        "live PID file should still exist"
    );
}

// ===========================================================================
// Utility commands
// ===========================================================================

#[test]
fn logs_exits_1_for_missing() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).args(["logs", "nonexistent"]));

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "should have error message");
}

#[test]
fn help_flag() {
    let tmp = setup_test_dir();
    let output = run_sgf(sgf_cmd(tmp.path()).arg("--help"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"), "help should list init built-in");
    assert!(stdout.contains("logs"), "help should list logs built-in");
}

// ===========================================================================
// ===========================================================================
// Spec-parity integration tests (implementation plan)
// ===========================================================================

#[test]
fn init_no_specs_directory() {
    let tmp = setup_test_dir();
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    assert!(
        !tmp.path().join("specs").exists(),
        "specs/ should not be scaffolded (forma manages specs now)"
    );
}

#[test]
fn prompts_not_scaffolded_by_init() {
    let tmp = setup_test_dir();
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    assert!(
        !tmp.path().join(".sgf/prompts").exists(),
        "init should NOT create .sgf/prompts/"
    );
    assert!(
        !tmp.path().join("BACKPRESSURE.md").exists(),
        "init should NOT create BACKPRESSURE.md"
    );
}

// ===========================================================================
// Frontend scaffolding integration tests
// ===========================================================================

/// Build a `sgf_cmd` with a custom mock PATH (instead of the shared MOCK_BINS).
fn sgf_cmd_with_path(dir: &Path, path: &str) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", path);
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd.env("SGF_TEST_NO_SETSID", "1");
    cmd.env("SGF_TEST_ITER_DELAY_MS", "0");
    cmd.env_remove("PN_DAEMON");
    cmd.env_remove("PN_DAEMON_HOST");
    cmd.env_remove("FM_DAEMON");
    cmd.env_remove("FM_DAEMON_HOST");
    cmd
}

/// Create a mock `pnpm` that creates frontend files when invoked with `create`.
fn mock_pnpm_that_creates_files(dir: &Path) -> PathBuf {
    create_mock_script(
        dir,
        "pnpm",
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "9.0.0"
    exit 0
fi
if [ "$1" = "create" ]; then
    echo '{"name":"test","version":"0.0.0"}' > "$PWD/package.json"
    echo 'import { defineConfig } from "vite";' > "$PWD/vite.config.ts"
    echo 'export default {};' > "$PWD/eslint.config.js"
    exit 0
fi
exit 0
"#,
    )
}

#[test]
fn init_frontend_scaffolding_runs_by_default() {
    let tmp = setup_test_dir();
    let mock_dir = TempDir::new().unwrap();
    mock_pnpm_that_creates_files(mock_dir.path());
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(output.status.success(), "sgf init failed");

    assert!(
        tmp.path().join("package.json").exists(),
        "package.json should be created by frontend scaffolding"
    );
    assert!(
        tmp.path().join("vite.config.ts").exists(),
        "vite.config.ts should be created by frontend scaffolding"
    );
    assert!(
        tmp.path().join("eslint.config.js").exists(),
        "eslint.config.js should be created by frontend scaffolding"
    );
}

#[test]
fn init_frontend_scaffolding_skipped_with_no_fe() {
    let tmp = setup_test_dir();
    let output = run_sgf(sgf_cmd(tmp.path()).args(["init", "--no-fe"]));
    assert!(output.status.success(), "sgf init --no-fe failed");

    assert!(
        !tmp.path().join("package.json").exists(),
        "package.json should NOT be created with --no-fe"
    );
    assert!(
        !tmp.path().join("vite.config.ts").exists(),
        "vite.config.ts should NOT be created with --no-fe"
    );
}

#[test]
fn init_frontend_skipped_when_package_json_exists() {
    let tmp = setup_test_dir();
    let mock_dir = TempDir::new().unwrap();
    mock_pnpm_that_creates_files(mock_dir.path());
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let original_content = r#"{"name":"my-project"}"#;
    fs::write(tmp.path().join("package.json"), original_content).unwrap();

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(output.status.success(), "sgf init failed");

    let content = fs::read_to_string(tmp.path().join("package.json")).unwrap();
    assert_eq!(
        content, original_content,
        "existing package.json should be preserved"
    );
}

#[test]
fn init_frontend_skipped_when_vite_config_exists() {
    let tmp = setup_test_dir();
    let mock_dir = TempDir::new().unwrap();
    mock_pnpm_that_creates_files(mock_dir.path());
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    fs::write(tmp.path().join("vite.config.ts"), "export default {}").unwrap();

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(output.status.success(), "sgf init failed");

    assert!(
        !tmp.path().join("package.json").exists(),
        "package.json should NOT be created when vite.config.ts exists"
    );
    let content = fs::read_to_string(tmp.path().join("vite.config.ts")).unwrap();
    assert_eq!(
        content, "export default {}",
        "existing vite.config.ts should be preserved"
    );
}

#[test]
fn init_frontend_pnpm_not_found_error() {
    let tmp = setup_test_dir();
    let mock_dir = TempDir::new().unwrap();
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    create_mock_script(
        mock_dir.path(),
        "git",
        "#!/bin/sh\nexec /usr/bin/git \"$@\"\n",
    );
    let path = mock_dir.path().display().to_string();

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(
        !output.status.success(),
        "sgf init should fail when pnpm is missing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pnpm not found"),
        "error should mention pnpm not found, got: {stderr}"
    );
}

#[test]
fn init_frontend_create_vite_failure_warning() {
    let tmp = setup_test_dir();
    let mock_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_dir.path(),
        "pnpm",
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "9.0.0"
    exit 0
fi
exit 1
"#,
    );
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(
        output.status.success(),
        "sgf init should still succeed when create-vite fails"
    );

    assert!(
        tmp.path().join(".sgf").is_dir(),
        "SGF infrastructure should still be scaffolded after create-vite failure"
    );
}

#[test]
fn init_force_does_not_rerun_frontend_scaffolding() {
    let tmp = setup_test_dir();

    static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
    let count = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    let marker_name = format!("pnpm_invoked_{count}");

    let mock_dir = TempDir::new().unwrap();
    let marker = mock_dir.path().join(&marker_name);
    create_mock_script(
        mock_dir.path(),
        "pnpm",
        &format!(
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "9.0.0"
    exit 0
fi
if [ "$1" = "create" ]; then
    echo '{{}}' > "$PWD/package.json"
    echo 'export default {{}};' > "$PWD/vite.config.ts"
    echo "invoked" >> "{}"
    exit 0
fi
exit 0
"#,
            marker.display()
        ),
    );
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).arg("init"));
    assert!(output.status.success(), "first init failed");
    assert!(
        marker.exists(),
        "pnpm create should be called on first init"
    );

    fs::remove_file(&marker).unwrap();
    git_add_commit(tmp.path(), "after first init");

    let output = run_sgf(sgf_cmd_with_path(tmp.path(), &path).args(["init", "--force"]));
    assert!(output.status.success(), "init --force failed");
    assert!(
        !marker.exists(),
        "pnpm create should NOT be re-invoked on --force (frontend files already exist)"
    );
}

#[test]
fn init_sandbox_domains_include_npm_registries() {
    let tmp = setup_test_dir();
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&content).unwrap();
    let domains = doc["sandbox"]["network"]["allowedDomains"]
        .as_array()
        .expect("allowedDomains should be an array");

    assert!(
        domains.contains(&serde_json::json!("registry.npmjs.org")),
        "allowedDomains should include registry.npmjs.org"
    );
    assert!(
        domains.contains(&serde_json::json!("registry.yarnpkg.com")),
        "allowedDomains should include registry.yarnpkg.com"
    );
}

#[test]
fn init_gitignore_no_sveltekit() {
    let tmp = setup_test_dir();
    run_sgf(sgf_cmd(tmp.path()).arg("init"));

    let gitignore = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(
        !gitignore.contains(".svelte-kit"),
        ".svelte-kit/ should NOT be in .gitignore"
    );
}

#[test]
fn end_to_end_build_passes_raw_path_and_spec() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    // Create spec file (validate requires it)
    fs::create_dir_all(tmp.path().join("specs")).unwrap();
    fs::write(tmp.path().join("specs/auth.md"), "# Auth spec").unwrap();
    git_add_commit(tmp.path(), "add spec");

    // Mock agent that logs args and env
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let env_file = mock_dir.path().join("agent_env.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\necho \"SGF_SPEC=$SGF_SPEC\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display(),
            env_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Agent should receive raw prompt path (not assembled)
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
        !args.contains("--spec"),
        "should NOT pass --spec to agent, got: {args}"
    );
}

// ===========================================================================
// End-to-end: sgf init does not scaffold prompts or backpressure
// ===========================================================================

#[test]
fn init_does_not_scaffold_context_files() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    assert!(
        !tmp.path().join("BACKPRESSURE.md").exists(),
        "BACKPRESSURE.md should NOT be scaffolded at root"
    );
    assert!(
        !tmp.path().join(".sgf/BACKPRESSURE.md").exists(),
        ".sgf/BACKPRESSURE.md should NOT be scaffolded"
    );
    assert!(
        !tmp.path().join(".sgf/MEMENTO.md").exists(),
        ".sgf/MEMENTO.md should NOT be scaffolded"
    );
    assert!(
        !tmp.path().join(".sgf/prompts").exists(),
        ".sgf/prompts/ should NOT be created"
    );
    assert!(
        !tmp.path().join("specs").exists(),
        "specs/ should NOT be created"
    );

    let settings = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&settings).unwrap();
    let deny = doc["permissions"]["deny"].as_array().unwrap();
    assert!(
        deny.contains(&serde_json::Value::String("Edit .sgf/**".to_string())),
        "deny rules should protect .sgf/ contents"
    );
}

// ===========================================================================
// End-to-end: sgf build without spec argument
// ===========================================================================

#[test]
fn build_without_spec_omits_spec_flag_and_env() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let env_file = mock_dir.path().join("agent_env.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\necho \"SGF_SPEC=${{SGF_SPEC:-}}\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display(),
            env_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

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
fn build_with_spec_does_not_pass_spec_to_agent() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display(),
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build with spec should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        !args.contains("--spec"),
        "should NOT pass --spec to agent, got: {args}"
    );
}

// ---------------------------------------------------------------------------
// Spec validation (CLI-level)
// ---------------------------------------------------------------------------

#[test]
fn build_nonexistent_spec_fails_with_error() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).args(["build", "nonexistent", "-a"]));

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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

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
    let out = ChildGuard::spawn(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(bare)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn git")
    .wait_with_output_timeout(Duration::from_secs(30))
    .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn spec_auto_pushes_after_session_with_new_commits() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "spec",
        concat!(
            "description = \"Spec\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"spec\"\n",
            "prompt = \"spec.md\"\n",
            "mode = \"interactive\"\n",
        ),
    );

    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock cl that creates a new commit and signals completion
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
                "touch \"{root}/.iter-complete\"\n",
                "echo '{{\"type\":\"result\",\"result\":\"Done\",\"session_id\":\"sess-push\"}}'\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("spec")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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
    setup_default_cursus(tmp.path());

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["spec", "--no-push"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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
    setup_default_cursus(tmp.path());

    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    // Mock cl that does NOT create any commits
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("spec")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "doc",
        concat!(
            "description = \"Doc\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"doc\"\n",
            "prompt = \"doc.md\"\n",
            "mode = \"interactive\"\n",
        ),
    );

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
                "touch \"{root}/.iter-complete\"\n",
                "echo '{{\"type\":\"result\",\"result\":\"Done\",\"session_id\":\"sess-push\"}}'\n",
                "exit 0\n",
            ),
            root = tmp.path().display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("doc")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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
    setup_default_cursus(tmp.path());

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("doc")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );
    assert!(
        output.status.success(),
        "sgf doc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions to agent"
    );
    assert!(
        args.contains(".sgf/prompts/doc.md"),
        "should pass raw doc prompt path, got: {args}"
    );
}

#[test]
fn doc_no_push_suppresses_auto_push() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["doc", "--no-push"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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
    setup_default_cursus(tmp.path());

    let bare = setup_bare_remote(tmp.path());

    let head_before = bare_remote_head(bare.path());

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("doc")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

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

fn create_slow_mock_agent(dir: &Path) -> PathBuf {
    create_mock_script(
        dir,
        "mock_agent_slow.sh",
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_quick.sh",
        "#!/bin/bash\ntrap '' INT\nsleep 3\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );
    let ready_file = mock_dir.path().join("sgf_ready");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send single SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(0),
        "should exit 0 (agent completed) after single Ctrl+C timeout, not 130"
    );
}

#[test]
fn sigterm_exits_immediately() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).expect("send SIGTERM");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Press Ctrl-C again to exit"),
        "should show double-press prompt on stderr, got:\n{stderr}"
    );
}

#[test]
fn double_ctrl_c_kills_entire_process_tree() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    // Use iterations=1 to avoid iter_runner looping many times before kill
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let grandchild_pid_file = mock_dir.path().join("grandchild.pid");
    let ready_file = mock_dir.path().join("sgf_ready");

    // Mock agent that spawns a long-running grandchild, writes the grandchild's
    // PID to a file, then sleeps. The grandchild sleeps forever — if it's still
    // alive after sgf exits, the process tree was NOT fully killed.
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_tree.sh",
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

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .env_remove("SGF_TEST_NO_SETSID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

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

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

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

#[test]
fn panic_does_not_leak_child_tree() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent_pid_file = mock_dir.path().join("mock_agent.pid");
    let ready_file = mock_dir.path().join("sgf_ready");

    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_pid.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "trap '' INT TERM\n",
                "echo $$ > \"{}\"\n",
                "for i in $(seq 1 500); do sleep 0.1; done\n",
                "exit 2\n",
            ),
            mock_agent_pid_file.display()
        ),
    );

    let sgf_pid;
    let mock_agent_pid: i32;

    {
        let guard = ChildGuard::spawn(
            sgf_cmd(tmp.path())
                .args(["build", "auth", "-a"])
                .env("SGF_AGENT_COMMAND", &mock_agent)
                .env("SGF_READY_FILE", &ready_file)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .expect("spawn sgf");

        sgf_pid = guard.id();

        wait_for_ready(&ready_file);

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !mock_agent_pid_file.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "mock agent PID file was not created within 5s"
            );
            std::thread::sleep(Duration::from_millis(25));
        }

        mock_agent_pid = fs::read_to_string(&mock_agent_pid_file)
            .expect("read mock agent PID file")
            .trim()
            .parse()
            .expect("parse mock agent PID");

        assert_eq!(
            unsafe { libc::kill(mock_agent_pid, 0) },
            0,
            "mock agent (pid {mock_agent_pid}) should be alive before panic"
        );

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = guard;
            panic!("intentional panic to test cleanup");
        }));
        assert!(result.is_err());
    }

    let wait_dead = |pid: u32| -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };

    assert!(
        wait_dead(sgf_pid),
        "sgf (pid {sgf_pid}) should be dead after guard dropped during panic"
    );
    assert!(
        wait_dead(mock_agent_pid as u32),
        "mock agent (pid {mock_agent_pid}) should be dead after guard dropped during panic"
    );
}

// ===========================================================================
// Mode-dependent shutdown behavior
// ===========================================================================

#[test]
fn afk_mode_child_gets_new_session() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let sid_file = mock_dir.path().join("sid_info.txt");

    // Mock agent that checks if it became a session leader.
    // setsid() creates a new session AND new process group where PGID == PID.
    // We get bash's own PID ($$) and its PGID via python os.getpgid(parent).
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_sid.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "MY_PID=$$\n",
                "MY_PGID=$(python3 -c \"import os; print(os.getpgid($MY_PID))\")\n",
                "echo \"pid=$MY_PID pgid=$MY_PGID\" > \"{}\"\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "exit 0\n",
            ),
            sid_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env_remove("SGF_TEST_NO_SETSID"),
    );
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

// default_build_without_config_runs_interactive removed: build is now defined
// as afk in the default cursus TOML, so the legacy fallback no longer applies.

#[test]
fn interactive_mode_stdin_eof_does_not_trigger_shutdown() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    // Mock cl that sleeps then exits 0
    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        "#!/bin/bash\ntrap '' INT\nsleep 2\nexit 0\n",
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    // -i overrides to interactive mode. monitor_stdin is false,
    // so closing stdin should NOT cause shutdown.
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-i"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let stdin = guard.child_mut().stdin.take().expect("open stdin");

    std::thread::sleep(Duration::from_millis(500));
    drop(stdin); // Close stdin — should NOT trigger shutdown in interactive mode

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(0),
        "interactive mode should NOT shutdown on stdin EOF"
    );
}

#[test]
fn config_afk_mode_invokes_agent_with_afk_flag() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 30\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl),
    );
    assert!(
        output.status.success(),
        "sgf build (afk via config) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    // iter_runner uses --print for AFK mode instead of passing -a to agent
    assert!(
        args.contains("--print"),
        "AFK mode from config should invoke agent with --print (afk marker), got: {args}"
    );
}

#[test]
fn interactive_override_does_not_invoke_agent() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    // -i overrides config mode=afk to interactive
    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-i"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );
    assert!(
        output.status.success(),
        "sgf build -i failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "-i should force interactive mode using cl"
    );
}

#[test]
fn alias_resolves_to_prompt() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
        ),
    );

    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["b", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("cursus complete"),
        "exit 0 should print 'cursus complete' message, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1;32m"),
        "exit 0 should use green (success) styling, got stderr: {stderr}"
    );
}

#[test]
fn exit_1_uses_error_styling() {
    // In the cursus runner, agent exiting with code 1 without touching
    // .iter-complete is treated as "exhausted" (stalled), exit code 2.
    // This test verifies stall styling for an agent error scenario.
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    // Override build cursus with iterations=1 to avoid timeout from iter_runner loop
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 1\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Cursus STALLED"),
        "agent exit 1 without sentinel should stall, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[33m"),
        "stalled should use yellow (warning) styling, got stderr: {stderr}"
    );
}

#[test]
fn exit_2_uses_warning_styling() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    // Override build cursus with iterations=1 to avoid timeout from iter_runner loop
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 2\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Iterations exhausted"),
        "exit 2 should print 'Iterations exhausted' in stall banner, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[33m"),
        "stalled should use yellow (warning) styling, got stderr: {stderr}"
    );
}

#[test]
fn no_color_exit_messages_use_plain_prefix() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    // Override build cursus with iterations=1 to avoid timeout from iter_runner loop
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let mock_dir = TempDir::new().unwrap();

    // Test exit 0 (success) with NO_COLOR
    let mock_agent_0 = create_mock_script(
        mock_dir.path(),
        "mock_agent_0.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent_0)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf: cursus complete"),
        "NO_COLOR exit 0 should use 'sgf:' prefix with success message, got stderr: {stderr}"
    );
    assert!(
        !stderr.contains("\x1b["),
        "NO_COLOR exit 0 should have no ANSI codes, got stderr: {stderr}"
    );

    // Test exit 1 (stalled, no sentinel) with NO_COLOR
    let mock_agent_1 =
        create_mock_script(mock_dir.path(), "mock_agent_1.sh", "#!/bin/sh\nexit 1\n");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent_1)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Cursus STALLED"),
        "NO_COLOR exit 1 without sentinel should stall, got stderr: {stderr}"
    );

    // Test exit 2 (warning) with NO_COLOR
    let mock_agent_2 =
        create_mock_script(mock_dir.path(), "mock_agent_2.sh", "#!/bin/sh\nexit 2\n");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent_2)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("Iterations exhausted"),
        "NO_COLOR exit 2 should show 'Iterations exhausted' in stall banner, got stderr: {stderr}"
    );
}

// ===========================================================================
// Unified command dispatch integration tests
// ===========================================================================

#[test]
fn install_runs_afk_with_one_iteration_via_mock_agent() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "install",
        concat!(
            "description = \"Install\"\n",
            "alias = \"i\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"install\"\n",
            "prompt = \"install.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("install")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--print"),
        "install should run in AFK mode (--print marker), got: {args}"
    );
    assert!(
        args.contains(".sgf/prompts/install.md"),
        "should pass install prompt path, got: {args}"
    );
}

#[test]
fn alias_i_resolves_to_install() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "install",
        concat!(
            "description = \"Install\"\n",
            "alias = \"i\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"install\"\n",
            "prompt = \"install.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("i")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

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
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"interactive\"\n",
            "iterations = 1\n",
        ),
    );
    create_spec_and_commit(tmp.path(), "auth");

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
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth"])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

    assert!(
        output.status.success(),
        "sgf build auth (interactive default) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Interactive mode invokes cl, not agent
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should invoke cl with --dangerously-skip-permissions"
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
    setup_default_cursus(tmp.path());
    // Config says interactive, but CLI -a should override to AFK
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"interactive\"\n",
            "iterations = 30\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build -a failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // -a should invoke agent in AFK mode (with --print marker)
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--print"),
        "should invoke agent with --print (afk marker), got: {args}"
    );
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass build prompt path, got: {args}"
    );
}

#[test]
fn build_dash_a_dash_i_errors() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).args(["build", "-a", "-i"]));

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
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "auto_push = true\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 30\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-n", "5"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build -n 5 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Iterations are now managed internally by iter_runner, not passed as agent args.
    // Verify -n override via the iteration banner in stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("of 5"),
        "should show 'of 5' in iteration banner (not 'of 30'), got stdout: {stdout}"
    );
    assert!(
        !stdout.contains("of 30"),
        "should NOT show 'of 30' in iteration banner, got stdout: {stdout}"
    );
}

#[test]
fn unknown_command_errors_with_clear_message() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).arg("nonexistent-cmd"));

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

// ===========================================================================
// End-to-end: sgf → agent → cl → mock claude-wrapper-secret
// ===========================================================================

fn target_bin_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_BIN_EXE_sgf"));
    dir.pop();
    dir
}

#[test]
fn e2e_sgf_agent_cl_context_files_in_append_system_prompt() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
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

    // Set up prompts in the HOME/.sgf/prompts/ directory so prompt resolution works
    let home_prompts = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&home_prompts).unwrap();
    for name in PROMPT_NAMES {
        fs::write(
            home_prompts.join(format!("{name}.md")),
            format!("{name} prompt\n"),
        )
        .unwrap();
    }
    git_add_commit(tmp.path(), "add context files");

    let bin_dir = target_bin_dir();
    assert!(
        bin_dir.join("cl").exists(),
        "cl binary not found at {}; run `cargo build --workspace` first",
        bin_dir.join("cl").display()
    );

    // Mock dir: claude-wrapper-secret (captures args) + pn (no-op) + fm (no-op)
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
                "touch \"$PWD/.iter-complete\"\n",
            ),
            captured = captured_args.display(),
        ),
    );
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");

    // PATH: mock_dir (claude-wrapper-secret, pn) → bin_dir (cl) → system
    let path = format!(
        "{}:{}:{}",
        mock_dir.path().display(),
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default(),
    );

    // iter_runner spawns cl directly (no separate agent binary); cl adds context
    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("PATH", &path)
            .env("HOME", tmp.path().to_str().unwrap()),
    );

    assert!(
        output.status.success(),
        "sgf build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let args = fs::read_to_string(&captured_args).expect("read captured args from mock agent");
    let lines: Vec<&str> = args.lines().collect();

    // cl should inject --append-system-prompt with MEMENTO, BACKPRESSURE context
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

    // agent should pass AFK-mode flags
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

// ===========================================================================
// Session resume E2E integration tests
// ===========================================================================

/// Helper: find exactly one .json metadata file in .sgf/run/ and parse it.
fn read_single_session_metadata(root: &Path) -> serde_json::Value {
    let run_dir = root.join(".sgf/run");
    let run_subdirs: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(
        run_subdirs.len(),
        1,
        "expected exactly 1 run directory, found {}",
        run_subdirs.len()
    );
    let meta_path = run_subdirs[0].path().join("meta.json");
    assert!(meta_path.exists(), "meta.json should exist in run dir");
    let contents = fs::read_to_string(&meta_path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

#[test]
fn afk_session_writes_metadata_with_session_id() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_single_session_metadata(tmp.path());

    assert!(
        meta["run_id"].as_str().unwrap().starts_with("build-"),
        "run_id should start with build-, got: {}",
        meta["run_id"]
    );
    assert_eq!(meta["cursus"], "build");
    assert_eq!(meta["status"], "completed");
    assert_eq!(meta["spec"], "auth");

    let iters_completed = meta["iters_completed"].as_array().unwrap();
    assert!(
        !iters_completed.is_empty(),
        "iters_completed should not be empty"
    );
    let session_id = iters_completed[0]["session_id"].as_str().unwrap();
    assert!(!session_id.is_empty(), "session_id should not be empty");
    assert!(
        session_id.contains('-'),
        "session_id should be a UUID, got: {session_id}"
    );
    assert_eq!(iters_completed[0]["outcome"], "complete");

    // Verify agent received the same session_id
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(session_id),
        "agent should receive same session_id as metadata, got: {args}"
    );
    assert!(
        args.contains("--session-id"),
        "agent should receive --session-id flag, got: {args}"
    );
}

#[test]
fn interactive_session_writes_metadata_with_session_id() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    // Mock cl produces JSON output (as run_programmatic expects) and captures args.
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo \"$@\" > \"{args}\"\n",
                "# Extract session-id from args for JSON output\n",
                "SID=$(echo \"$@\" | sed -n 's/.*--session-id \\([^ ]*\\).*/\\1/p')\n",
                "echo '{{\"type\":\"result\",\"result\":\"Ready\",\"session_id\":\"'\"$SID\"'\"}}'\n",
                "exit 0\n",
            ),
            args = args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("spec")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

    assert!(
        output.status.success(),
        "sgf spec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_single_session_metadata(tmp.path());

    assert!(
        meta["run_id"].as_str().unwrap().starts_with("spec-"),
        "run_id should start with spec-, got: {}",
        meta["run_id"]
    );
    assert_eq!(meta["cursus"], "spec");
    // Non-TTY interactive cursus enters turn-based flow; no sentinel → waiting_for_input
    assert_eq!(meta["status"], "waiting_for_input");

    let session_id = meta["current_session_id"].as_str().unwrap();
    assert!(
        !session_id.is_empty() && session_id.contains('-'),
        "current_session_id should be a UUID, got: {session_id}"
    );

    // Verify cl received --session-id with the same UUID
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--session-id"),
        "cl should receive --session-id flag, got: {args}"
    );
    assert!(
        args.contains(session_id),
        "cl should receive same session_id as metadata, got: {args}"
    );
}

#[test]
fn resume_with_loop_id_passes_resume_flag_to_cl() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Pre-seed session metadata
    let loop_id = "build-auth-20260316T120000";
    let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let run_dir = tmp.path().join(".sgf/run");
    fs::create_dir_all(&run_dir).unwrap();
    let metadata = serde_json::json!({
        "loop_id": loop_id,
        "iterations": [
            {
                "iteration": 1,
                "session_id": session_id,
                "completed_at": "2026-03-16T12:02:30Z"
            }
        ],
        "stage": "build",
        "spec": "auth",
        "mode": "interactive",
        "prompt": ".sgf/prompts/build.md",
        "iterations_total": 3,
        "status": "interrupted",
        "created_at": "2026-03-16T12:00:00Z",
        "updated_at": "2026-03-16T12:05:30Z"
    });
    fs::write(
        run_dir.join(format!("{loop_id}.json")),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    // Mock cl that captures args
    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("cl_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "--resume", loop_id])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

    assert!(
        output.status.success(),
        "sgf build --resume failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let cl_args = fs::read_to_string(&args_file).unwrap();
    assert!(
        cl_args.contains("--resume"),
        "cl should receive --resume flag, got: {cl_args}"
    );
    assert!(
        cl_args.contains(session_id),
        "cl should receive the session_id, got: {cl_args}"
    );
    assert!(
        cl_args.contains("--dangerously-skip-permissions"),
        "cl should receive --dangerously-skip-permissions flag, got: {cl_args}"
    );
    assert!(
        cl_args.contains("--verbose"),
        "cl should receive --verbose flag, got: {cl_args}"
    );

    // Metadata status should be updated to completed
    let updated_meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join(format!("{loop_id}.json"))).unwrap())
            .unwrap();
    assert_eq!(
        updated_meta["status"], "completed",
        "metadata status should be updated to completed after resume"
    );
}

#[test]
fn resume_with_bad_run_id_exits_1() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).args(["build", "--resume", "nonexistent-loop-id"]));

    assert_eq!(
        output.status.code(),
        Some(1),
        "sgf build --resume with bad run-id should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("run not found"),
        "stderr should contain 'run not found', got: {stderr}"
    );
}

#[test]
fn metadata_survives_interrupted_session() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);

    // Send double SIGINT to trigger shutdown
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
    assert_eq!(output.status.code(), Some(130));

    // Metadata should exist with interrupted status
    let meta = read_single_session_metadata(tmp.path());

    assert_eq!(
        meta["status"], "interrupted",
        "status should be 'interrupted' after SIGINT"
    );
    assert!(
        meta["run_id"].as_str().unwrap().starts_with("build-"),
        "run_id should start with build-"
    );
    assert_eq!(meta["cursus"], "build");
}

#[test]
fn resume_multi_iteration_picks_selected_session() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let loop_id = "build-auth-20260316T140000";
    let session_id_1 = "11111111-1111-1111-1111-111111111111";
    let session_id_2 = "22222222-2222-2222-2222-222222222222";
    let run_dir = tmp.path().join(".sgf/run");
    fs::create_dir_all(&run_dir).unwrap();
    let metadata = serde_json::json!({
        "loop_id": loop_id,
        "iterations": [
            {
                "iteration": 1,
                "session_id": session_id_1,
                "completed_at": "2026-03-16T14:02:30Z"
            },
            {
                "iteration": 2,
                "session_id": session_id_2,
                "completed_at": "2026-03-16T14:05:30Z"
            }
        ],
        "stage": "build",
        "spec": "auth",
        "mode": "afk",
        "prompt": ".sgf/prompts/build.md",
        "iterations_total": 3,
        "status": "exhausted",
        "created_at": "2026-03-16T14:00:00Z",
        "updated_at": "2026-03-16T14:05:30Z"
    });
    fs::write(
        run_dir.join(format!("{loop_id}.json")),
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("cl_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    // Pipe "2\n" to stdin to select iteration 2
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "--resume", loop_id])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().expect("open stdin");
        stdin.write_all(b"2\n").expect("write selection");
    }

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert!(
        output.status.success(),
        "sgf build --resume should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let cl_args = fs::read_to_string(&args_file).unwrap();
    assert!(
        cl_args.contains("--resume"),
        "cl should receive --resume flag, got: {cl_args}"
    );
    assert!(
        cl_args.contains(session_id_2),
        "cl should receive iteration 2's session_id, got: {cl_args}"
    );
    assert!(
        !cl_args.contains(session_id_1),
        "cl should NOT receive iteration 1's session_id, got: {cl_args}"
    );
}

#[test]
fn resume_cursus_run_id_delegates_to_cursus_resume() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Create a cursus TOML definition that the resume logic can resolve
    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("mypipe.toml"),
        concat!(
            "description = \"Dispatch test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"step1\"\n",
            "prompt = \"step1.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"step2\"\n",
            "prompt = \"step2.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("step1.md"), "step1 prompt\n").unwrap();
    fs::write(prompts_dir.join("step2.md"), "step2 prompt\n").unwrap();
    git_add_commit(tmp.path(), "add dispatch test cursus");

    // Pre-seed cursus metadata at .sgf/run/<run_id>/meta.json with stalled status
    let run_id = "mypipe-20260316T150000";
    let run_meta_dir = tmp.path().join(".sgf/run").join(run_id);
    fs::create_dir_all(&run_meta_dir).unwrap();
    let cursus_meta = serde_json::json!({
        "run_id": run_id,
        "cursus": "mypipe",
        "status": "stalled",
        "current_iter": "step2",
        "current_iter_index": 1,
        "iters_completed": [
            {
                "name": "step1",
                "session_id": "aaaaaaaa-1111-1111-1111-111111111111",
                "completed_at": "2026-03-16T15:02:30Z",
                "outcome": "complete"
            }
        ],
        "spec": null,
        "mode_override": null,
        "context_producers": {},
        "current_session_id": "bbbbbbbb-2222-2222-2222-222222222222",
        "created_at": "2026-03-16T15:00:00Z",
        "updated_at": "2026-03-16T15:05:30Z"
    });
    fs::write(
        run_meta_dir.join("meta.json"),
        serde_json::to_string_pretty(&cursus_meta).unwrap(),
    )
    .unwrap();

    // Mock agent that completes step2
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    // Resume via --resume on a subcommand — should dispatch to cursus resume
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["mypipe", "--resume", run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("failed to spawn sgf --resume for cursus");

    // Cursus resume with stalled status prompts for action; send "retry"
    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"retry\n").unwrap();
    }

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf --resume cursus");
    drop(_permit);

    // Verify the dispatch delegated to cursus resume (metadata updated)
    let meta = read_run_metadata(tmp.path());
    assert_eq!(
        meta["cursus"].as_str().unwrap(),
        "mypipe",
        "cursus name should be preserved in metadata"
    );
    assert_eq!(
        meta["status"].as_str().unwrap(),
        "completed",
        "run should complete after resuming step2, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let completed = meta["iters_completed"].as_array().unwrap();
    assert!(
        completed.iter().any(|c| c["name"] == "step1"),
        "step1 should be in completed iters"
    );
}

#[test]
fn resume_corrupt_metadata_exits_1() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Pre-seed a corrupt (invalid JSON) non-cursus metadata file
    let loop_id = "build-auth-20260316T160000";
    let run_dir = tmp.path().join(".sgf/run");
    fs::create_dir_all(&run_dir).unwrap();
    fs::write(
        run_dir.join(format!("{loop_id}.json")),
        "this is not valid json{{{",
    )
    .unwrap();

    let output = run_sgf(sgf_cmd(tmp.path()).args(["build", "--resume", loop_id]));

    assert_eq!(
        output.status.code(),
        Some(1),
        "sgf --resume with corrupt metadata should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("corrupt") || stderr.contains("Session metadata not found"),
        "stderr should mention corruption, got: {stderr}"
    );
}

// ===========================================================================
// Cursus: single-iter integration (binary-level)
// ===========================================================================

#[test]
fn cursus_single_iter_dispatches_and_completes() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Create a local cursus TOML (single iter, afk mode)
    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Test build cursus\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 5\n",
        ),
    )
    .unwrap();

    // Create the prompt file referenced by the cursus
    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt content\n").unwrap();
    git_add_commit(tmp.path(), "add cursus and prompt");

    // Mock agent that logs args and touches .iter-complete sentinel
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "echo \"$@\" > \"{args}\"\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "exit 0\n",
            ),
            args = args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf build via cursus should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify agent was invoked with expected flags (iter_runner passes these to cl)
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--print"),
        "should pass --print in afk mode, got: {args}"
    );
    assert!(
        args.contains("--output-format stream-json"),
        "should pass --output-format stream-json in afk mode, got: {args}"
    );
    assert!(
        args.contains("--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions, got: {args}"
    );
    assert!(
        args.contains("--session-id"),
        "should pass --session-id, got: {args}"
    );
    assert!(
        args.contains(".sgf/prompts/build.md"),
        "should pass prompt path, got: {args}"
    );

    // Sentinel should be cleaned up after completion
    assert!(
        !tmp.path().join(".iter-complete").exists(),
        ".iter-complete sentinel should be cleaned up"
    );

    // Run metadata should exist and show completed status
    let run_dir = tmp.path().join(".sgf/run");
    assert!(run_dir.exists(), ".sgf/run/ should exist");
    let run_entries: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(
        run_entries.len(),
        1,
        "should have exactly one run directory"
    );

    let meta_path = run_entries[0].path().join("meta.json");
    assert!(meta_path.exists(), "meta.json should exist in run dir");
    let meta_content = fs::read_to_string(&meta_path).unwrap();
    let meta_json: serde_json::Value = serde_json::from_str(&meta_content).unwrap();
    assert_eq!(
        meta_json["status"].as_str().unwrap(),
        "completed",
        "run status should be completed"
    );
    assert_eq!(
        meta_json["iters_completed"][0]["outcome"].as_str().unwrap(),
        "complete",
        "iter outcome should be complete"
    );

    // PID file should be cleaned up
    let remaining_pids: Vec<_> = fs::read_dir(&run_entries[0].path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "pid"))
        .collect();
    assert!(
        remaining_pids.is_empty(),
        "PID file should be cleaned up after cursus completes"
    );
}

#[test]
fn cursus_single_iter_alias_dispatch() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Test build cursus\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add cursus and prompt");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    // Use alias "b" instead of "build"
    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["b", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf b (alias) via cursus should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify agent was actually invoked
    assert!(
        args_file.exists(),
        "agent should have been invoked via alias dispatch"
    );
}

#[test]
fn cursus_single_iter_sentinel_cleaned_on_exhausted() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Test build cursus\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add cursus and prompt");

    // Mock agent that does NOT touch any sentinel and exits 2 (simulates exhaustion)
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 2\n");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    // Should exit with code 2 (stalled)
    assert_eq!(
        output.status.code(),
        Some(2),
        "exhausted iter should exit 2 (stalled), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Run metadata should show stalled status
    let run_dir = tmp.path().join(".sgf/run");
    let run_entries: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    let meta_content = fs::read_to_string(run_entries[0].path().join("meta.json")).unwrap();
    let meta_json: serde_json::Value = serde_json::from_str(&meta_content).unwrap();
    assert_eq!(
        meta_json["status"].as_str().unwrap(),
        "stalled",
        "run status should be stalled"
    );
}

// ===========================================================================
// Test harness infrastructure
// ===========================================================================

#[test]
fn harness_semaphore_limits_concurrency() {
    let sem = ProcessSemaphore::new(3);
    let peak = AtomicUsize::new(0);
    let current = AtomicUsize::new(0);

    std::thread::scope(|s| {
        for _ in 0..10 {
            s.spawn(|| {
                let _permit = sem.acquire();
                let val = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(val, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                current.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });

    let peak_val = peak.load(Ordering::SeqCst);
    assert!(
        peak_val <= 3,
        "peak concurrency was {peak_val}, expected <= 3"
    );
    assert!(
        peak_val >= 2,
        "peak concurrency was {peak_val}, expected >= 2 (semaphore should allow parallelism)"
    );
}

// ===========================================================================
// Cursus: multi-iter integration (binary-level)
// ===========================================================================

const MULTI_ITER_CURSUS: &str = "\
description = \"Multi-iter test cursus\"\n\
auto_push = false\n\
\n\
[[iter]]\n\
name = \"discuss\"\n\
prompt = \"discuss.md\"\n\
mode = \"afk\"\n\
iterations = 1\n\
produces = \"discuss-summary\"\n\
\n\
[[iter]]\n\
name = \"draft\"\n\
prompt = \"draft.md\"\n\
mode = \"afk\"\n\
iterations = 10\n\
consumes = [\"discuss-summary\"]\n\
produces = \"draft-presentation\"\n\
\n\
[[iter]]\n\
name = \"review\"\n\
prompt = \"review.md\"\n\
mode = \"afk\"\n\
iterations = 1\n\
consumes = [\"discuss-summary\", \"draft-presentation\"]\n\
next = \"approve\"\n\
\n\
[iter.transitions]\n\
on_reject = \"draft\"\n\
on_revise = \"revise\"\n\
\n\
[[iter]]\n\
name = \"revise\"\n\
prompt = \"revise.md\"\n\
mode = \"afk\"\n\
iterations = 5\n\
consumes = [\"discuss-summary\", \"draft-presentation\"]\n\
produces = \"draft-presentation\"\n\
next = \"review\"\n\
\n\
[[iter]]\n\
name = \"approve\"\n\
prompt = \"approve.md\"\n\
mode = \"afk\"\n\
iterations = 1\n\
consumes = [\"draft-presentation\"]\n\
";

fn setup_multi_iter_test() -> TempDir {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(cursus_dir.join("pipeline.toml"), MULTI_ITER_CURSUS).unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    for name in &["discuss", "draft", "review", "revise", "approve"] {
        fs::write(
            prompts_dir.join(format!("{name}.md")),
            format!("{name} prompt\n"),
        )
        .unwrap();
    }
    git_add_commit(tmp.path(), "add multi-iter cursus and prompts");
    tmp
}

fn read_run_metadata(tmp: &Path) -> serde_json::Value {
    let run_dir = tmp.join(".sgf/run");
    let run_entries: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert!(
        !run_entries.is_empty(),
        "should have at least one run directory"
    );
    let meta_content = fs::read_to_string(run_entries[0].path().join("meta.json")).unwrap();
    serde_json::from_str(&meta_content).unwrap()
}

fn get_run_id(tmp: &Path) -> String {
    let run_dir = tmp.join(".sgf/run");
    let run_entries: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    run_entries[0].file_name().to_string_lossy().to_string()
}

#[test]
fn cursus_multi_iter_happy_path() {
    let tmp = setup_multi_iter_test();

    let mock_dir = TempDir::new().unwrap();
    let invocation_log = mock_dir.path().join("invocations.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "PROMPT=\"${{@: -1}}\"\n",
                "echo \"$PROMPT\" >> \"{log}\"\n",
                "# Write context for iters that produce\n",
                "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
                "  mkdir -p \"$SGF_RUN_CONTEXT\"\n",
                "  echo 'Discussion summary content' > \"$SGF_RUN_CONTEXT/discuss-summary.md\"\n",
                "fi\n",
                "if echo \"$PROMPT\" | grep -q 'draft.md'; then\n",
                "  mkdir -p \"$SGF_RUN_CONTEXT\"\n",
                "  echo 'Draft presentation content' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
                "fi\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "exit 0\n",
            ),
            log = invocation_log.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "multi-iter happy path should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invocations = fs::read_to_string(&invocation_log).unwrap();
    let lines: Vec<&str> = invocations.lines().collect();
    assert_eq!(
        lines.len(),
        4,
        "should invoke 4 iters (revise skipped): {invocations}"
    );
    assert!(lines[0].contains("discuss.md"), "1st iter: discuss");
    assert!(lines[1].contains("draft.md"), "2nd iter: draft");
    assert!(lines[2].contains("review.md"), "3rd iter: review");
    assert!(lines[3].contains("approve.md"), "4th iter: approve");

    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
    assert_eq!(meta["iters_completed"].as_array().unwrap().len(), 4);
}

#[test]
fn cursus_multi_iter_reject_transition() {
    let tmp = setup_multi_iter_test();

    let mock_dir = TempDir::new().unwrap();
    let invocation_log = mock_dir.path().join("invocations.txt");
    let call_count_file = mock_dir.path().join("review_count");
    fs::write(&call_count_file, "0").unwrap();

    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "PROMPT=\"${{@: -1}}\"\n",
                "echo \"$PROMPT\" >> \"{log}\"\n",
                "mkdir -p \"$SGF_RUN_CONTEXT\" 2>/dev/null\n",
                "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
                "  echo 'summary' > \"$SGF_RUN_CONTEXT/discuss-summary.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'draft.md'; then\n",
                "  echo 'draft' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'review.md'; then\n",
                "  COUNT=$(cat \"{count}\")\n",
                "  COUNT=$((COUNT + 1))\n",
                "  echo $COUNT > \"{count}\"\n",
                "  if [ $COUNT -eq 1 ]; then\n",
                "    touch \"${{PWD}}/.iter-reject\"\n",
                "  else\n",
                "    touch \"${{PWD}}/.iter-complete\"\n",
                "  fi\n",
                "elif echo \"$PROMPT\" | grep -q 'approve.md'; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "fi\n",
                "exit 0\n",
            ),
            log = invocation_log.display(),
            count = call_count_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "reject transition should eventually complete: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invocations = fs::read_to_string(&invocation_log).unwrap();
    let lines: Vec<&str> = invocations.lines().collect();
    // discuss -> draft -> review(reject) -> draft -> review(complete) -> approve
    assert_eq!(
        lines.len(),
        6,
        "should have 6 invocations (with reject loop): {invocations}"
    );
    assert!(lines[0].contains("discuss.md"));
    assert!(lines[1].contains("draft.md"));
    assert!(lines[2].contains("review.md"), "first review");
    assert!(lines[3].contains("draft.md"), "back to draft after reject");
    assert!(lines[4].contains("review.md"), "second review");
    assert!(lines[5].contains("approve.md"));

    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
    let completed = meta["iters_completed"].as_array().unwrap();
    let outcomes: Vec<&str> = completed
        .iter()
        .map(|c| c["outcome"].as_str().unwrap())
        .collect();
    assert!(
        outcomes.contains(&"reject"),
        "should record reject outcome: {outcomes:?}"
    );
}

#[test]
fn cursus_multi_iter_revise_transition() {
    let tmp = setup_multi_iter_test();

    let mock_dir = TempDir::new().unwrap();
    let invocation_log = mock_dir.path().join("invocations.txt");
    let call_count_file = mock_dir.path().join("review_count");
    fs::write(&call_count_file, "0").unwrap();

    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "PROMPT=\"${{@: -1}}\"\n",
                "echo \"$PROMPT\" >> \"{log}\"\n",
                "mkdir -p \"$SGF_RUN_CONTEXT\" 2>/dev/null\n",
                "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
                "  echo 'summary' > \"$SGF_RUN_CONTEXT/discuss-summary.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'draft.md'; then\n",
                "  echo 'draft' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'review.md'; then\n",
                "  COUNT=$(cat \"{count}\")\n",
                "  COUNT=$((COUNT + 1))\n",
                "  echo $COUNT > \"{count}\"\n",
                "  if [ $COUNT -eq 1 ]; then\n",
                "    touch \"${{PWD}}/.iter-revise\"\n",
                "  else\n",
                "    touch \"${{PWD}}/.iter-complete\"\n",
                "  fi\n",
                "elif echo \"$PROMPT\" | grep -q 'revise.md'; then\n",
                "  echo 'revised draft' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'approve.md'; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "fi\n",
                "exit 0\n",
            ),
            log = invocation_log.display(),
            count = call_count_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "revise transition should eventually complete: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let invocations = fs::read_to_string(&invocation_log).unwrap();
    let lines: Vec<&str> = invocations.lines().collect();
    // discuss -> draft -> review(revise) -> revise -> review(complete) -> approve
    assert_eq!(
        lines.len(),
        6,
        "should have 6 invocations (with revise loop): {invocations}"
    );
    assert!(lines[0].contains("discuss.md"));
    assert!(lines[1].contains("draft.md"));
    assert!(lines[2].contains("review.md"), "first review");
    assert!(
        lines[3].contains("revise.md"),
        "revise after review signals revise"
    );
    assert!(
        lines[4].contains("review.md"),
        "back to review after revise (next override)"
    );
    assert!(lines[5].contains("approve.md"));

    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
    let completed = meta["iters_completed"].as_array().unwrap();
    let outcomes: Vec<&str> = completed
        .iter()
        .map(|c| c["outcome"].as_str().unwrap())
        .collect();
    assert!(
        outcomes.contains(&"revise"),
        "should record revise outcome: {outcomes:?}"
    );
}

#[test]
fn cursus_multi_iter_context_passing() {
    let tmp = setup_multi_iter_test();

    let mock_dir = TempDir::new().unwrap();
    let draft_args_file = mock_dir.path().join("draft_args.txt");

    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "PROMPT=\"${{@: -1}}\"\n",
                "mkdir -p \"$SGF_RUN_CONTEXT\" 2>/dev/null\n",
                "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
                "  echo 'Key insight from discussion' > \"$SGF_RUN_CONTEXT/discuss-summary.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'draft.md'; then\n",
                "  echo \"$@\" > \"{draft_args}\"\n",
                "  echo 'Draft content' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'review.md'; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "elif echo \"$PROMPT\" | grep -q 'approve.md'; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "fi\n",
                "exit 0\n",
            ),
            draft_args = draft_args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "context passing test should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify that draft received context from discuss via --append-system-prompt
    // The consumed content is written to a _consumed file and injected via --append-system-prompt
    let draft_args = fs::read_to_string(&draft_args_file).unwrap();
    assert!(
        draft_args.contains("--append-system-prompt"),
        "draft should receive consumed context via --append-system-prompt, got: {draft_args}"
    );

    // Verify the context file was written and contains the consumed content
    let run_id = get_run_id(tmp.path());
    let context_dir = tmp.path().join(format!(".sgf/run/{run_id}/context"));
    assert!(context_dir.exists(), "context directory should exist");

    // Verify discuss-summary was produced
    let summary_path = context_dir.join("discuss-summary.md");
    assert!(
        summary_path.exists(),
        "discuss-summary.md should exist in context dir"
    );
    let summary_content = fs::read_to_string(&summary_path).unwrap();
    assert!(
        summary_content.contains("Key insight from discussion"),
        "summary should contain discuss output"
    );
}

#[test]
fn cursus_multi_iter_stall_recovery() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    // Two-iter cursus: discuss completes, draft stalls
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Stall test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"discuss\"\n",
            "prompt = \"discuss.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"draft\"\n",
            "prompt = \"draft.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 3\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("discuss.md"), "discuss prompt\n").unwrap();
    fs::write(prompts_dir.join("draft.md"), "draft prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
            "  touch \"${PWD}/.iter-complete\"\n",
            "  exit 0\n",
            "fi\n",
            "# draft does NOT touch any sentinel -> exhaustion (exit 2)\n",
            "exit 2\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_FORCE_TERMINAL", "1"),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "stalled pipeline should exit 2: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_run_metadata(tmp.path());
    assert_eq!(
        meta["status"].as_str().unwrap(),
        "stalled",
        "run status should be stalled"
    );
    assert_eq!(
        meta["current_iter"].as_str().unwrap(),
        "draft",
        "should stall at draft iter"
    );
    let completed = meta["iters_completed"].as_array().unwrap();
    assert!(completed.len() >= 1, "discuss should be in completed iters");
    assert_eq!(
        completed[0]["name"].as_str().unwrap(),
        "discuss",
        "first completed iter should be discuss"
    );

    // Stall banner should mention resume command
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("STALLED") || stderr.contains("stalled"),
        "stderr should contain stall indication: {stderr}"
    );
}

#[test]
fn cursus_multi_iter_resume_stalled_run() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Create a two-iter cursus where draft stalls, then we resume
    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Resume test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"discuss\"\n",
            "prompt = \"discuss.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"draft\"\n",
            "prompt = \"draft.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("discuss.md"), "discuss prompt\n").unwrap();
    fs::write(prompts_dir.join("draft.md"), "draft prompt\n").unwrap();
    git_add_commit(tmp.path(), "add resume cursus");

    // First run: discuss completes, draft stalls
    let mock_dir = TempDir::new().unwrap();
    let mock_agent_stall = create_mock_script(
        mock_dir.path(),
        "mock_agent_stall.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
            "  touch \"${PWD}/.iter-complete\"\n",
            "  exit 0\n",
            "fi\n",
            "exit 2\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent_stall),
    );

    assert_eq!(output.status.code(), Some(2), "should stall");

    let run_id = get_run_id(tmp.path());

    // Now resume: mock agent that completes draft
    let mock_agent_complete = create_mock_script(
        mock_dir.path(),
        "mock_agent_complete.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    // Resume sends "2\n" (skip) via stdin to skip the stalled iter
    // Resume prompts interactively for action (1=retry, 2=skip, 3=abort).
    // Pipe "2\n" (skip) via a child process to test the skip path.
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["pipeline", "--resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent_complete)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("failed to spawn sgf pipeline --resume");

    // Write "skip" to stdin (programmatic mode accepts action words)
    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"skip\n").unwrap();
    }

    let resume_output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf pipeline --resume");
    drop(_permit);
    let _ = &resume_output;

    let meta = read_run_metadata(tmp.path());
    // After skip, the pipeline should complete since draft was the last iter
    assert_eq!(
        meta["status"].as_str().unwrap(),
        "completed",
        "run should be completed after skipping final iter, got: {}. stderr: {}",
        meta["status"].as_str().unwrap(),
        String::from_utf8_lossy(&resume_output.stderr)
    );
    assert_eq!(
        meta["cursus"].as_str().unwrap(),
        "pipeline",
        "cursus name should be preserved"
    );
    let completed = meta["iters_completed"].as_array().unwrap();
    assert!(
        completed.len() >= 1,
        "at least discuss should be in completed"
    );
    assert_eq!(completed[0]["name"].as_str().unwrap(), "discuss");
}

#[test]
fn cursus_layered_resolution_local_overrides_global() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // Set up global cursus (via FAKE_HOME)
    let global_cursus = fake_home().join(".sgf/cursus");
    fs::create_dir_all(&global_cursus).unwrap();
    fs::write(
        global_cursus.join("layered.toml"),
        concat!(
            "description = \"Global layered\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"global-iter\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    // Set up local cursus (overrides global)
    let local_cursus = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&local_cursus).unwrap();
    fs::write(
        local_cursus.join("layered.toml"),
        concat!(
            "description = \"Local layered\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"local-iter\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    // Prompt file (build.md already exists in global prompts)
    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Local build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add layered cursus");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["layered", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "layered resolution should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the local iter ran (not the global one)
    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
    let completed = meta["iters_completed"].as_array().unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0]["name"].as_str().unwrap(),
        "local-iter",
        "should run local iter, not global"
    );

    // Clean up global cursus to not interfere with other tests
    let _ = fs::remove_file(global_cursus.join("layered.toml"));
}

// ===========================================================================
// Cursus: banner flag passthrough
// ===========================================================================

#[test]
fn cursus_banner_true_passes_banner_flag_to_agent() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "banner-test",
        concat!(
            "description = \"Banner test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "banner = true\n",
        ),
    );

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["banner-test", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf banner-test should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Banner is now rendered directly by iter_runner, not passed as agent flag.
    // Verify banner output appears in stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Loop Starting"),
        "banner = true should render startup banner in stdout, got: {stdout}"
    );
}

#[test]
fn cursus_banner_false_omits_banner_flag() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "no-banner",
        concat!(
            "description = \"No banner test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["no-banner", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf no-banner should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        !args.contains("--banner"),
        "should not pass --banner flag when banner is not set, got: {args}"
    );
}

// ===========================================================================
// sgf list
// ===========================================================================

#[test]
fn list_shows_builtins_when_no_cursus() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).arg("list"));

    assert!(output.status.success(), "sgf list should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Built-ins:"),
        "should show Built-ins header: {stdout}"
    );
    assert!(stdout.contains("init"), "should list init builtin");
    assert!(stdout.contains("list"), "should list list builtin");
    assert!(stdout.contains("logs"), "should list logs builtin");
    assert!(
        !stdout.contains("Available commands:"),
        "should not show Available commands when no cursus defined: {stdout}"
    );
}

#[test]
fn list_shows_local_cursus_commands() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).arg("list"));

    assert!(output.status.success(), "sgf list should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Available commands:"),
        "should show Available commands header: {stdout}"
    );

    for name in [
        "build",
        "doc",
        "install",
        "issues-log",
        "spec",
        "test",
        "test-plan",
        "verify",
    ] {
        assert!(
            stdout.contains(name),
            "should list '{name}' command: {stdout}"
        );
    }

    assert!(
        stdout.contains("Built-ins:"),
        "should still show Built-ins: {stdout}"
    );
}

#[test]
fn list_shows_descriptions_from_cursus_toml() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "deploy",
        "description = \"Deploy the app\"\nauto_push = false\n\n\
         [[iter]]\nname = \"deploy\"\nprompt = \"deploy.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    );

    let output = run_sgf(sgf_cmd(tmp.path()).arg("list"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Deploy the app"),
        "should show description from TOML: {stdout}"
    );
}

/// Create a temporary HOME with .sgf/prompts (like fake_home) but isolated
/// so global cursus files don't leak between tests.
fn isolated_home_with_global_cursus(cursus_files: &[(&str, &str)]) -> TempDir {
    let home = TempDir::new().unwrap();
    let prompts = home.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts).unwrap();
    for name in PROMPT_NAMES {
        fs::write(
            prompts.join(format!("{name}.md")),
            format!("{name} prompt\n"),
        )
        .unwrap();
    }
    fs::write(prompts.join("config.toml"), CONFIG_TOML).unwrap();

    let global_cursus = home.path().join(".sgf/cursus");
    fs::create_dir_all(&global_cursus).unwrap();
    for &(name, content) in cursus_files {
        fs::write(global_cursus.join(format!("{name}.toml")), content).unwrap();
    }
    home
}

#[test]
fn list_global_cursus_appears_when_no_local() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let home = isolated_home_with_global_cursus(&[(
        "global-cmd",
        "description = \"A global command\"\nauto_push = false\n\n\
         [[iter]]\nname = \"global-cmd\"\nprompt = \"build.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    )]);

    let output = run_sgf(sgf_cmd(tmp.path()).env("HOME", home.path()).arg("list"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("global-cmd"),
        "should show global cursus command: {stdout}"
    );
    assert!(
        stdout.contains("A global command"),
        "should show global command description: {stdout}"
    );
}

#[test]
fn list_local_overrides_global_cursus() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let home = isolated_home_with_global_cursus(&[(
        "shared-cmd",
        "description = \"Global version\"\nauto_push = false\n\n\
         [[iter]]\nname = \"shared-cmd\"\nprompt = \"build.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    )]);

    write_cursus_toml(
        tmp.path(),
        "shared-cmd",
        "description = \"Local version\"\nauto_push = false\n\n\
         [[iter]]\nname = \"shared-cmd\"\nprompt = \"build.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    );

    let output = run_sgf(sgf_cmd(tmp.path()).env("HOME", home.path()).arg("list"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Local version"),
        "local should override global description: {stdout}"
    );
    assert!(
        !stdout.contains("Global version"),
        "global description should not appear when overridden: {stdout}"
    );
}

#[test]
fn list_commands_sorted_alphabetically() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "zebra",
        "description = \"Zebra cmd\"\nauto_push = false\n\n\
         [[iter]]\nname = \"zebra\"\nprompt = \"build.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    );
    write_cursus_toml(
        tmp.path(),
        "alpha",
        "description = \"Alpha cmd\"\nauto_push = false\n\n\
         [[iter]]\nname = \"alpha\"\nprompt = \"build.md\"\n\
         mode = \"afk\"\niterations = 1\n",
    );

    let output = run_sgf(sgf_cmd(tmp.path()).arg("list"));

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let alpha_pos = stdout.find("alpha").expect("should contain alpha");
    let zebra_pos = stdout.find("zebra").expect("should contain zebra");
    assert!(
        alpha_pos < zebra_pos,
        "alpha should appear before zebra (sorted): {stdout}"
    );
}

#[test]
fn cursus_afk_then_interactive_stdin_not_stolen() {
    use std::io::Write;

    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let stdin_capture = mock_dir.path().join("stdin_capture.txt");

    // Unified mock cl that handles both AFK mode (stream-json) and interactive mode (json).
    // AFK path uses --output-format stream-json; programmatic interactive uses --output-format json.
    // In programmatic interactive mode, user input arrives as the last positional arg.
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "if echo \"$@\" | grep -q -- 'stream-json'; then\n",
                "  mkdir -p \"$SGF_RUN_CONTEXT\" 2>/dev/null\n",
                "  echo 'gathered' > \"$SGF_RUN_CONTEXT/gather-output.md\"\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  exit 0\n",
                "else\n",
                "  LAST_ARG=\"${{@: -1}}\"\n",
                "  if [[ \"$LAST_ARG\" == @* ]]; then\n",
                "    cat \"${{LAST_ARG:1}}\" > \"{capture}\"\n",
                "  else\n",
                "    echo \"$@\" > \"{capture}\"\n",
                "  fi\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  echo '{{\"type\":\"result\",\"result\":\"Done\",\"session_id\":\"sess-discuss\"}}'\n",
                "  exit 0\n",
                "fi\n",
            ),
            capture = stdin_capture.display()
        ),
    );

    write_cursus_toml(
        tmp.path(),
        "mixed",
        concat!(
            "description = \"Mixed afk-then-interactive\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"gather\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "produces = \"gather-output\"\n",
            "\n",
            "[[iter]]\n",
            "name = \"discuss\"\n",
            "prompt = \"spec.md\"\n",
            "mode = \"interactive\"\n",
            "consumes = [\"gather-output\"]\n",
        ),
    );

    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["mixed", "auth"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let mut stdin = guard.child_mut().stdin.take().expect("open stdin");

    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        let _ = stdin.write_all(b"hello_from_stdin\n");
        let _ = stdin.flush();
        std::thread::sleep(Duration::from_secs(1));
        drop(stdin);
    });

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
    writer.join().ok();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "sgf should succeed, stderr: {stderr}"
    );

    let captured = fs::read_to_string(&stdin_capture).unwrap_or_default();
    assert!(
        captured.contains("hello_from_stdin"),
        "interactive cl should receive piped user input as arg, but got: {captured:?}\nstderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Simple Prompt Mode tests (sgf <file>)
// ---------------------------------------------------------------------------

#[test]
fn simple_prompt_mode_runs_file_directly() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let prompt_path = tmp.path().join("my-task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("my-task.md")
            .arg("-a")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf simple prompt should succeed (exit 0), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("my-task.md"),
        "agent should receive the prompt file path, got: {args}"
    );
}

#[test]
fn simple_prompt_mode_defaults_to_one_iteration() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let count_file = mock_dir.path().join("count.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  count=$(cat \"{}\")\n",
                "else\n",
                "  count=0\n",
                "fi\n",
                "count=$((count + 1))\n",
                "echo $count > \"{}\"\n",
                "exit 0\n",
            ),
            count_file.display(),
            count_file.display(),
            count_file.display(),
        ),
    );

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("task.md")
            .arg("-a")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (iterations exhausted) with default 1 iteration and no sentinel, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let count: u32 = fs::read_to_string(&count_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(count, 1, "should run exactly 1 iteration by default");
}

#[test]
fn simple_prompt_mode_respects_iteration_count() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let count_file = mock_dir.path().join("count.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  count=$(cat \"{}\")\n",
                "else\n",
                "  count=0\n",
                "fi\n",
                "count=$((count + 1))\n",
                "echo $count > \"{}\"\n",
                "if [ $count -ge 3 ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "fi\n",
                "exit 0\n",
            ),
            count_file.display(),
            count_file.display(),
            count_file.display(),
        ),
    );

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("task.md")
            .arg("-a")
            .arg("-n")
            .arg("5")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf should succeed when sentinel is created, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let count: u32 = fs::read_to_string(&count_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        count, 3,
        "should stop after sentinel is created on iteration 3"
    );
}

#[test]
fn simple_prompt_mode_with_absolute_path() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let prompt_path = tmp.path().join("my-task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg(prompt_path.to_str().unwrap())
            .arg("-a")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf simple prompt with absolute path should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn simple_prompt_mode_nonexistent_file_falls_through_to_cursus() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).arg("nonexistent.md"));

    assert!(
        !output.status.success(),
        "should fail for nonexistent file that is also not a cursus command"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown command"),
        "should report unknown command, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Migrated from legacy integration tests
// These tests exercise the iter_runner module through sgf simple prompt mode.
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn create_fixture_mock(dir: &Path, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!("#!/bin/bash\ncat {}\n", fixture_path.display()),
    )
}

fn create_fixture_mock_with_sentinel(dir: &Path, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\ncat {}\ntouch .iter-complete\n",
            fixture_path.display()
        ),
    )
}

fn create_fixture_mock_with_nested_sentinel(
    dir: &Path,
    fixture_name: &str,
    subdir: &str,
) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let nested_dir = dir.join(subdir);
    fs::create_dir_all(&nested_dir).expect("create nested dir");
    let sentinel_path = nested_dir.join(".iter-complete");
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\ncat {}\ntouch {}\n",
            fixture_path.display(),
            sentinel_path.display()
        ),
    )
}

fn create_fixture_arg_capturing_mock(dir: &Path, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let args_file = dir.join("captured-args.txt");
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\nprintf '%s\\n' \"$@\" > {}\ncat {}\ntouch .iter-complete\n",
            args_file.display(),
            fixture_path.display()
        ),
    )
}

fn create_fixture_slow_mock(dir: &Path, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\ntrap '' INT\nfor i in $(seq 1 50); do cat {}; sleep 0.1; done\n",
            fixture_path.display()
        ),
    )
}

fn create_fixture_multi_iter_arg_capturing_mock(dir: &Path, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let args_dir = dir.join("captured-args");
    fs::create_dir_all(&args_dir).expect("create captured-args dir");
    create_mock_script(
        dir,
        "mock_agent.sh",
        &format!(
            r#"#!/bin/bash
COUNTER_FILE="{counter}"
if [ ! -f "$COUNTER_FILE" ]; then
    echo 1 > "$COUNTER_FILE"
    N=1
else
    N=$(cat "$COUNTER_FILE")
    N=$((N + 1))
    echo $N > "$COUNTER_FILE"
fi
printf '%s\n' "$@" > "{args_dir}/invocation-$N.txt"
cat {fixture}
"#,
            counter = dir.join("invocation-counter.txt").display(),
            args_dir = args_dir.display(),
            fixture = fixture_path.display(),
        ),
    )
}

/// Helper: build sgf command for simple prompt mode with a mock agent.
/// Sets up the prompt file and returns (mock_path, prompt_filename).
fn setup_simple_prompt_test(dir: &Path) -> &'static str {
    fs::write(dir.join("prompt.md"), "Test prompt.\n").expect("write prompt.md");
    "prompt.md"
}

// ---- NDJSON output formatting ----

#[test]
fn iter_afk_formats_text_blocks() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains(
            "I'll start by studying the required files to understand the context and plan."
        ),
        "should contain first text block, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Now I can see the cleanup plan."),
        "should contain second text block, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Done. Updated `specs/tokenizer-embedding.md`"),
        "should contain final text block, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_formats_tool_calls_as_one_liners() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("─ Read  /Users/william/Repos/test-project/specs/README.md"),
        "should contain Read tool call, got:\n{stdout}"
    );
    assert!(
        stdout.contains(
            "─ Read  /Users/william/Repos/test-project/crates/buddy-llm/src/inference.rs [1:80]"
        ),
        "should contain Read with offset:limit, got:\n{stdout}"
    );
    assert!(
        stdout.contains("─ Edit  /Users/william/Repos/test-project/specs/tokenizer-embedding.md"),
        "should contain Edit tool call, got:\n{stdout}"
    );
    assert!(
        stdout.contains("─ TodoWrite  3 items"),
        "should contain TodoWrite with count, got:\n{stdout}"
    );
    assert!(
        stdout.contains("─ Bash  git diff specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md"),
        "should contain Bash tool call, got:\n{stdout}"
    );
    assert!(
        stdout.contains("─ Grep  GgufModelBuilder"),
        "should contain Grep tool call, got:\n{stdout}"
    );
    assert!(
        stdout.contains("─ Glob  specs/**/*.md"),
        "should contain Glob tool call, got:\n{stdout}"
    );

    assert!(
        !stdout.contains("\"old_string\""),
        "should not contain raw Edit old_string key"
    );
    assert!(
        !stdout.contains("\"new_string\""),
        "should not contain raw Edit new_string key"
    );
    assert!(
        !stdout.contains("\"replace_all\""),
        "should not contain raw Edit replace_all key"
    );
    assert!(
        !stdout.contains("GgufModelBuilder::new"),
        "should not contain content from Edit old_string value"
    );
    assert!(
        !stdout.contains("file_path:"),
        "should not contain old jq-style 'file_path:' formatting"
    );
}

#[test]
fn iter_bash_command_truncation() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("─ Bash  git add specs/tokenizer-embedding.md"),
        "should contain the long Bash tool call, got stdout:\n{stdout}"
    );

    let start = stdout
        .find("─ Bash  git add specs/tokenizer-embedding.md")
        .expect("should find Bash tool call");
    let rest = &stdout[start..];
    assert!(
        rest.contains("..."),
        "truncated Bash command should contain '...', got:\n{rest}"
    );

    assert!(
        stdout.contains("─ Bash  git log --oneline -5"),
        "short Bash command should not be truncated, got stdout:\n{stdout}"
    );
}

#[test]
fn iter_afk_hides_tool_results() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("# Test Project Specifications"),
        "should NOT contain tool result content from Read, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Applied edit to specs/tokenizer-embedding.md"),
        "should NOT contain tool result from Edit, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("No such file or directory"),
        "should NOT contain error tool result content, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_hides_test_result_lines() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "test-results.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env_remove("NO_COLOR")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("test inference::test_load_model ... ok"),
        "tool result lines should NOT appear in AFK output, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("test inference::test_generate ... FAILED"),
        "tool result lines should NOT appear in AFK output, got:\n{stdout}"
    );
}

// ---- ANSI color handling ----

#[test]
fn iter_afk_no_color_disables_ansi() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    let has_ansi_style = stdout.as_bytes().windows(2).enumerate().any(|(i, w)| {
        if w == b"\x1b[" {
            let rest = &stdout.as_bytes()[i + 2..];
            if rest.first() == Some(&b'2') && rest.get(1) == Some(&b'K') {
                return false;
            }
            rest.iter()
                .take_while(|&&b| b.is_ascii_digit() || b == b';')
                .last()
                .is_some()
                && rest
                    .iter()
                    .position(|&b| !b.is_ascii_digit() && b != b';')
                    .map(|pos| rest[pos] == b'm')
                    .unwrap_or(false)
        } else {
            false
        }
    });
    assert!(
        !has_ansi_style,
        "NO_COLOR=1 should produce no ANSI style/color codes in stdout"
    );

    assert!(
        stdout.contains("─ Read"),
        "should still contain tool call formatting without ANSI, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_tool_calls_have_ansi_colors() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env_remove("NO_COLOR")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("\x1b[1;34mRead\x1b[0m"),
        "Read tool call should have bold blue ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[1;33mBash\x1b[0m"),
        "Bash tool call should have bold yellow ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[1;35mEdit\x1b[0m"),
        "Edit tool call should have bold magenta ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[1;34mGrep\x1b[0m"),
        "Grep tool call should have bold blue ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[1;34mGlob\x1b[0m"),
        "Glob tool call should have bold blue ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[1;36mTodoWrite\x1b[0m"),
        "TodoWrite tool call should have bold cyan ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[37m"),
        "tool call details should have white ANSI codes, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_agent_text_has_bold_white_ansi() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env_remove("NO_COLOR")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("\x1b[37m\x1b[1mI'll start by studying the required files"),
        "agent text should have white+bold ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[37m\x1b[1mNow I can see the cleanup plan."),
        "agent text should have white+bold ANSI codes, got:\n{stdout}"
    );
}

// ---- Sentinel detection & exit codes ----

#[test]
fn iter_afk_detects_completion_file() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on completion file, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        stdout.contains("COMPLETE"),
        "should contain COMPLETE banner, got:\n{stdout}"
    );
    assert!(
        !tmp.path().join(".iter-complete").exists(),
        "sentinel file should be cleaned up after exit"
    );
}

#[test]
fn iter_afk_detects_nested_completion_file() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock =
        create_fixture_mock_with_nested_sentinel(tmp.path(), "complete.ndjson", "sub/project");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on nested completion file, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        stdout.contains("COMPLETE"),
        "should contain COMPLETE banner, got:\n{stdout}"
    );
    assert!(
        !tmp.path().join("sub/project/.iter-complete").exists(),
        "nested sentinel file should be cleaned up after exit"
    );
}

#[test]
fn iter_afk_exhausts_iterations_without_completion() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "should exit non-zero when no completion found"
    );
    assert_eq!(output.status.code(), Some(2), "should exit with code 2");
    assert!(
        stdout.contains("reached max iterations"),
        "should contain max iterations message, got:\n{stdout}"
    );
}

// ---- Banner formatting ----

#[test]
fn iter_startup_banner_uses_box_format() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ sgf Loop Starting"),
        "startup banner should use box format with 'sgf' name, got:\n{stdout}"
    );
}

#[test]
fn iter_iteration_banner_uses_box_format() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ Iteration 1 of"),
        "iteration banner should use box format, got:\n{stdout}"
    );
}

#[test]
fn iter_completion_banner_uses_box_format() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ sgf COMPLETE"),
        "completion banner should use box format with 'sgf' name, got:\n{stdout}"
    );
}

#[test]
fn iter_max_iterations_banner_uses_box_format() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "afk-session.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(2), "should exit with code 2");
    assert!(
        stdout.contains("╭─ sgf reached max iterations"),
        "max-iterations banner should use box format with 'sgf' name, got:\n{stdout}"
    );
}

// ---- Agent invocation flags ----

#[test]
fn iter_afk_invocation_passes_correct_flags() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_arg_capturing_mock(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(tmp.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    assert!(
        arg_lines.contains(&"--verbose"),
        "should pass --verbose, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--print"),
        "should pass --print in AFK mode, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--output-format"),
        "should pass --output-format in AFK mode, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"stream-json"),
        "should pass stream-json as output format, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--settings"),
        "should pass --settings, got args:\n{args}"
    );

    let last_arg = arg_lines.last().expect("should have args");
    assert!(
        last_arg.starts_with('@') && last_arg.ends_with("prompt.md"),
        "last arg should be @...prompt.md (file prompt), got: {last_arg}"
    );
}

#[test]
fn iter_interactive_invocation_passes_correct_flags() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_arg_capturing_mock(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg(prompt)
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(tmp.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    assert!(
        arg_lines.contains(&"--verbose"),
        "should pass --verbose, got args:\n{args}"
    );
    assert!(
        !arg_lines.contains(&"--print"),
        "should NOT pass --print in interactive mode, got args:\n{args}"
    );
    assert!(
        !arg_lines.contains(&"--output-format"),
        "should NOT pass --output-format in interactive mode, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--dangerously-skip-permissions"),
        "should pass --dangerously-skip-permissions, got args:\n{args}"
    );
    assert!(
        arg_lines.contains(&"--settings"),
        "should pass --settings, got args:\n{args}"
    );

    let last_arg = arg_lines.last().expect("should have args");
    assert!(
        last_arg.starts_with('@') && last_arg.ends_with("prompt.md"),
        "last arg should be @...prompt.md (file prompt), got: {last_arg}"
    );
}

// ---- Iteration limits ----

// Requires tracing-subscriber in springfield (pn-69cdd94b) to see the warn! output.
#[test]
#[ignore]
fn iter_iterations_clamped_at_1000() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a", "-n", "5000"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("clamping iterations to hard limit"),
        "should warn about clamping, got stderr:\n{stderr}"
    );
}

// ---- Terminal restoration ----

#[test]
fn iter_afk_terminal_restore_graceful_with_non_tty_stdin() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a", "-n", "2"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should complete 2 iterations — save/restore is a no-op when stdin is not a tty"
    );
}

fn open_pty() -> Option<(std::os::fd::OwnedFd, std::os::fd::OwnedFd)> {
    use std::os::fd::FromRawFd;
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master < 0 {
            return None;
        }
        if libc::grantpt(master) != 0 || libc::unlockpt(master) != 0 {
            libc::close(master);
            return None;
        }
        let slave_name = libc::ptsname(master);
        if slave_name.is_null() {
            libc::close(master);
            return None;
        }
        let slave = libc::open(slave_name, libc::O_RDWR | libc::O_NOCTTY);
        if slave < 0 {
            libc::close(master);
            return None;
        }
        Some((
            std::os::fd::OwnedFd::from_raw_fd(master),
            std::os::fd::OwnedFd::from_raw_fd(slave),
        ))
    }
}

fn termios_check_script() -> &'static str {
    r#"python3 -c "
import termios
attrs = termios.tcgetattr(0)
lflag = attrs[3]
isig = 'yes' if (lflag & termios.ISIG) else 'no'
icanon = 'yes' if (lflag & termios.ICANON) else 'no'
print(f'isig={isig},icanon={icanon}')
""#
}

fn termios_corrupt_script() -> &'static str {
    r#"python3 -c "
import termios
attrs = termios.tcgetattr(0)
attrs[3] &= ~(termios.ICANON | termios.ISIG)
termios.tcsetattr(0, termios.TCSANOW, attrs)
""#
}

#[test]
fn iter_interactive_restores_terminal_settings_after_agent_corrupts() {
    let Some((master, slave)) = open_pty() else {
        eprintln!("skipping: PTY allocation unavailable (sandboxed environment)");
        return;
    };
    let _master = master;

    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let captures_file = tmp.path().join("termios-captures.txt");

    let check_cmd = termios_check_script();
    let corrupt_cmd = termios_corrupt_script();
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\n{check_cmd} >> {captures} 2>/dev/null\n{corrupt_cmd} 2>/dev/null\n",
            captures = captures_file.display(),
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-n", "2"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(std::process::Stdio::from(slave)),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (iterations exhausted), stderr:\n{stderr}"
    );

    let captures = fs::read_to_string(&captures_file).expect("read termios captures");
    let lines: Vec<&str> = captures.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "should have 2 termios captures (one per iteration), got: {captures}"
    );
    assert_eq!(
        lines[0], "isig=yes,icanon=yes",
        "iteration 1 should start with ISIG and ICANON enabled"
    );
    assert_eq!(
        lines[1], "isig=yes,icanon=yes",
        "iteration 2 should have ISIG and ICANON restored after agent corrupted them"
    );
}

// ---- Multi-iteration session management ----

#[test]
fn iter_multi_iteration_generates_fresh_session_id_per_iteration() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_multi_iter_arg_capturing_mock(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a", "-n", "2"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (iterations exhausted), stderr:\n{stderr}"
    );

    let args1 = fs::read_to_string(tmp.path().join("captured-args/invocation-1.txt"))
        .expect("read invocation 1 args");
    let lines1: Vec<&str> = args1.lines().collect();

    let sid1_idx = lines1
        .iter()
        .position(|&a| a == "--session-id")
        .expect("iteration 1 should pass --session-id");
    let sid1_val = lines1[sid1_idx + 1];

    let args2 = fs::read_to_string(tmp.path().join("captured-args/invocation-2.txt"))
        .expect("read invocation 2 args");
    let lines2: Vec<&str> = args2.lines().collect();

    let sid2_idx = lines2
        .iter()
        .position(|&a| a == "--session-id")
        .expect("iteration 2 should pass --session-id");
    let sid2_val = lines2[sid2_idx + 1];

    assert_ne!(
        sid1_val, sid2_val,
        "iterations should use different session IDs"
    );
    assert!(
        sid2_val.len() == 36 && sid2_val.contains('-'),
        "iteration 2 session id should be a UUID, got: {sid2_val}"
    );
}

// ---- Signal handling through simple prompt mode ----

#[test]
fn iter_afk_double_ctrlc_aborts() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_slow_mock(tmp.path(), "afk-session.ndjson");

    let child = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args([prompt, "-a", "-n", "5"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C, got stdout:\n{stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Press Ctrl-C again to exit"),
        "should show double-press prompt on stderr, got:\n{stderr}"
    );
}

#[test]
fn iter_sigterm_exits_immediately() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        "#!/bin/bash\ntrap '' INT TERM\nsleep 10\n",
    );

    let child = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args([prompt])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).expect("send SIGTERM");

    let output = child
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 immediately on SIGTERM"
    );
}

#[test]
fn simple_prompt_interactive_mode_does_not_monitor_stdin() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    let prompt = setup_simple_prompt_test(tmp.path());

    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        "#!/bin/bash\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("SGF_FORCE_TERMINAL", "1")
            .env("RUST_LOG", "debug")
            .env("NO_COLOR", "1"),
    );

    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("monitor_stdin=false"),
        "interactive (non-AFK) mode must have monitor_stdin=false even with terminal, stderr:\n{stderr}"
    );
}

#[test]
fn simple_prompt_afk_mode_monitors_stdin_with_terminal() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    let prompt = setup_simple_prompt_test(tmp.path());

    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        "#!/bin/bash\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("SGF_FORCE_TERMINAL", "1")
            .env("RUST_LOG", "debug")
            .env("NO_COLOR", "1"),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("monitor_stdin=true"),
        "AFK mode with terminal should have monitor_stdin=true, stderr:\n{stderr}"
    );
}

// ---- E2E chain: sgf → cl → mock claude-wrapper ----

#[test]
fn iter_sgf_to_cl_e2e_invocation_chain() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let fixture_path = fixtures_dir().join("complete.ndjson");

    fs::create_dir_all(tmp.path().join(".sgf")).expect("create .sgf dir");
    fs::write(tmp.path().join(".sgf/MEMENTO.md"), "memento content").expect("write MEMENTO.md");
    fs::write(
        tmp.path().join(".sgf/BACKPRESSURE.md"),
        "backpressure content",
    )
    .expect("write BACKPRESSURE.md");

    let mock_dir = TempDir::new().expect("create mock dir");
    let args_file = tmp.path().join("captured-secret-args.txt");
    let mock_script = mock_dir.path().join("claude-wrapper-secret");
    let script_content = format!(
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > {args}\ncat {fixture}\ntouch {sentinel}\n",
        args = args_file.display(),
        fixture = fixture_path.display(),
        sentinel = tmp.path().join(".iter-complete").display(),
    );
    fs::write(&mock_script, script_content).expect("write mock claude-wrapper-secret");
    fs::set_permissions(&mock_script, fs::Permissions::from_mode(0o755)).expect("chmod");

    let cl_path = {
        let mut p = std::env::current_exe().unwrap();
        p.pop();
        p.pop();
        p.push("cl");
        p
    };
    assert!(
        cl_path.exists(),
        "cl binary not found at {}; build with `cargo build --workspace`",
        cl_path.display()
    );

    let path_var = format!(
        "{}:{}",
        mock_dir.path().to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", cl_path.to_str().unwrap())
            .env("PATH", &path_var)
            .env("HOME", tmp.path().to_str().unwrap())
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "sgf → cl → mock should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    assert!(
        args_file.exists(),
        "claude-wrapper-secret was never invoked — the chain is broken"
    );

    let captured = fs::read_to_string(&args_file).expect("read captured args");
    let lines: Vec<&str> = captured.lines().collect();

    let asp_idx = lines
        .iter()
        .position(|&l| l == "--append-system-prompt")
        .expect("cl should inject --append-system-prompt for context files");
    let study_arg = lines[asp_idx + 1];
    assert!(
        study_arg.contains("study @") && study_arg.contains("MEMENTO.md"),
        "should contain study @...MEMENTO.md, got: {study_arg}"
    );
    assert!(
        study_arg.contains("BACKPRESSURE.md"),
        "should contain BACKPRESSURE.md, got: {study_arg}"
    );

    assert!(
        lines.contains(&"--verbose"),
        "should forward --verbose, got:\n{captured}"
    );
    assert!(
        lines.contains(&"--dangerously-skip-permissions"),
        "should forward --dangerously-skip-permissions, got:\n{captured}"
    );
    assert!(
        lines.contains(&"--print"),
        "should forward --print (AFK mode), got:\n{captured}"
    );
    assert!(
        lines.contains(&"stream-json"),
        "should forward stream-json output format, got:\n{captured}"
    );

    assert!(
        lines
            .iter()
            .any(|&l| l.starts_with('@') && l.ends_with("prompt.md")),
        "should forward prompt arg through the chain, got:\n{captured}"
    );
}

// ---- Auto-push through simple prompt mode ----

#[test]
fn iter_auto_push_pushes_when_head_changes() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());

    let bare_dir = TempDir::new().expect("create bare repo dir");
    let run_git = |args: &[&str], cwd: &Path| {
        let _permit = SGF_PERMITS
            .acquire_timeout(Duration::from_secs(60))
            .expect("semaphore timed out");
        ChildGuard::spawn(
            Command::new("git")
                .args(args)
                .current_dir(cwd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .expect("spawn git")
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("git command")
    };

    run_git(&["init", "--bare"], bare_dir.path());
    run_git(
        &["remote", "add", "origin", bare_dir.path().to_str().unwrap()],
        tmp.path(),
    );
    run_git(&["push", "-u", "origin", "master"], tmp.path());

    let fixture_path = fixtures_dir().join("complete.ndjson");
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "cat {fixture}\n",
                "echo 'auto-push test' > change.txt\n",
                "git -c user.name=test -c user.email=test@test.com add change.txt\n",
                "git -c user.name=test -c user.email=test@test.com commit -m 'test commit'\n",
                "touch .iter-complete\n",
            ),
            fixture = fixture_path.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    assert!(
        stdout.contains("New commits detected, pushing..."),
        "should detect new commits and push, got stdout:\n{stdout}"
    );

    let remote_log = run_git(&["log", "--oneline"], bare_dir.path());
    let remote_log_str = String::from_utf8_lossy(&remote_log.stdout);
    assert!(
        remote_log_str.contains("test commit"),
        "remote should contain the pushed commit, got:\n{remote_log_str}"
    );
}

#[test]
fn iter_auto_push_disabled_with_no_push_flag() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());

    let fixture_path = fixtures_dir().join("complete.ndjson");
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "cat {fixture}\n",
                "echo 'no-push test' > change.txt\n",
                "git -c user.name=test -c user.email=test@test.com add change.txt\n",
                "git -c user.name=test -c user.email=test@test.com commit -m 'test commit'\n",
                "touch .iter-complete\n",
            ),
            fixture = fixture_path.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a", "--no-push"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );

    assert!(
        !stdout.contains("New commits detected, pushing..."),
        "should NOT push when --no-push is used, got stdout:\n{stdout}"
    );
}

// ---- Log file handling ----

#[test]
fn iter_simple_mode_creates_log_file() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "complete.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .stdin(Stdio::null()),
    );

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );

    let logs_dir = tmp.path().join(".sgf/logs");
    if logs_dir.exists() {
        let log_files: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
            .collect();
        assert!(
            !log_files.is_empty(),
            "simple prompt mode should create a log file in .sgf/logs/"
        );

        let log_content = fs::read_to_string(log_files[0].path()).expect("read log file");
        assert!(
            log_content.contains("Iteration 1 of 1"),
            "log should contain iteration banner, got: {log_content}"
        );
    }
}

// ---- SGF_AGENT_COMMAND override (test-harness spec §7) ----

#[test]
fn agent_command_env_overrides_default_binary_simple_mode() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let marker = mock_dir.path().join("custom_agent_was_invoked");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "custom_agent.sh",
        &format!(
            "#!/bin/sh\necho \"custom-agent-marker\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            marker.display()
        ),
    );

    fs::write(tmp.path().join("task.md"), "Test task.\n").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf should succeed with custom agent, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        marker.exists(),
        "custom agent marker file should exist — SGF_AGENT_COMMAND was not used"
    );
    let content = fs::read_to_string(&marker).unwrap();
    assert!(
        content.contains("custom-agent-marker"),
        "marker should contain expected content, got: {content}"
    );
}

#[test]
fn agent_command_env_overrides_default_binary_cursus_mode() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let marker = mock_dir.path().join("cursus_agent_invoked");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "custom_agent.sh",
        &format!(
            "#!/bin/sh\necho \"cursus-agent-marker\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            marker.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf cursus should succeed with custom agent, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        marker.exists(),
        "custom agent marker file should exist in cursus mode — SGF_AGENT_COMMAND was not used"
    );
    let content = fs::read_to_string(&marker).unwrap();
    assert!(
        content.contains("cursus-agent-marker"),
        "marker should contain expected content, got: {content}"
    );
}

// ---- Usage stats display from result events ----

#[test]
fn iter_afk_displays_usage_stats_from_result_event() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "long-tool-result.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on completion, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        stdout.contains("Input: 8500 tokens"),
        "should display input token count from usage stats, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Output: 420 tokens"),
        "should display output token count from usage stats, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_usage_stats_with_color_uses_dim_ansi() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "long-tool-result.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env_remove("NO_COLOR")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on completion, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        stdout.contains("\x1b[2m"),
        "usage stats should use dim ANSI code, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Input: 8500 tokens"),
        "should display input token count, got:\n{stdout}"
    );
}

// ---- Tool result truncation ----

#[test]
fn iter_afk_long_tool_result_does_not_leak_to_stdout() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());
    let mock = create_fixture_mock_with_sentinel(tmp.path(), "long-tool-result.ndjson");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on completion, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );

    for i in 1..=25 {
        assert!(
            !stdout.contains(&format!("line {i} of output")),
            "tool result line {i} should NOT appear in AFK stdout, got:\n{stdout}"
        );
    }

    assert!(
        stdout.contains("Let me read the large file."),
        "agent text should still appear, got:\n{stdout}"
    );
    assert!(
        stdout.contains("The file has been read. Task complete."),
        "agent text should still appear, got:\n{stdout}"
    );
}

#[test]
fn iter_afk_result_without_usage_displays_result_text() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());

    let fixture_path = fixtures_dir().join("complete.ndjson");
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/bash\ncat {}\ntouch .iter-complete\n",
            fixture_path.display()
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null()),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 on completion, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        !stdout.contains("Input:") && !stdout.contains("tokens"),
        "result without usage should NOT display token stats, got:\n{stdout}"
    );
}

// ===========================================================================
// --skip-preflight flag
// ===========================================================================

#[test]
fn skip_preflight_flag_skips_recovery_and_export() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Create a stale run directory with dead PID so pre_launch_recovery
    // would trigger git checkout/clean if preflight ran.
    let stale_run_dir = tmp.path().join(".sgf/run/stale-build-20260101T000000");
    fs::create_dir_all(&stale_run_dir).unwrap();
    fs::write(
        stale_run_dir.join("stale-build-20260101T000000.pid"),
        "4000000",
    )
    .unwrap();
    let stale_meta = serde_json::json!({
        "run_id": "stale-build-20260101T000000",
        "cursus": "build",
        "status": "running",
        "current_iter": "build",
        "current_iter_index": 0,
        "iters_completed": [],
        "spec": null,
        "mode_override": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    });
    fs::write(
        stale_run_dir.join("meta.json"),
        serde_json::to_string_pretty(&stale_meta).unwrap(),
    )
    .unwrap();

    // Dirty the tracked README — recovery would restore it via git checkout
    fs::write(tmp.path().join("README.md"), "DIRTY CONTENT").unwrap();
    // Create untracked file — recovery would remove it via git clean
    fs::write(tmp.path().join("untracked_marker.txt"), "marker").unwrap();

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a", "--skip-preflight"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(
        output.status.success(),
        "sgf build --skip-preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Tracked file should still be dirty (git checkout did NOT run)
    assert_eq!(
        fs::read_to_string(tmp.path().join("README.md")).unwrap(),
        "DIRTY CONTENT",
        "README.md should remain dirty when --skip-preflight is passed"
    );

    // Untracked file should still exist (git clean did NOT run)
    assert!(
        tmp.path().join("untracked_marker.txt").exists(),
        "untracked file should still exist when --skip-preflight is passed"
    );

    // pn export should not have run (whole run_pre_launch is skipped)
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("pn export"),
        "pn export should not run with --skip-preflight, got:\n{stderr}"
    );
}

#[test]
fn without_skip_preflight_recovery_and_daemons_attempted() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Create a stale run directory with dead PID
    let stale_run_dir = tmp.path().join(".sgf/run/stale-build-20260101T000000");
    fs::create_dir_all(&stale_run_dir).unwrap();
    fs::write(
        stale_run_dir.join("stale-build-20260101T000000.pid"),
        "4000000",
    )
    .unwrap();
    let stale_meta = serde_json::json!({
        "run_id": "stale-build-20260101T000000",
        "cursus": "build",
        "status": "running",
        "current_iter": "build",
        "current_iter_index": 0,
        "iters_completed": [],
        "spec": null,
        "mode_override": null,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    });
    fs::write(
        stale_run_dir.join("meta.json"),
        serde_json::to_string_pretty(&stale_meta).unwrap(),
    )
    .unwrap();

    // Custom mock pn/fm that log invocations to a file
    let log_dir = TempDir::new().unwrap();
    let pn_log = log_dir.path().join("pn_calls.log");
    let fm_log = log_dir.path().join("fm_calls.log");

    let mock_bin_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_bin_dir.path(),
        "pn",
        &format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 0\n",
            pn_log.display()
        ),
    );
    create_mock_script(
        mock_bin_dir.path(),
        "fm",
        &format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 0\n",
            fm_log.display()
        ),
    );
    let custom_path = format!(
        "{}:{}",
        mock_bin_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mock_agent_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_agent_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("PATH", &custom_path)
            .env_remove("SGF_SKIP_PREFLIGHT")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
    assert!(
        output.status.success(),
        "sgf build without --skip-preflight failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Recovery should have run: stale PID file removed
    assert!(
        !stale_run_dir
            .join("stale-build-20260101T000000.pid")
            .exists(),
        "stale PID file should be removed when preflight runs"
    );

    // pn daemon status should have been called (daemons checked)
    let pn_calls = fs::read_to_string(&pn_log).unwrap_or_default();
    assert!(
        pn_calls.contains("daemon status"),
        "pn daemon status should be called without --skip-preflight, got:\n{pn_calls}"
    );

    // fm daemon status should have been called
    let fm_calls = fs::read_to_string(&fm_log).unwrap_or_default();
    assert!(
        fm_calls.contains("daemon status"),
        "fm daemon status should be called without --skip-preflight, got:\n{fm_calls}"
    );

    // pn export should have been called
    assert!(
        pn_calls.contains("export"),
        "pn export should be called without --skip-preflight, got:\n{pn_calls}"
    );
}

fn read_simple_prompt_metadata(root: &Path) -> serde_json::Value {
    let run_dir = root.join(".sgf/run");
    let json_files: Vec<_> = fs::read_dir(&run_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            p.extension().and_then(|x| x.to_str()) == Some("json")
        })
        .collect();
    assert_eq!(
        json_files.len(),
        1,
        "expected exactly 1 session metadata JSON, found {}",
        json_files.len()
    );
    let contents = fs::read_to_string(json_files[0].path()).unwrap();
    serde_json::from_str(&contents).unwrap()
}

#[test]
fn simple_prompt_afk_writes_session_metadata() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("agent_args.txt");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("task.md")
            .arg("-a")
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "sgf simple prompt should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_simple_prompt_metadata(tmp.path());

    assert!(
        meta["loop_id"].as_str().unwrap().starts_with("simple-"),
        "loop_id should start with simple-, got: {}",
        meta["loop_id"]
    );
    assert_eq!(meta["mode"], "afk");
    assert_eq!(meta["status"], "completed");
    assert_eq!(meta["stage"], "simple");
    assert!(meta["cursus"].is_null(), "cursus should be null");
    assert!(meta["spec"].is_null(), "spec should be null");

    let iterations = meta["iterations"].as_array().unwrap();
    assert_eq!(iterations.len(), 1, "should have 1 iteration record");
    let session_id = iterations[0]["session_id"].as_str().unwrap();
    assert!(
        !session_id.is_empty() && session_id.contains('-'),
        "session_id should be a UUID, got: {session_id}"
    );
    assert_eq!(iterations[0]["iteration"], 1);

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(session_id),
        "agent should receive same session_id, got: {args}"
    );
}

#[test]
fn simple_prompt_interactive_writes_session_metadata() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_cl_dir = TempDir::new().unwrap();
    let args_file = mock_cl_dir.path().join("agent_args.txt");
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            "#!/bin/sh\necho \"$@\" > \"{}\"\ntouch \"${{PWD}}/.iter-complete\"\nexit 0\n",
            args_file.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .arg("task.md")
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

    assert!(
        output.status.success(),
        "sgf simple prompt interactive should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_simple_prompt_metadata(tmp.path());

    assert!(
        meta["loop_id"].as_str().unwrap().starts_with("simple-"),
        "loop_id should start with simple-, got: {}",
        meta["loop_id"]
    );
    assert_eq!(meta["mode"], "interactive");
    assert_eq!(meta["status"], "completed");

    let iterations = meta["iterations"].as_array().unwrap();
    assert_eq!(iterations.len(), 1);
    let session_id = iterations[0]["session_id"].as_str().unwrap();
    assert!(!session_id.is_empty() && session_id.contains('-'));

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(session_id),
        "cl should receive same session_id, got: {args}"
    );
}

#[test]
fn simple_prompt_exhausted_writes_metadata_with_correct_status() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 0\n");

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a", "-n", "2"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (exhausted), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta = read_simple_prompt_metadata(tmp.path());
    assert_eq!(meta["status"], "exhausted");
    assert_eq!(meta["iterations_total"], 2);

    let iterations = meta["iterations"].as_array().unwrap();
    assert_eq!(
        iterations.len(),
        2,
        "should have 2 iteration records for 2 exhausted iterations"
    );
    assert_eq!(iterations[0]["iteration"], 1);
    assert_eq!(iterations[1]["iteration"], 2);

    let sid1 = iterations[0]["session_id"].as_str().unwrap();
    let sid2 = iterations[1]["session_id"].as_str().unwrap();
    assert_ne!(sid1, sid2, "each iteration should have a unique session_id");
}

// ===========================================================================
// Pre-launch: Daemon lifecycle
// ===========================================================================

#[test]
fn daemon_lifecycle_daemons_reachable_and_loop_runs() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    // Mock pn/fm that handle daemon status (exit 0 = reachable) and export
    create_mock_script(mock_dir.path(), "pn", "#!/bin/sh\nexit 0\n");
    create_mock_script(mock_dir.path(), "fm", "#!/bin/sh\nexit 0\n");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );
    let mock_path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let prompt = tmp.path().join("task.md");
    fs::write(&prompt, "test task").unwrap();

    // Run WITHOUT SGF_SKIP_PREFLIGHT so daemon lifecycle code executes
    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a", "-n", "2"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path)
            .env_remove("SGF_SKIP_PREFLIGHT"),
    );

    assert!(
        output.status.success(),
        "loop should complete after daemon pre-launch, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ===========================================================================
// Pre-launch: Data export
// ===========================================================================

#[test]
fn data_export_runs_pn_and_fm_export_before_loop() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let marker_dir = TempDir::new().unwrap();

    // Mock pn: writes marker on "export", exits 0 on everything else (including daemon status)
    create_mock_script(
        mock_dir.path(),
        "pn",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"export\" ]; then touch \"{}/pn_export\"; fi\nexit 0\n",
            marker_dir.path().display()
        ),
    );
    create_mock_script(
        mock_dir.path(),
        "fm",
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"export\" ]; then touch \"{}/fm_export\"; fi\nexit 0\n",
            marker_dir.path().display()
        ),
    );
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );
    let mock_path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let prompt = tmp.path().join("task.md");
    fs::write(&prompt, "test task").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a", "-n", "1"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path)
            .env_remove("SGF_SKIP_PREFLIGHT"),
    );

    assert!(
        output.status.success(),
        "sgf should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        marker_dir.path().join("pn_export").exists(),
        "pn export should have been called before loop"
    );
    assert!(
        marker_dir.path().join("fm_export").exists(),
        "fm export should have been called before loop"
    );
}

// ===========================================================================
// Pre-launch: Export failure non-fatal
// ===========================================================================

#[test]
fn export_failure_does_not_block_loop() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();

    // Mock pn: daemon status → exit 0, export → exit 1 with stderr
    create_mock_script(
        mock_dir.path(),
        "pn",
        "#!/bin/sh\nif [ \"$1\" = \"export\" ]; then echo 'pn export error' >&2; exit 1; fi\nexit 0\n",
    );
    create_mock_script(
        mock_dir.path(),
        "fm",
        "#!/bin/sh\nif [ \"$1\" = \"export\" ]; then echo 'fm export error' >&2; exit 1; fi\nexit 0\n",
    );
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );
    let mock_path = format!(
        "{}:{}",
        mock_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let prompt = tmp.path().join("task.md");
    fs::write(&prompt, "test task").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a", "-n", "1"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path)
            .env_remove("SGF_SKIP_PREFLIGHT"),
    );

    assert!(
        output.status.success(),
        "loop should still complete despite export failures, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pn export") || stderr.contains("export"),
        "stderr should contain export failure warning, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// AFK post-result timeout
// ---------------------------------------------------------------------------

#[test]
fn afk_post_result_timeout_kills_hung_agent() {
    let tmp = setup_test_dir();
    let prompt = setup_simple_prompt_test(tmp.path());

    let result_json = r#"{"type":"result","result":"Done.","session_id":"s1","usage":{"input_tokens":100,"output_tokens":200}}"#;
    let mock = create_mock_script(
        tmp.path(),
        "mock_agent.sh",
        &format!("#!/bin/sh\necho '{}'\nsleep 300\n", result_json),
    );

    let timeout_secs = 3;
    let start = std::time::Instant::now();
    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args([prompt, "-a"])
            .env("SGF_AGENT_COMMAND", &mock)
            .env("SGF_POST_RESULT_TIMEOUT_SECS", timeout_secs.to_string())
            .env("RUST_LOG", "springfield=warn")
            .env_remove("SGF_TEST_NO_SETSID")
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );
    let elapsed = start.elapsed();

    assert!(
        elapsed >= Duration::from_secs(timeout_secs),
        "should have waited for the timeout period, only took {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(timeout_secs + 10),
        "should have killed hung process within reasonable time after timeout, took {elapsed:?}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("did not exit after result event"),
        "stderr should contain post-result timeout warning, got:\n{stderr}"
    );
}

// ===========================================================================
// Retry config parsing and defaults (cursus)
// ===========================================================================

#[test]
fn cursus_custom_retry_config_parses_and_runs() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 5\n",
            "interval_secs = 600\n",
            "max_duration_secs = 86400\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
    );

    assert!(
        output.status.success(),
        "cursus with custom [retry] should run successfully: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore]
fn cursus_custom_retry_config_overrides_defaults_during_execution() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // immediate=2, interval=60, max_duration=120 → max_attempts = 2 + 120/60 = 4
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 2\n",
            "interval_secs = 60\n",
            "max_duration_secs = 120\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: first call sleeps 6s (past STARTUP_ERROR_THRESHOLD) then fails;
    // second call succeeds.
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("state");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  exit 0\n",
                "fi\n",
                "touch \"{}\"\n",
                "sleep 6\n",
                "exit 1\n",
            ),
            state_file.display(),
            state_file.display()
        ),
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );

    assert!(
        output.status.success(),
        "cursus should succeed after retry: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Custom config: immediate=2, interval=60, max_duration=120 → max_attempts=4
    assert!(
        stderr.contains("attempt 1/4"),
        "retry message should reflect custom config (max_attempts=4), got stderr:\n{stderr}"
    );
}

#[test]
#[ignore]
fn cursus_default_retry_config_when_section_omitted() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // No [retry] section — defaults apply: immediate=3, interval=300, max_duration=43200
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: first call sleeps 6s then fails; second call succeeds.
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("state");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  exit 0\n",
                "fi\n",
                "touch \"{}\"\n",
                "sleep 6\n",
                "exit 1\n",
            ),
            state_file.display(),
            state_file.display()
        ),
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );

    assert!(
        output.status.success(),
        "cursus should succeed after retry: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Default config: immediate=3, interval=300, max_duration=43200 → max_attempts=147
    assert!(
        stderr.contains("attempt 1/147"),
        "retry message should reflect default config (max_attempts=147), got stderr:\n{stderr}"
    );
}

// ===========================================================================
// Auto-retry behavior (binary-level)
// ===========================================================================

#[test]
#[ignore]
fn retry_retryable_failure_triggers_retry() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 3\n",
            "interval_secs = 60\n",
            "max_duration_secs = 120\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: first run sleeps 6s (past STARTUP_ERROR_THRESHOLD=5s) then exits 1;
    // second run succeeds by touching .iter-complete.
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("state");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  exit 0\n",
                "fi\n",
                "touch \"{}\"\n",
                "sleep 6\n",
                "exit 1\n",
            ),
            state_file.display(),
            state_file.display()
        ),
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );

    assert!(
        output.status.success(),
        "retryable failure (exit 1 after 6s) should trigger retry and succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("retrying immediately"),
        "should see retry message in stderr, got:\n{stderr}"
    );
}

#[test]
#[ignore]
fn retry_non_retryable_fast_exit_does_not_retry() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 3\n",
            "interval_secs = 60\n",
            "max_duration_secs = 120\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "limit = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: exits 1 immediately (within STARTUP_ERROR_THRESHOLD=5s) — not retryable.
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 1\n");

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(15),
    );

    assert!(
        !output.status.success(),
        "fast exit (< 5s) should NOT be retried and should fail"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("retrying"),
        "should NOT see any retry message for fast exit, got:\n{stderr}"
    );
}

#[test]
#[ignore]
fn retry_signal_killed_process_triggers_retry() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 3\n",
            "interval_secs = 60\n",
            "max_duration_secs = 120\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: first run kills itself with SIGKILL (exit_code=None → retryable);
    // second run succeeds.
    let mock_dir = TempDir::new().unwrap();
    let state_file = mock_dir.path().join("state");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "if [ -f \"{}\" ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  exit 0\n",
                "fi\n",
                "touch \"{}\"\n",
                "kill -9 $$\n",
            ),
            state_file.display(),
            state_file.display()
        ),
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );

    assert!(
        output.status.success(),
        "signal-killed process (SIGKILL) should trigger retry and succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("retrying"),
        "should see retry message after signal kill, got:\n{stderr}"
    );
}

#[test]
#[ignore]
fn retry_exit_130_does_not_retry() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 3\n",
            "interval_secs = 60\n",
            "max_duration_secs = 120\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "limit = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: sleeps past threshold then exits 130 (SIGINT convention) — not retryable.
    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\nsleep 6\nexit 130\n",
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(30),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("retrying"),
        "exit 130 (SIGINT) should NOT trigger retry, got:\n{stderr}"
    );
}

#[test]
#[ignore]
fn retry_immediate_retries_exhaust_before_backoff() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    // immediate=2, interval=1 → first 2 retries are immediate, 3rd is backoff (1s wait)
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[retry]\n",
            "immediate = 2\n",
            "interval_secs = 1\n",
            "max_duration_secs = 300\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );
    fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
    fs::write(tmp.path().join(".sgf/prompts/build.md"), "build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add prompt");

    // Mock agent: fails (sleep 6 + exit 1) for first 3 calls, succeeds on 4th.
    // Call 1 = original run (fails), call 2 = attempt 1 (immediate, fails),
    // call 3 = attempt 2 (immediate, fails), call 4 = attempt 3 (backoff, succeeds).
    let mock_dir = TempDir::new().unwrap();
    let counter_file = mock_dir.path().join("counter");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "COUNTER_FILE=\"{}\"\n",
                "if [ ! -f \"$COUNTER_FILE\" ]; then\n",
                "  echo 1 > \"$COUNTER_FILE\"\n",
                "  sleep 6\n",
                "  exit 1\n",
                "fi\n",
                "COUNT=$(cat \"$COUNTER_FILE\")\n",
                "COUNT=$((COUNT + 1))\n",
                "echo $COUNT > \"$COUNTER_FILE\"\n",
                "if [ $COUNT -le 3 ]; then\n",
                "  sleep 6\n",
                "  exit 1\n",
                "fi\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "exit 0\n",
            ),
            counter_file.display()
        ),
    );

    let output = run_sgf_timeout(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::null()),
        Duration::from_secs(60),
    );

    assert!(
        output.status.success(),
        "should succeed after exhausting immediate retries: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // max_attempts = 2 + 300/1 = 302
    // Attempt 1 should be immediate, attempt 2 should be immediate,
    // attempt 3 should be backoff (interval_secs=1 → "retrying in 0m")
    assert!(
        stderr.contains("retrying immediately (attempt 1/302)"),
        "first retry should be immediate, got:\n{stderr}"
    );
    assert!(
        stderr.contains("retrying immediately (attempt 2/302)"),
        "second retry should be immediate, got:\n{stderr}"
    );
    assert!(
        stderr.contains("attempt 3/302)"),
        "third retry should exist as backoff attempt, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("retrying immediately (attempt 3/302)"),
        "third retry should NOT be immediate (should be backoff), got:\n{stderr}"
    );
}

// ===========================================================================
// Cursus: programmatic NDJSON event emission (binary-level)
// ===========================================================================

fn parse_ndjson_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn event_types(events: &[serde_json::Value]) -> Vec<&str> {
    events.iter().filter_map(|e| e["event"].as_str()).collect()
}

#[test]
fn cursus_programmatic_single_iter_afk_event_ordering() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Single iter event test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 5\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add cursus and prompt");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    assert!(
        types.len() >= 4,
        "should emit at least 4 events, got {}: {types:?}",
        types.len()
    );
    assert_eq!(types[0], "run_start", "first event should be run_start");
    assert_eq!(types[1], "iter_start", "second event should be iter_start");

    // iter_complete and run_complete should appear in order (possibly with other events between)
    let iter_complete_pos = types.iter().position(|&t| t == "iter_complete");
    let run_complete_pos = types.iter().position(|&t| t == "run_complete");
    assert!(
        iter_complete_pos.is_some(),
        "should emit iter_complete, got: {types:?}"
    );
    assert!(
        run_complete_pos.is_some(),
        "should emit run_complete, got: {types:?}"
    );
    assert!(
        iter_complete_pos.unwrap() < run_complete_pos.unwrap(),
        "iter_complete should come before run_complete"
    );

    // Validate run_start content
    let run_start = &events[0];
    assert_eq!(run_start["cursus"], "build");
    assert!(
        run_start["run_id"].as_str().unwrap().starts_with("build-"),
        "run_id should start with cursus name"
    );
    let iters = run_start["iters"].as_array().unwrap();
    assert_eq!(iters.len(), 1);
    assert_eq!(iters[0]["name"], "build");
    assert_eq!(iters[0]["mode"], "afk");
    assert_eq!(iters[0]["iterations"], 5);

    // Validate iter_start content
    let iter_start = &events[1];
    assert_eq!(iter_start["iter"], "build");
    assert_eq!(iter_start["mode"], "afk");
    assert_eq!(iter_start["iteration"], 5);
    assert!(
        iter_start["session_id"].as_str().is_some(),
        "iter_start should include session_id"
    );

    // Validate iter_complete content
    let ic = &events[iter_complete_pos.unwrap()];
    assert_eq!(ic["iter"], "build");
    assert_eq!(ic["outcome"], "complete");
    assert_eq!(ic["iterations_used"], 5);

    // Validate run_complete content
    let rc = &events[run_complete_pos.unwrap()];
    assert_eq!(rc["status"], "completed");
    assert!(rc["resume_command"].as_str().unwrap().contains("--resume"));

    // No turn events should be emitted for AFK iters
    assert!(
        !types.contains(&"turn"),
        "AFK iter should not emit turn events, got: {types:?}"
    );
}

#[test]
fn cursus_programmatic_multi_iter_transition_events() {
    let tmp = setup_multi_iter_test();

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "mkdir -p \"$SGF_RUN_CONTEXT\" 2>/dev/null\n",
            "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
            "  echo 'summary' > \"$SGF_RUN_CONTEXT/discuss-summary.md\"\n",
            "fi\n",
            "if echo \"$PROMPT\" | grep -q 'draft.md'; then\n",
            "  echo 'draft' > \"$SGF_RUN_CONTEXT/draft-presentation.md\"\n",
            "fi\n",
            "touch \"${PWD}/.iter-complete\"\n",
            "exit 0\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "multi-iter should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    // Should have: run_start, then for each iter: iter_start, iter_complete, context_produced(?),
    // transition, ... and finally run_complete
    assert_eq!(types[0], "run_start");

    // Collect transition events
    let transitions: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["event"] == "transition")
        .collect();

    // Pipeline: discuss -> draft -> review -> approve (4 iters, 3 transitions)
    assert_eq!(
        transitions.len(),
        3,
        "should have 3 transitions for 4-iter happy path, got: {types:?}"
    );

    assert_eq!(transitions[0]["from_iter"], "discuss");
    assert_eq!(transitions[0]["to_iter"], "draft");
    assert_eq!(transitions[0]["reason"], "complete");

    assert_eq!(transitions[1]["from_iter"], "draft");
    assert_eq!(transitions[1]["to_iter"], "review");
    assert_eq!(transitions[1]["reason"], "complete");

    // review has next = "approve", so the transition should be review -> approve
    assert_eq!(transitions[2]["from_iter"], "review");
    assert_eq!(transitions[2]["to_iter"], "approve");
    assert_eq!(transitions[2]["reason"], "complete");

    // Validate iter_start events match the expected iter sequence
    let iter_starts: Vec<&str> = events
        .iter()
        .filter(|e| e["event"] == "iter_start")
        .filter_map(|e| e["iter"].as_str())
        .collect();
    assert_eq!(
        iter_starts,
        vec!["discuss", "draft", "review", "approve"],
        "iter_start events should match expected sequence"
    );

    // Validate iter_complete events match the expected iter sequence
    let iter_completes: Vec<&str> = events
        .iter()
        .filter(|e| e["event"] == "iter_complete")
        .filter_map(|e| e["iter"].as_str())
        .collect();
    assert_eq!(
        iter_completes,
        vec!["discuss", "draft", "review", "approve"],
        "iter_complete events should match expected sequence"
    );

    // run_complete should be last
    assert_eq!(
        types.last().unwrap(),
        &"run_complete",
        "run_complete should be the last event"
    );
    let rc = events.last().unwrap();
    assert_eq!(rc["status"], "completed");
}

#[test]
fn cursus_programmatic_stall_emits_stall_event() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Stall event test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"discuss\"\n",
            "prompt = \"discuss.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"draft\"\n",
            "prompt = \"draft.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 2\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("discuss.md"), "discuss prompt\n").unwrap();
    fs::write(prompts_dir.join("draft.md"), "draft prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall event cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
            "  touch \"${PWD}/.iter-complete\"\n",
            "  exit 0\n",
            "fi\n",
            "# draft does NOT touch sentinel -> exhaustion\n",
            "exit 2\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "stalled pipeline should exit 2: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    // Should contain a stall event
    let stall_events: Vec<&serde_json::Value> =
        events.iter().filter(|e| e["event"] == "stall").collect();
    assert_eq!(
        stall_events.len(),
        1,
        "should emit exactly one stall event, got types: {types:?}"
    );

    let stall = stall_events[0];
    assert_eq!(
        stall["iter"], "draft",
        "stall should reference the draft iter"
    );
    assert_eq!(
        stall["iterations_attempted"], 2,
        "stall should report 2 iterations attempted"
    );
    let actions = stall["actions"].as_array().unwrap();
    assert!(
        actions.iter().any(|a| a == "retry"),
        "stall actions should include retry"
    );
    assert!(
        actions.iter().any(|a| a == "skip"),
        "stall actions should include skip"
    );
    assert!(
        actions.iter().any(|a| a == "abort"),
        "stall actions should include abort"
    );

    // run_complete should follow stall with status "stalled"
    let rc_events: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["event"] == "run_complete")
        .collect();
    assert_eq!(rc_events.len(), 1, "should emit run_complete after stall");
    assert_eq!(rc_events[0]["status"], "stalled");

    // Stall should come before run_complete
    let stall_pos = types.iter().position(|&t| t == "stall").unwrap();
    let rc_pos = types.iter().position(|&t| t == "run_complete").unwrap();
    assert!(stall_pos < rc_pos, "stall should come before run_complete");
}

#[test]
fn cursus_programmatic_afk_piped_stdin_no_turn_events() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"AFK piped stdin test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 3\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add cursus and prompt");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    // Pipe stdin (triggers programmatic mode via !is_terminal), no --output-format json
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    // Close stdin immediately (no input provided)
    drop(guard.child_mut().stdin.take());

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf");
    drop(_permit);

    assert!(
        output.status.success(),
        "should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    // Should emit iter_start and iter_complete
    assert!(
        types.contains(&"iter_start"),
        "should emit iter_start, got: {types:?}"
    );
    assert!(
        types.contains(&"iter_complete"),
        "should emit iter_complete, got: {types:?}"
    );

    // No turn events for AFK iters
    assert!(
        !types.contains(&"turn"),
        "AFK iter with piped stdin should NOT emit turn events, got: {types:?}"
    );

    // Should still emit run_start and run_complete
    assert_eq!(types[0], "run_start");
    assert_eq!(types.last().unwrap(), &"run_complete");
}

#[test]
fn cursus_programmatic_context_produced_and_consumed_events() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Context events test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"gather\"\n",
            "prompt = \"gather.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "produces = \"gathered-data\"\n",
            "\n",
            "[[iter]]\n",
            "name = \"process\"\n",
            "prompt = \"process.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "consumes = [\"gathered-data\"]\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("gather.md"), "gather prompt\n").unwrap();
    fs::write(prompts_dir.join("process.md"), "process prompt\n").unwrap();
    git_add_commit(tmp.path(), "add context events cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'gather.md'; then\n",
            "  mkdir -p \"$SGF_RUN_CONTEXT\"\n",
            "  echo 'gathered data content' > \"$SGF_RUN_CONTEXT/gathered-data.md\"\n",
            "fi\n",
            "touch \"${PWD}/.iter-complete\"\n",
            "exit 0\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "context events test should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    // Should contain context_produced event
    let produced_events: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["event"] == "context_produced")
        .collect();
    assert_eq!(
        produced_events.len(),
        1,
        "should emit exactly one context_produced event, got types: {types:?}"
    );
    assert_eq!(produced_events[0]["key"], "gathered-data");
    assert_eq!(produced_events[0]["iter"], "gather");

    // Should contain context_consumed event
    let consumed_events: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["event"] == "context_consumed")
        .collect();
    assert_eq!(
        consumed_events.len(),
        1,
        "should emit exactly one context_consumed event, got types: {types:?}"
    );
    assert_eq!(consumed_events[0]["key"], "gathered-data");
    assert_eq!(consumed_events[0]["from_iter"], "gather");

    // context_produced should come after gather's iter_complete
    let gather_complete_pos = events
        .iter()
        .position(|e| e["event"] == "iter_complete" && e["iter"] == "gather")
        .unwrap();
    let produced_pos = events
        .iter()
        .position(|e| e["event"] == "context_produced")
        .unwrap();
    assert!(
        produced_pos > gather_complete_pos,
        "context_produced should come after gather's iter_complete"
    );

    // context_consumed should come before process's iter_start
    let process_start_pos = events
        .iter()
        .position(|e| e["event"] == "iter_start" && e["iter"] == "process")
        .unwrap();
    let consumed_pos = events
        .iter()
        .position(|e| e["event"] == "context_consumed")
        .unwrap();
    assert!(
        consumed_pos < process_start_pos,
        "context_consumed should come before process's iter_start"
    );
}

// ===========================================================================
// Cursus: programmatic turn-by-turn and stall resume (binary-level)
// ===========================================================================

#[test]
fn cursus_programmatic_turn_by_turn_interactive_iter() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("chat.toml"),
        concat!(
            "description = \"Turn-by-turn test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"chat\"\n",
            "prompt = \"chat.md\"\n",
            "mode = \"interactive\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("chat.md"), "chat prompt\n").unwrap();
    git_add_commit(tmp.path(), "add turn-by-turn cursus");

    // Mock agent: outputs JSON with result/session_id, exits 0, no sentinel
    // -> waiting_for_input should be true on first turn
    let mock_dir = TempDir::new().unwrap();
    let invocation_count_file = mock_dir.path().join("count.txt");
    fs::write(&invocation_count_file, "0").unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/sh\n",
                "COUNT=$(cat \"{count}\")\n",
                "COUNT=$((COUNT + 1))\n",
                "echo $COUNT > \"{count}\"\n",
                "if [ $COUNT -ge 2 ]; then\n",
                "  touch \"${{PWD}}/.iter-complete\"\n",
                "  echo '{{\"result\": \"Done! Task complete.\", \"session_id\": \"sess-final\"}}'\n",
                "else\n",
                "  echo '{{\"result\": \"What should I do next?\", \"session_id\": \"sess-001\"}}'\n",
                "fi\n",
                "exit 0\n",
            ),
            count = invocation_count_file.display(),
        ),
    );

    // --- Turn 1: initial invocation with piped stdin ---
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["chat", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf chat");

    // Close stdin immediately — programmatic mode triggered by --output-format json
    drop(guard.child_mut().stdin.take());

    let output1 = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf chat turn 1");
    drop(_permit);

    assert!(
        output1.status.success(),
        "turn 1 should exit 0 (waiting_for_input): {}",
        String::from_utf8_lossy(&output1.stderr)
    );

    let events1 = parse_ndjson_events(&output1.stdout);
    let types1 = event_types(&events1);

    // Should have: run_start, iter_start, turn, run_complete
    assert_eq!(types1[0], "run_start", "first event should be run_start");
    assert!(
        types1.contains(&"iter_start"),
        "should emit iter_start: {types1:?}"
    );
    assert!(
        types1.contains(&"turn"),
        "should emit turn event: {types1:?}"
    );

    // Validate the turn event
    let turn_event = events1.iter().find(|e| e["event"] == "turn").unwrap();
    assert_eq!(
        turn_event["waiting_for_input"], true,
        "first turn should have waiting_for_input=true"
    );
    assert!(
        turn_event["content"]
            .as_str()
            .unwrap()
            .contains("What should I do next?"),
        "turn content should contain agent response"
    );
    assert!(
        turn_event["session_id"].as_str().is_some(),
        "turn should include session_id"
    );

    // run_complete should have status "waiting_for_input"
    let rc = events1
        .iter()
        .find(|e| e["event"] == "run_complete")
        .unwrap();
    assert_eq!(
        rc["status"], "waiting_for_input",
        "run_complete should have status waiting_for_input"
    );
    let resume_cmd = rc["resume_command"].as_str().unwrap();
    assert!(
        resume_cmd.contains("--resume"),
        "resume_command should contain --resume"
    );

    // Extract run_id for resume
    let run_id = get_run_id(tmp.path());

    // Verify metadata shows WaitingForInput
    let meta1 = read_run_metadata(tmp.path());
    assert_eq!(
        meta1["status"].as_str().unwrap(),
        "waiting_for_input",
        "metadata status should be waiting_for_input"
    );

    // --- Turn 2: resume with input, agent creates .iter-complete ---
    let _permit2 = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard2 = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["chat", "--resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf chat --resume");

    {
        use std::io::Write;
        let stdin = guard2.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"Please finish the task.\n").unwrap();
    }
    drop(guard2.child_mut().stdin.take());

    let output2 = guard2
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf chat turn 2");
    drop(_permit2);

    assert!(
        output2.status.success(),
        "turn 2 should exit 0 (completed): {}",
        String::from_utf8_lossy(&output2.stderr)
    );

    let events2 = parse_ndjson_events(&output2.stdout);
    let types2 = event_types(&events2);

    // Should have a turn event with waiting_for_input=false (agent created sentinel)
    let turn_event2 = events2.iter().find(|e| e["event"] == "turn");
    assert!(
        turn_event2.is_some(),
        "resume should emit turn event: {types2:?}"
    );
    assert_eq!(
        turn_event2.unwrap()["waiting_for_input"],
        false,
        "second turn should have waiting_for_input=false (sentinel created)"
    );

    // Should have iter_complete and run_complete
    assert!(
        types2.contains(&"iter_complete"),
        "should emit iter_complete after sentinel: {types2:?}"
    );
    let rc2 = events2
        .iter()
        .find(|e| e["event"] == "run_complete")
        .unwrap();
    assert_eq!(
        rc2["status"], "completed",
        "final run_complete should be completed"
    );

    // Verify metadata shows completed
    let meta2 = read_run_metadata(tmp.path());
    assert_eq!(
        meta2["status"].as_str().unwrap(),
        "completed",
        "metadata status should be completed after second turn"
    );
}

#[test]
fn cursus_programmatic_stall_resume_retry() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Stall resume test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"discuss\"\n",
            "prompt = \"discuss.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"draft\"\n",
            "prompt = \"draft.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("discuss.md"), "discuss prompt\n").unwrap();
    fs::write(prompts_dir.join("draft.md"), "draft prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall resume cursus");

    // First run: discuss completes, draft stalls (no sentinel, exit 2)
    let mock_dir = TempDir::new().unwrap();
    let mock_agent_stall = create_mock_script(
        mock_dir.path(),
        "mock_agent_stall.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'discuss.md'; then\n",
            "  touch \"${PWD}/.iter-complete\"\n",
            "  exit 0\n",
            "fi\n",
            "exit 2\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent_stall),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should stall (exit 2): {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify stall event was emitted
    let stall_events = parse_ndjson_events(&output.stdout);
    let stall_event = stall_events
        .iter()
        .find(|e| e["event"] == "stall")
        .expect("should emit stall event in initial run");
    assert_eq!(stall_event["iter"], "draft");

    let run_id = get_run_id(tmp.path());

    // --- Resume with "retry": re-run stalled iter, this time it completes ---
    let mock_agent_complete = create_mock_script(
        mock_dir.path(),
        "mock_agent_complete.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["pipeline", "--resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent_complete)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn resume with retry");

    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"retry\n").unwrap();
    }
    drop(guard.child_mut().stdin.take());

    let resume_output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for resume");
    drop(_permit);

    assert!(
        resume_output.status.success(),
        "resume with retry should exit 0: {}",
        String::from_utf8_lossy(&resume_output.stderr)
    );

    // Verify programmatic events from resume
    let resume_events = parse_ndjson_events(&resume_output.stdout);
    let resume_types = event_types(&resume_events);

    // Should emit stall event (re-emitted on resume), then iter events, then run_complete
    assert!(
        resume_types.contains(&"stall"),
        "resume should emit stall event before action: {resume_types:?}"
    );

    // run_complete should show completed
    let rc = resume_events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("should emit run_complete");
    assert_eq!(rc["status"], "completed", "retry should complete the run");

    // Verify metadata
    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
}

#[test]
fn cursus_programmatic_stall_resume_skip() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Stall skip test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"gather\"\n",
            "prompt = \"gather.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"process\"\n",
            "prompt = \"process.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
            "\n",
            "[[iter]]\n",
            "name = \"finalize\"\n",
            "prompt = \"finalize.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("gather.md"), "gather prompt\n").unwrap();
    fs::write(prompts_dir.join("process.md"), "process prompt\n").unwrap();
    fs::write(prompts_dir.join("finalize.md"), "finalize prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall skip cursus");

    // First run: gather completes, process stalls
    let mock_dir = TempDir::new().unwrap();
    let mock_agent_stall = create_mock_script(
        mock_dir.path(),
        "mock_agent_stall.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'gather.md'; then\n",
            "  touch \"${PWD}/.iter-complete\"\n",
            "  exit 0\n",
            "fi\n",
            "exit 2\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent_stall),
    );

    assert_eq!(output.status.code(), Some(2), "should stall");
    let run_id = get_run_id(tmp.path());

    // Resume with "skip": should advance to finalize
    let mock_agent_complete = create_mock_script(
        mock_dir.path(),
        "mock_agent_complete.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["pipeline", "--resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent_complete)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn resume with skip");

    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"skip\n").unwrap();
    }
    drop(guard.child_mut().stdin.take());

    let resume_output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for resume skip");
    drop(_permit);

    assert!(
        resume_output.status.success(),
        "resume with skip should exit 0: {}",
        String::from_utf8_lossy(&resume_output.stderr)
    );

    // Verify completed
    let resume_events = parse_ndjson_events(&resume_output.stdout);
    let rc = resume_events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("should emit run_complete");
    assert_eq!(rc["status"], "completed", "skip should complete the run");

    let meta = read_run_metadata(tmp.path());
    assert_eq!(meta["status"].as_str().unwrap(), "completed");
}

#[test]
fn cursus_programmatic_stall_resume_abort() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Stall abort test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"work\"\n",
            "prompt = \"work.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("work.md"), "work prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall abort cursus");

    // Run: work stalls
    let mock_dir = TempDir::new().unwrap();
    let mock_agent_stall = create_mock_script(
        mock_dir.path(),
        "mock_agent_stall.sh",
        "#!/bin/sh\nexit 2\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent_stall),
    );

    assert_eq!(output.status.code(), Some(2), "should stall");
    let run_id = get_run_id(tmp.path());

    // Resume with "abort"
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["pipeline", "--resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent_stall)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn resume with abort");

    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"abort\n").unwrap();
    }
    drop(guard.child_mut().stdin.take());

    let resume_output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for resume abort");
    drop(_permit);

    // Abort should exit with non-zero
    assert!(
        !resume_output.status.success(),
        "resume with abort should exit non-zero"
    );

    // Verify interrupted status
    let resume_events = parse_ndjson_events(&resume_output.stdout);
    let rc = resume_events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("should emit run_complete on abort");
    assert_eq!(
        rc["status"], "interrupted",
        "abort should produce interrupted status"
    );

    let meta = read_run_metadata(tmp.path());
    assert_eq!(
        meta["status"].as_str().unwrap(),
        "interrupted",
        "metadata should be interrupted after abort"
    );
}

// ===========================================================================
// Resume command printed on all exit types
// ===========================================================================

#[test]
fn cursus_stall_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 1\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .stdin(Stdio::null()),
    );

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf build --resume build-"),
        "stalled cursus run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn cursus_completion_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .stdin(Stdio::null()),
    );

    assert!(
        output.status.success(),
        "cursus build should complete, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf build --resume build-"),
        "completed cursus run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn cursus_exhausted_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    write_cursus_toml(
        tmp.path(),
        "build",
        concat!(
            "description = \"Build\"\n",
            "alias = \"b\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    );

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 2\n");

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(mock_cl_dir.path(), "cl", "#!/bin/sh\nexit 0\n");
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("PATH", &mock_path_with_cl)
            .env("SGF_FORCE_TERMINAL", "1")
            .stdin(Stdio::null()),
    );

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf build --resume build-"),
        "exhausted cursus run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn cursus_interrupt_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .env("SGF_FORCE_TERMINAL", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf build --resume build-"),
        "interrupted cursus run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn simple_prompt_completion_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "simple prompt should complete, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf task.md --resume simple-"),
        "completed simple prompt run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn simple_prompt_exhausted_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 0\n");

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a", "-n", "1"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (exhausted), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf task.md --resume simple-"),
        "exhausted simple prompt run should print resume command, got stderr:\n{stderr}"
    );
}

#[test]
fn simple_prompt_interrupt_prints_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let ready_file = mock_dir.path().join("agent_ready");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_slow.sh",
        &format!(
            "#!/bin/bash\ntrap '' INT\ntouch \"{}\"\nfor i in $(seq 1 50); do sleep 0.1; done\nexit 2\n",
            ready_file.display()
        ),
    );

    let prompt_path = tmp.path().join("task.md");
    fs::write(&prompt_path, "Do the thing").unwrap();

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["task.md", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("To resume: sgf task.md --resume simple-"),
        "interrupted simple prompt run should print resume command, got stderr:\n{stderr}"
    );
}

// ===========================================================================
// Programmatic mode: resume_command in run_complete across all exit types
// ===========================================================================

#[test]
fn cursus_programmatic_stalled_run_complete_has_resume_command_and_run_id() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Stall resume_command test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 1\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add stall resume_command cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(mock_dir.path(), "mock_agent.sh", "#!/bin/sh\nexit 1\n");

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (stalled): {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);

    let run_start = events
        .iter()
        .find(|e| e["event"] == "run_start")
        .expect("should emit run_start");
    let run_id = run_start["run_id"].as_str().unwrap();
    assert!(
        run_id.starts_with("build-"),
        "run_id should start with cursus name, got: {run_id}"
    );

    let rc = events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("should emit run_complete");
    assert_eq!(rc["status"], "stalled");
    assert_eq!(
        rc["run_id"].as_str().unwrap(),
        run_id,
        "run_complete run_id should match run_start run_id"
    );
    let resume_cmd = rc["resume_command"].as_str().unwrap();
    assert!(
        resume_cmd.contains("--resume"),
        "stalled run_complete should include --resume in resume_command, got: {resume_cmd}"
    );
    assert!(
        resume_cmd.contains(run_id),
        "resume_command should contain run_id, got: {resume_cmd}"
    );
}

#[test]
fn cursus_programmatic_interrupted_run_complete_has_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Interrupt resume_command test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 5\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add interrupt resume_command cursus");

    let mock_dir = TempDir::new().unwrap();
    let ready_file = mock_dir.path().join("agent_ready");
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_slow.sh",
        &format!(
            "#!/bin/bash\ntrap '' INT\ntouch \"{}\"\nfor i in $(seq 1 50); do sleep 0.1; done\nexit 2\n",
            ready_file.display()
        ),
    );

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");

    let guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_READY_FILE", &ready_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
    drop(_permit);

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double Ctrl+C"
    );

    let events = parse_ndjson_events(&output.stdout);

    let run_start = events
        .iter()
        .find(|e| e["event"] == "run_start")
        .expect("should emit run_start on interrupt");
    let run_id = run_start["run_id"].as_str().unwrap();

    let rc = events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("should emit run_complete on interrupt");
    assert_eq!(
        rc["status"], "interrupted",
        "interrupted run should have status 'interrupted'"
    );
    assert_eq!(
        rc["run_id"].as_str().unwrap(),
        run_id,
        "run_complete run_id should match run_start run_id"
    );
    let resume_cmd = rc["resume_command"].as_str().unwrap();
    assert!(
        resume_cmd.contains("--resume"),
        "interrupted run_complete should include --resume in resume_command, got: {resume_cmd}"
    );
    assert!(
        resume_cmd.contains(run_id),
        "resume_command should contain run_id, got: {resume_cmd}"
    );
}

#[test]
fn cursus_programmatic_piped_stdin_run_complete_has_resume_command() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("build.toml"),
        concat!(
            "description = \"Piped stdin resume_command test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"build\"\n",
            "prompt = \"build.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 3\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("build.md"), "Build prompt\n").unwrap();
    git_add_commit(tmp.path(), "add piped stdin resume_command cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf with piped stdin");

    drop(guard.child_mut().stdin.take());

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf");
    drop(_permit);

    assert!(
        output.status.success(),
        "piped stdin should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);

    let run_start = events
        .iter()
        .find(|e| e["event"] == "run_start")
        .expect("piped stdin should emit run_start");
    let run_id = run_start["run_id"].as_str().unwrap();
    assert!(
        run_id.starts_with("build-"),
        "run_id should start with cursus name, got: {run_id}"
    );

    let rc = events
        .iter()
        .find(|e| e["event"] == "run_complete")
        .expect("piped stdin should emit run_complete");
    assert_eq!(rc["status"], "completed");
    assert_eq!(
        rc["run_id"].as_str().unwrap(),
        run_id,
        "run_complete run_id should match run_start run_id"
    );
    let resume_cmd = rc["resume_command"].as_str().unwrap();
    assert!(
        resume_cmd.contains("--resume"),
        "piped stdin run_complete should include --resume in resume_command, got: {resume_cmd}"
    );
    assert!(
        resume_cmd.contains(run_id),
        "resume_command should contain run_id, got: {resume_cmd}"
    );
}

#[test]
fn cursus_programmatic_interactive_multiline_stdin_reaches_agent() {
    use std::io::Write;

    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let mock_dir = TempDir::new().unwrap();
    let stdin_capture = mock_dir.path().join("stdin_capture.txt");

    // Mock agent captures the @file content to verify multiline stdin arrived intact
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "LAST_ARG=\"${{@: -1}}\"\n",
                "if [[ \"$LAST_ARG\" == @* ]]; then\n",
                "  cat \"${{LAST_ARG:1}}\" > \"{capture}\"\n",
                "else\n",
                "  echo \"$LAST_ARG\" > \"{capture}\"\n",
                "fi\n",
                "touch \"${{PWD}}/.iter-complete\"\n",
                "echo '{{\"result\": \"Done\", \"session_id\": \"sess-multi\"}}'\n",
                "exit 0\n",
            ),
            capture = stdin_capture.display()
        ),
    );

    write_cursus_toml(
        tmp.path(),
        "multitest",
        concat!(
            "description = \"Multiline stdin test\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"work\"\n",
            "prompt = \"work.md\"\n",
            "mode = \"interactive\"\n",
            "iterations = 1\n",
        ),
    );
    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("work.md"), "do work\n").unwrap();
    git_add_commit(tmp.path(), "add work prompt");

    // Simulate heredoc-style multiline content: multiple lines with blank lines
    let multiline_content = "line one\n\nline three\nline four with special: $VAR `backtick`\n";

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["multitest", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf multitest");

    {
        let mut stdin = guard.child_mut().stdin.take().expect("open stdin");
        stdin.write_all(multiline_content.as_bytes()).unwrap();
        stdin.flush().unwrap();
        // Drop closes the write end → EOF
    }

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf multitest");
    drop(_permit);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "multiline stdin should succeed, stderr: {stderr}"
    );

    let captured = fs::read_to_string(&stdin_capture).unwrap_or_default();
    assert_eq!(
        captured, multiline_content,
        "agent should receive exact multiline content via @file reference"
    );

    // Verify the _stdin.md file was created in the run context dir
    let run_dirs: Vec<_> = fs::read_dir(tmp.path().join(".sgf/run"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert!(!run_dirs.is_empty(), "should have a run directory");

    let stdin_file = run_dirs[0].path().join("context/_stdin.md");
    assert!(stdin_file.exists(), "_stdin.md should exist in context dir");
    let file_content = fs::read_to_string(&stdin_file).unwrap();
    assert_eq!(
        file_content, multiline_content,
        "_stdin.md should contain the exact multiline input"
    );
}

#[test]
fn cursus_programmatic_event_structure_validates_all_fields() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let cursus_dir = tmp.path().join(".sgf/cursus");
    fs::create_dir_all(&cursus_dir).unwrap();
    fs::write(
        cursus_dir.join("pipeline.toml"),
        concat!(
            "description = \"Full event structure validation\"\n",
            "auto_push = false\n",
            "\n",
            "[[iter]]\n",
            "name = \"gather\"\n",
            "prompt = \"gather.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 2\n",
            "produces = \"gathered-data\"\n",
            "\n",
            "[[iter]]\n",
            "name = \"process\"\n",
            "prompt = \"process.md\"\n",
            "mode = \"afk\"\n",
            "iterations = 3\n",
            "consumes = [\"gathered-data\"]\n",
        ),
    )
    .unwrap();

    let prompts_dir = tmp.path().join(".sgf/prompts");
    fs::create_dir_all(&prompts_dir).unwrap();
    fs::write(prompts_dir.join("gather.md"), "Gather data\n").unwrap();
    fs::write(prompts_dir.join("process.md"), "Process data\n").unwrap();
    git_add_commit(tmp.path(), "add event structure cursus");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "PROMPT=\"${@: -1}\"\n",
            "if echo \"$PROMPT\" | grep -q 'gather.md'; then\n",
            "  mkdir -p \"$SGF_RUN_CONTEXT\"\n",
            "  echo 'data' > \"$SGF_RUN_CONTEXT/gathered-data.md\"\n",
            "fi\n",
            "touch \"${PWD}/.iter-complete\"\n",
            "exit 0\n",
        ),
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["pipeline", "-a", "--output-format", "json"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );

    assert!(
        output.status.success(),
        "event structure test should exit 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = parse_ndjson_events(&output.stdout);
    let types = event_types(&events);

    // --- run_start: validate all required fields ---
    let rs = &events[0];
    assert_eq!(rs["event"], "run_start");
    assert!(
        rs["run_id"].is_string(),
        "run_start must have string run_id"
    );
    assert_eq!(rs["cursus"], "pipeline");
    let iters_arr = rs["iters"].as_array().unwrap();
    assert_eq!(iters_arr.len(), 2, "run_start should list 2 iters");
    assert_eq!(iters_arr[0]["name"], "gather");
    assert_eq!(iters_arr[0]["mode"], "afk");
    assert_eq!(iters_arr[0]["iterations"], 2);
    assert_eq!(iters_arr[1]["name"], "process");
    assert_eq!(iters_arr[1]["mode"], "afk");
    assert_eq!(iters_arr[1]["iterations"], 3);

    // --- iter_start: validate all fields for gather ---
    let is_gather = events
        .iter()
        .find(|e| e["event"] == "iter_start" && e["iter"] == "gather")
        .unwrap();
    assert_eq!(is_gather["mode"], "afk");
    assert_eq!(is_gather["iteration"], 2);
    assert!(
        is_gather["session_id"].is_string(),
        "iter_start must have string session_id"
    );

    // --- iter_complete: validate all fields for gather ---
    let ic_gather = events
        .iter()
        .find(|e| e["event"] == "iter_complete" && e["iter"] == "gather")
        .unwrap();
    assert_eq!(ic_gather["outcome"], "complete");
    assert_eq!(ic_gather["iterations_used"], 2);

    // --- context_produced: validate all fields ---
    let cp = events
        .iter()
        .find(|e| e["event"] == "context_produced")
        .unwrap();
    assert_eq!(cp["key"], "gathered-data");
    assert_eq!(cp["iter"], "gather");

    // --- context_consumed: validate all fields ---
    let cc = events
        .iter()
        .find(|e| e["event"] == "context_consumed")
        .unwrap();
    assert_eq!(cc["key"], "gathered-data");
    assert_eq!(cc["from_iter"], "gather");

    // --- transition: validate all fields ---
    let tr = events.iter().find(|e| e["event"] == "transition").unwrap();
    assert_eq!(tr["from_iter"], "gather");
    assert_eq!(tr["to_iter"], "process");
    assert_eq!(tr["reason"], "complete");

    // --- iter_start for process: validate consumes context is set up ---
    let is_process = events
        .iter()
        .find(|e| e["event"] == "iter_start" && e["iter"] == "process")
        .unwrap();
    assert_eq!(is_process["mode"], "afk");
    assert_eq!(is_process["iteration"], 3);

    // --- iter_complete for process ---
    let ic_process = events
        .iter()
        .find(|e| e["event"] == "iter_complete" && e["iter"] == "process")
        .unwrap();
    assert_eq!(ic_process["outcome"], "complete");

    // --- run_complete: validate all fields ---
    let rc = events.last().unwrap();
    assert_eq!(rc["event"], "run_complete");
    assert_eq!(rc["status"], "completed");
    let run_id = rs["run_id"].as_str().unwrap();
    assert_eq!(
        rc["run_id"].as_str().unwrap(),
        run_id,
        "run_complete run_id must match run_start run_id"
    );
    let resume_cmd = rc["resume_command"].as_str().unwrap();
    assert!(
        resume_cmd.contains("--resume"),
        "resume_command must contain --resume"
    );
    assert!(
        resume_cmd.contains(run_id),
        "resume_command must contain the run_id"
    );

    // --- Verify no unexpected event types (only known events) ---
    let known = [
        "run_start",
        "iter_start",
        "iter_complete",
        "transition",
        "context_produced",
        "context_consumed",
        "run_complete",
    ];
    for t in &types {
        assert!(
            known.contains(t),
            "unexpected event type '{t}' in happy-path multi-iter run, got: {types:?}"
        );
    }

    // --- Ordering: context_consumed before process iter_start ---
    let consumed_pos = events
        .iter()
        .position(|e| e["event"] == "context_consumed")
        .unwrap();
    let process_start_pos = events
        .iter()
        .position(|e| e["event"] == "iter_start" && e["iter"] == "process")
        .unwrap();
    assert!(
        consumed_pos < process_start_pos,
        "context_consumed must precede process iter_start"
    );

    // --- Ordering: context_produced after gather iter_complete ---
    let produced_pos = events
        .iter()
        .position(|e| e["event"] == "context_produced")
        .unwrap();
    let gather_complete_pos = events
        .iter()
        .position(|e| e["event"] == "iter_complete" && e["iter"] == "gather")
        .unwrap();
    assert!(
        produced_pos > gather_complete_pos,
        "context_produced must follow gather iter_complete"
    );
}
