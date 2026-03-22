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
    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn sgf")
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to run sgf")
}

fn sgf_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new(sgf_bin());
    cmd.current_dir(dir);
    cmd.env("HOME", fake_home());
    cmd.env("PATH", mock_bin_path());
    cmd.env("SGF_SKIP_PREFLIGHT", "1");
    cmd.env("SGF_TEST_NO_SETSID", "1");
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
// Loop orchestration (mocked ralph via CLI)
// ===========================================================================

#[test]
fn build_invokes_ralph_with_correct_flags() {
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

    // Mock ralph that logs all args
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    // Mock ralph that checks for PID file existence during execution
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

    // PID file should have existed while ralph was running
    let pid_state = fs::read_to_string(&state_file).unwrap();
    assert!(
        pid_state.contains(".pid"),
        "PID file should exist during ralph execution"
    );

    // PID file should be cleaned up after ralph exits
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
fn afk_passes_log_file_to_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\ntouch \"${PWD}/.iter-complete\"\nexit 0\n",
    );

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent),
    );
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

#[test]
fn end_to_end_build_passes_raw_path_and_spec() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

    // Create spec file (validate requires it)
    fs::create_dir_all(tmp.path().join("specs")).unwrap();
    fs::write(tmp.path().join("specs/auth.md"), "# Auth spec").unwrap();
    git_add_commit(tmp.path(), "add spec");

    // Mock ralph that logs args and env
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
    let env_file = mock_dir.path().join("ralph_env.txt");
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
        !args.contains("--spec"),
        "should NOT pass --spec to ralph, got: {args}"
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
    let args_file = mock_dir.path().join("ralph_args.txt");
    let env_file = mock_dir.path().join("ralph_env.txt");
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
fn build_with_spec_does_not_pass_spec_to_ralph() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
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
        "should NOT pass --spec to ralph, got: {args}"
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
        "should exit 0 (ralph completed) after single Ctrl+C timeout, not 130"
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
fn double_ctrl_d_exits_130() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());
    let ready_file = mock_dir.path().join("sgf_ready");

    // Closing a piped stdin causes continuous EOF (read returns 0), which
    // simulates a rapid double Ctrl+D press.
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_MONITOR_STDIN", "1")
            .env("SGF_READY_FILE", &ready_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let stdin = guard.child_mut().stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    drop(stdin);

    std::thread::sleep(Duration::from_millis(1000));
    let output = match guard.try_wait() {
        Ok(Some(_)) => guard
            .wait_with_output_timeout(Duration::from_secs(30))
            .expect("wait for sgf"),
        _ => {
            nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)
                .expect("send cleanup SIGTERM");
            guard
                .wait_with_output_timeout(Duration::from_secs(30))
                .expect("wait for sgf")
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent_mixed.sh",
        "#!/bin/bash\ntrap '' INT\nfor i in $(seq 1 50); do sleep 0.1; done\nexit 0\n",
    );
    let ready_file = mock_dir.path().join("sgf_ready");

    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_MONITOR_STDIN", "1")
            .env("SGF_READY_FILE", &ready_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let mut stdin = guard.child_mut().stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

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
        guard.try_wait().expect("try_wait").is_none(),
        "process should still be running after Ctrl+C + non-EOF stdin"
    );

    // Clean up.
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).expect("send cleanup SIGTERM");
    drop(stdin);

    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");
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
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let grandchild_pid_file = mock_dir.path().join("grandchild.pid");
    let ready_file = mock_dir.path().join("sgf_ready");

    // Mock ralph that spawns a long-running grandchild, writes the grandchild's
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

    // Mock ralph that checks if it became a session leader.
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
            .env("SGF_AGENT_COMMAND", &mock_agent),
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
fn afk_monitor_stdin_eof_triggers_shutdown() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let mock_dir = TempDir::new().unwrap();
    let mock_agent = create_slow_mock_agent(mock_dir.path());

    let ready_file = mock_dir.path().join("sgf_ready");

    // AFK mode with SGF_MONITOR_STDIN=1: closing piped stdin triggers EOF shutdown.
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &mock_agent)
            .env("SGF_MONITOR_STDIN", "1")
            .env("SGF_READY_FILE", &ready_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("spawn sgf");

    let stdin = guard.child_mut().stdin.take().expect("open stdin");
    let pid = nix::unistd::Pid::from_raw(guard.id() as i32);

    wait_for_ready(&ready_file);
    drop(stdin); // EOF triggers double-Ctrl+D detection

    std::thread::sleep(Duration::from_millis(1000));
    let output = match guard.try_wait() {
        Ok(Some(_)) => guard
            .wait_with_output_timeout(Duration::from_secs(30))
            .expect("wait for sgf"),
        _ => {
            nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)
                .expect("send cleanup SIGTERM");
            guard
                .wait_with_output_timeout(Duration::from_secs(30))
                .expect("wait for sgf")
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    let argv: Vec<&str> = args.split_whitespace().collect();
    assert!(
        argv.contains(&"-a"),
        "AFK mode from config should pass -a flag to ralph, got: {args}"
    );
}

#[test]
fn interactive_override_does_not_invoke_ralph() {
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
        args.contains("--verbose"),
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    // In the cursus runner, ralph exiting with code 1 without touching
    // .iter-complete is treated as "exhausted" (stalled), exit code 2.
    // This test verifies stall styling for a ralph error scenario.
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());
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
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("cursus STALLED"),
        "ralph exit 1 without sentinel should stall, got stderr: {stderr}"
    );
    assert!(
        stderr.contains("\x1b[1;33m"),
        "stalled should use yellow (warning) styling, got stderr: {stderr}"
    );
}

#[test]
fn exit_2_uses_warning_styling() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    setup_default_cursus(tmp.path());

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
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

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
    setup_default_cursus(tmp.path());
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
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("sgf: cursus STALLED"),
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
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("iterations exhausted"),
        "NO_COLOR exit 2 should show 'iterations exhausted' message, got stderr: {stderr}"
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
// End-to-end: sgf → ralph → cl → mock claude-wrapper-secret
// ===========================================================================

fn target_bin_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_BIN_EXE_sgf"));
    dir.pop();
    dir
}

#[test]
fn e2e_sgf_ralph_cl_context_files_in_append_system_prompt() {
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

    let output = run_sgf(
        sgf_cmd(tmp.path())
            .args(["build", "auth", "-a"])
            .env("SGF_AGENT_COMMAND", &ralph_bin)
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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

    // Verify ralph received the same session_id
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains(session_id),
        "ralph should receive same session_id as metadata, got: {args}"
    );
    assert!(
        args.contains("--session-id"),
        "ralph should receive --session-id flag, got: {args}"
    );
}

#[test]
fn interactive_session_writes_metadata_with_session_id() {
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

    let meta = read_single_session_metadata(tmp.path());

    assert!(
        meta["run_id"].as_str().unwrap().starts_with("spec-"),
        "run_id should start with spec-, got: {}",
        meta["run_id"]
    );
    assert_eq!(meta["cursus"], "spec");
    assert_eq!(meta["status"], "completed");

    let iters_completed = meta["iters_completed"].as_array().unwrap();
    assert!(
        !iters_completed.is_empty(),
        "iters_completed should not be empty"
    );
    let session_id = iters_completed[0]["session_id"].as_str().unwrap();
    assert!(
        !session_id.is_empty() && session_id.contains('-'),
        "session_id should be a UUID, got: {session_id}"
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
            .args(["resume", loop_id])
            .env("PATH", &mock_path_with_cl)
            .env_remove("CLAUDECODE"),
    );

    assert!(
        output.status.success(),
        "sgf resume failed: {}",
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
fn resume_with_no_sessions_exits_1() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).arg("resume"));

    assert_eq!(
        output.status.code(),
        Some(1),
        "sgf resume with no sessions should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No sessions found"),
        "stderr should contain 'No sessions found', got: {stderr}"
    );
}

#[test]
fn resume_with_bad_loop_id_exits_1() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let output = run_sgf(sgf_cmd(tmp.path()).args(["resume", "nonexistent-loop-id"]));

    assert_eq!(
        output.status.code(),
        Some(1),
        "sgf resume with bad loop_id should exit 1"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Session not found"),
        "stderr should contain 'Session not found', got: {stderr}"
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
            .args(["resume", loop_id])
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
        "sgf resume should succeed, stderr: {}",
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
fn resume_flat_picker_across_multiple_loops() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let run_dir = tmp.path().join(".sgf/run");
    fs::create_dir_all(&run_dir).unwrap();

    let loop1 = "build-auth-20260316T120000";
    let session1 = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let meta1 = serde_json::json!({
        "loop_id": loop1,
        "iterations": [
            { "iteration": 1, "session_id": session1, "completed_at": "2026-03-16T12:02:30Z" }
        ],
        "stage": "build",
        "spec": "auth",
        "mode": "afk",
        "prompt": ".sgf/prompts/build.md",
        "iterations_total": 1,
        "status": "completed",
        "created_at": "2026-03-16T12:00:00Z",
        "updated_at": "2026-03-16T12:02:30Z"
    });
    fs::write(
        run_dir.join(format!("{loop1}.json")),
        serde_json::to_string_pretty(&meta1).unwrap(),
    )
    .unwrap();

    let loop2 = "spec-20260316T130000";
    let session2 = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let meta2 = serde_json::json!({
        "loop_id": loop2,
        "iterations": [
            { "iteration": 1, "session_id": session2, "completed_at": "2026-03-16T13:02:30Z" }
        ],
        "stage": "spec",
        "spec": null,
        "mode": "interactive",
        "prompt": ".sgf/prompts/spec.md",
        "iterations_total": 1,
        "status": "completed",
        "created_at": "2026-03-16T13:00:00Z",
        "updated_at": "2026-03-16T13:02:30Z"
    });
    fs::write(
        run_dir.join(format!("{loop2}.json")),
        serde_json::to_string_pretty(&meta2).unwrap(),
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

    // Select "1" — should be the newest session (spec loop, completed_at 13:02:30)
    let mut guard = ChildGuard::spawn(
        sgf_cmd(tmp.path())
            .arg("resume")
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
        stdin.write_all(b"1\n").expect("write selection");
    }

    let _permit = SGF_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    let output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("wait for sgf");

    assert!(
        output.status.success(),
        "sgf resume should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let cl_args = fs::read_to_string(&args_file).unwrap();
    assert!(
        cl_args.contains("--resume"),
        "cl should receive --resume flag, got: {cl_args}"
    );
    // Newest entry (session2, completed at 13:02:30) should be first
    assert!(
        cl_args.contains(session2),
        "cl should receive the newest session's id (spec loop), got: {cl_args}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Recent sessions"),
        "picker should show 'Recent sessions' header, got: {stderr}"
    );
    assert!(
        stderr.contains(loop1) && stderr.contains(loop2),
        "picker should show both loop IDs, got: {stderr}"
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

    // Mock ralph that logs args and touches .iter-complete sentinel
    let mock_dir = TempDir::new().unwrap();
    let args_file = mock_dir.path().join("ralph_args.txt");
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

    // Verify ralph was invoked with expected flags
    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains("-a"), "should pass -a (afk) flag");
    assert!(args.contains("--loop-id"), "should pass --loop-id");
    assert!(
        args.contains("--auto-push false"),
        "should pass --auto-push false, got: {args}"
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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

    // Verify ralph was actually invoked
    assert!(
        args_file.exists(),
        "ralph should have been invoked via alias dispatch"
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

    // Mock ralph that does NOT touch any sentinel and exits 2 (simulates exhaustion)
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

    // Verify that draft received context from discuss via --prompt-file
    // The consumed content is written to a _consumed.md file and passed via --prompt-file
    let draft_args = fs::read_to_string(&draft_args_file).unwrap();
    assert!(
        draft_args.contains("--prompt-file"),
        "draft should receive consumed context via --prompt-file, got: {draft_args}"
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
            .env("SGF_AGENT_COMMAND", &mock_agent),
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
            .args(["resume", &run_id])
            .env("SGF_AGENT_COMMAND", &mock_agent_complete)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .expect("failed to spawn sgf resume");

    // Write "2" (skip) to stdin
    {
        use std::io::Write;
        let stdin = guard.child_mut().stdin.as_mut().unwrap();
        stdin.write_all(b"2\n").unwrap();
    }

    let resume_output = guard
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to wait for sgf resume");
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
fn cursus_banner_true_passes_banner_flag_to_ralph() {
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(
        args.contains("--banner"),
        "should pass --banner flag when banner = true, got: {args}"
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
    let args_file = mock_dir.path().join("ralph_args.txt");
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
    assert!(stdout.contains("resume"), "should list resume builtin");
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

    let mock_agent = create_mock_script(
        mock_dir.path(),
        "mock_agent.sh",
        concat!(
            "#!/bin/sh\n",
            "mkdir -p \"$SGF_RUN_CONTEXT\"\n",
            "echo 'gathered' > \"$SGF_RUN_CONTEXT/gather-output.md\"\n",
            "touch \"${PWD}/.iter-complete\"\n",
            "exit 0\n",
        ),
    );

    let mock_cl_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_cl_dir.path(),
        "cl",
        &format!(
            concat!(
                "#!/bin/bash\n",
                "read -t 5 LINE\n",
                "echo \"$LINE\" > \"{capture}\"\n",
                "exit 0\n",
            ),
            capture = stdin_capture.display()
        ),
    );
    let mock_path_with_cl = format!("{}:{}", mock_cl_dir.path().display(), mock_bin_path());

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
            .env("PATH", &mock_path_with_cl)
            .env("SGF_MONITOR_STDIN", "1")
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
        "interactive cl should receive stdin data, but got: {captured:?}\nstderr: {stderr}"
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
