use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    assert_eq!(deny.len(), 5, "expected 1 custom + 4 sgf rules");
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
        args.contains("--template ralph-sandbox:latest"),
        "missing --template"
    );
    assert!(
        args.contains("--auto-push true"),
        "missing --auto-push true"
    );
    assert!(
        args.contains("--max-iterations 30"),
        "missing --max-iterations"
    );
    assert!(
        !args.contains("--no-sandbox"),
        "build should NOT pass --no-sandbox"
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
// Docker template build (gated)
// ===========================================================================

#[test]
#[ignore] // Requires Docker; run explicitly with `cargo test -p springfield -- --ignored`
fn template_build_requires_pn() {
    // Run with empty PATH so pn is not found
    let output = Command::new(sgf_bin())
        .args(["template", "build"])
        .env("PATH", "")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("pn") || stderr.to_lowercase().contains("not found"),
        "should report pn not found: {stderr}"
    );
}

// ===========================================================================
// Template pre-flight checks
// ===========================================================================

#[test]
fn preflight_template_missing_and_build_fails_aborts_loop() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock docker that fails both inspect and build
    let mock_docker_dir = TempDir::new().unwrap();
    create_mock_script(mock_docker_dir.path(), "docker", "#!/bin/sh\nexit 1\n");
    let path_with_mock_docker = format!("{}:{}", mock_docker_dir.path().display(), mock_path);

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("PATH", &path_with_mock_docker)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "loop should abort when template build fails"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ralph-sandbox:latest not found")
            || stderr.contains("template")
            || stderr.contains("docker build failed"),
        "should mention template failure: {stderr}"
    );
}

#[test]
fn preflight_template_exists_loop_proceeds() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());
    create_spec_and_commit(tmp.path(), "auth");

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    // Mock docker that succeeds for inspect (image exists) and returns labels
    let mock_docker_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_docker_dir.path(),
        "docker",
        concat!(
            "#!/bin/sh\n",
            "case \"$1\" in\n",
            "  image) echo 'somehash|somehash'; exit 0 ;;\n",
            "  *) exit 0 ;;\n",
            "esac\n",
        ),
    );
    let path_with_mock_docker = format!("{}:{}", mock_docker_dir.path().display(), mock_path);

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
        .env("PATH", &path_with_mock_docker)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "loop should proceed when template exists: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Ralph should have been invoked
    assert!(args_file.exists(), "ralph should have been invoked");
}

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

    // Mock docker that succeeds for inspect (image exists)
    let mock_docker_dir = TempDir::new().unwrap();
    create_mock_script(
        mock_docker_dir.path(),
        "docker",
        concat!(
            "#!/bin/sh\n",
            "case \"$1\" in\n",
            "  image) echo 'somehash|somehash'; exit 0 ;;\n",
            "  *) exit 0 ;;\n",
            "esac\n",
        ),
    );
    let path_with_mock_docker = format!("{}:{}", mock_docker_dir.path().display(), mock_path);

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
        .env("PATH", &path_with_mock_docker)
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
