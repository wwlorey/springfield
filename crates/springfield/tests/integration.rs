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
        ".sgf/prompts/.assembled",
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
        ".sgf/PENSA.md",
        ".sgf/MEMENTO.md",
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

    // .sgf/MEMENTO.md
    let memento = fs::read_to_string(tmp.path().join(".sgf/MEMENTO.md")).unwrap();
    assert!(memento.contains("study `@specs/README.md`"));
    assert!(memento.contains("study `@BACKPRESSURE.md`"));
    assert!(
        !memento.contains("study `@.sgf/BACKPRESSURE.md`"),
        "MEMENTO should reference root BACKPRESSURE.md, not .sgf/"
    );
    assert!(memento.contains("study `@.sgf/PENSA.md`"));
    assert!(!memento.contains("## Stack"));
    assert!(!memento.contains("## References"));

    // .sgf/PENSA.md
    let pensa = fs::read_to_string(tmp.path().join(".sgf/PENSA.md")).unwrap();
    assert!(pensa.contains("pn"), "pensa.md should mention pn");
    assert!(
        pensa.contains("Claim Workflow"),
        "pensa.md should contain claim workflow"
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
// Prompt assembly (library-level, called from integration test context)
// ===========================================================================

#[test]
fn prompt_assembly_substitutes_spec() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts/.assembled")).unwrap();
    fs::write(
        tmp.path().join(".sgf/prompts/build.md"),
        "Build {{spec}} now.",
    )
    .unwrap();

    let vars = std::collections::HashMap::from([("spec".to_string(), "auth".to_string())]);
    let result = springfield::prompt::assemble(tmp.path(), "build", &vars).unwrap();

    let assembled = fs::read_to_string(&result).unwrap();
    assert_eq!(assembled, "Build auth now.");
    assert!(result.ends_with(".sgf/prompts/.assembled/build.md"));
}

#[test]
fn prompt_assembly_validates_unresolved() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts/.assembled")).unwrap();
    fs::write(
        tmp.path().join(".sgf/prompts/build.md"),
        "Build {{spec}} with {{unknown}}.",
    )
    .unwrap();

    let vars = std::collections::HashMap::from([("spec".to_string(), "auth".to_string())]);
    let result = springfield::prompt::assemble(tmp.path(), "build", &vars);

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("unknown"),
        "error should name unresolved var: {err}"
    );
}

#[test]
fn prompt_assembly_passthrough() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts/.assembled")).unwrap();
    let content = "No variables here, just plain text.";
    fs::write(tmp.path().join(".sgf/prompts/verify.md"), content).unwrap();

    let vars = std::collections::HashMap::new();
    let result = springfield::prompt::assemble(tmp.path(), "verify", &vars).unwrap();

    let assembled = fs::read_to_string(&result).unwrap();
    assert_eq!(assembled, content);
}

#[test]
fn prompt_assembly_prepends_memento() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts/.assembled")).unwrap();

    let memento = "study `specs/README.md`\nstudy `.sgf/BACKPRESSURE.md`\nstudy `.sgf/PENSA.md`\n";
    fs::write(tmp.path().join(".sgf/MEMENTO.md"), memento).unwrap();

    let template = "Read `specs/README.md`.\n\nVerify the specs.\n";
    fs::write(tmp.path().join(".sgf/prompts/verify.md"), template).unwrap();

    let vars = std::collections::HashMap::new();
    let result = springfield::prompt::assemble(tmp.path(), "verify", &vars).unwrap();

    let assembled = fs::read_to_string(&result).unwrap();
    assert!(
        assembled.starts_with(memento),
        "assembled should start with memento content"
    );
    assert!(
        assembled.contains(template),
        "assembled should contain template content"
    );
    assert!(
        assembled.find(memento).unwrap() < assembled.find(template).unwrap(),
        "memento should appear before template"
    );
}

#[test]
fn prompt_assembly_without_memento() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".sgf/prompts/.assembled")).unwrap();
    // Do NOT create .sgf/MEMENTO.md
    let template = "No variables here, just plain text.";
    fs::write(tmp.path().join(".sgf/prompts/verify.md"), template).unwrap();

    let vars = std::collections::HashMap::new();
    let result = springfield::prompt::assemble(tmp.path(), "verify", &vars).unwrap();

    let assembled = fs::read_to_string(&result).unwrap();
    assert_eq!(
        assembled, template,
        "without memento, assembled should equal template"
    );
}

// ===========================================================================
// Loop orchestration (mocked ralph via CLI)
// ===========================================================================

#[test]
fn build_invokes_ralph_with_correct_flags() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

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
fn afk_tees_output_to_log() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

    let (_mock_pn_dir, mock_path) = setup_mock_pn();

    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(
        mock_dir.path(),
        "mock_ralph.sh",
        "#!/bin/sh\necho 'iteration 1 of 30'\necho 'task complete'\nexit 0\n",
    );

    let output = sgf_cmd(tmp.path())
        .args(["build", "auth", "-a"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(output.status.success());

    // Find the log file
    let logs_dir = tmp.path().join(".sgf/logs");
    let log_files: Vec<_> = fs::read_dir(&logs_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .collect();
    assert_eq!(log_files.len(), 1, "expected exactly one log file");

    let log_content = fs::read_to_string(log_files[0].path()).unwrap();
    assert!(log_content.contains("iteration 1 of 30"));
    assert!(log_content.contains("task complete"));
}

#[test]
fn spec_runs_one_interactive_iteration() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

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
        .arg("spec")
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf spec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    // Should have 1 iteration, no --afk, with --no-sandbox
    assert!(args.contains(" 1 "), "should have 1 iteration");
    assert!(!args.starts_with("-a "), "should not have --afk flag");
    assert!(
        args.contains("--no-sandbox"),
        "spec should pass --no-sandbox"
    );
}

#[test]
fn issues_log_runs_one_interactive_iteration() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

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
        .args(["issues", "log"])
        .env("SGF_RALPH_BINARY", &mock_ralph)
        .env("PATH", &mock_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "sgf issues log failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let args = fs::read_to_string(&args_file).unwrap();
    assert!(args.contains(" 1 "), "should have 1 iteration");
    assert!(!args.starts_with("-a "), "should not have --afk flag");
    assert!(
        args.contains("--no-sandbox"),
        "issues log should pass --no-sandbox"
    );
    assert!(
        args.contains("issues-log-"),
        "loop ID should contain issues-log"
    );
}

// ===========================================================================
// Recovery
// ===========================================================================

#[test]
fn recovery_cleans_stale_state() {
    let tmp = setup_test_dir();
    sgf_init_and_commit(tmp.path());

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

    // test.md should reference uppercase .sgf/PENSA.md
    let test = fs::read_to_string(tmp.path().join(".sgf/prompts/test.md")).unwrap();
    assert!(
        test.contains(".sgf/PENSA.md"),
        "test.md should reference .sgf/PENSA.md (uppercase)"
    );
    assert!(
        !test.contains(".sgf/pensa.md"),
        "test.md should NOT reference .sgf/pensa.md (lowercase)"
    );

    // test.md should reference root BACKPRESSURE.md (uppercase, not inside .sgf/)
    assert!(
        test.contains("BACKPRESSURE.md"),
        "test.md should reference BACKPRESSURE.md (uppercase)"
    );
    assert!(
        !test.contains("backpressure.md"),
        "test.md should NOT reference backpressure.md (lowercase)"
    );
}

#[test]
fn end_to_end_build_loop_with_memento_injection() {
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

    // Mock ralph that just exits 0
    let mock_dir = TempDir::new().unwrap();
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

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

    // Check the assembled prompt
    let assembled =
        fs::read_to_string(tmp.path().join(".sgf/prompts/.assembled/build.md")).unwrap();

    // Should start with .sgf/MEMENTO.md content (study directives with @ prefix)
    assert!(
        assembled.starts_with("study `@specs/README.md`"),
        "assembled prompt should start with memento content, got: {}",
        assembled.lines().next().unwrap_or("")
    );
    assert!(
        assembled.contains("study `@BACKPRESSURE.md`"),
        "assembled prompt should contain root BACKPRESSURE study directive"
    );
    assert!(
        !assembled.contains("study `@.sgf/BACKPRESSURE.md`"),
        "assembled prompt should NOT reference .sgf/BACKPRESSURE.md"
    );
    assert!(
        assembled.contains("study `@.sgf/PENSA.md`"),
        "assembled prompt should contain PENSA study directive"
    );

    // Should NOT contain Read `memento.md` directive
    assert!(
        !assembled.contains("Read `memento.md`"),
        "assembled prompt should NOT contain 'Read `memento.md`'"
    );

    // Memento should reference uppercase filenames
    assert!(
        assembled.contains(".sgf/PENSA.md"),
        "assembled prompt should reference .sgf/PENSA.md (uppercase)"
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

    // (3) .sgf/MEMENTO.md contains 'study `@BACKPRESSURE.md`' (root reference)
    let memento = fs::read_to_string(tmp.path().join(".sgf/MEMENTO.md")).unwrap();
    assert!(
        memento.contains("study `@BACKPRESSURE.md`"),
        "MEMENTO.md should reference root BACKPRESSURE.md with @ prefix"
    );
    assert!(
        !memento.contains("study `@.sgf/BACKPRESSURE.md`"),
        "MEMENTO.md should NOT reference .sgf/BACKPRESSURE.md"
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

    // (5) Prompt assembly produces assembled prompt with correct memento directives
    let (_mock_pn_dir, mock_path) = setup_mock_pn();

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
    let mock_ralph = create_mock_script(mock_dir.path(), "mock_ralph.sh", "#!/bin/sh\nexit 0\n");

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

    let assembled =
        fs::read_to_string(tmp.path().join(".sgf/prompts/.assembled/build.md")).unwrap();

    // Assembled prompt should start with memento directives
    assert!(
        assembled.starts_with("study `@specs/README.md`"),
        "assembled prompt should start with memento, got: {}",
        assembled.lines().next().unwrap_or("")
    );
    // Root BACKPRESSURE.md reference (not .sgf/)
    assert!(
        assembled.contains("study `@BACKPRESSURE.md`"),
        "assembled prompt should reference root BACKPRESSURE.md"
    );
    assert!(
        !assembled.contains("study `@.sgf/BACKPRESSURE.md`"),
        "assembled prompt should NOT reference .sgf/BACKPRESSURE.md"
    );
    // PENSA.md still inside .sgf/
    assert!(
        assembled.contains("study `@.sgf/PENSA.md`"),
        "assembled prompt should reference .sgf/PENSA.md"
    );
}
