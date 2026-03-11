use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn setup_test_dir() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    fs::write(dir.path().join("prompt.md"), "Test prompt.\n").expect("write prompt.md");
    // Initialize a git repo with an initial commit so git_head() works
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git command");
    };
    run(&["init"]);
    run(&[
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@test.com",
        "commit",
        "--allow-empty",
        "-m",
        "init",
    ]);
    dir
}

fn create_mock_script(dir: &TempDir, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let script_path = dir.path().join("mock.sh");
    let content = format!("#!/bin/bash\ncat {}\n", fixture_path.display());
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");
    script_path
}

fn create_mock_script_with_sentinel(dir: &TempDir, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let script_path = dir.path().join("mock.sh");
    let content = format!(
        "#!/bin/bash\ncat {}\ntouch .ralph-complete\n",
        fixture_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");
    script_path
}

fn create_mock_script_with_nested_sentinel(
    dir: &TempDir,
    fixture_name: &str,
    subdir: &str,
) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let nested_dir = dir.path().join(subdir);
    fs::create_dir_all(&nested_dir).expect("create nested dir");
    let script_path = dir.path().join("mock.sh");
    let sentinel_path = nested_dir.join(".ralph-complete");
    let content = format!(
        "#!/bin/bash\ncat {}\ntouch {}\n",
        fixture_path.display(),
        sentinel_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");
    script_path
}

fn ralph_cmd(dir: &TempDir) -> Command {
    let bin = env!("CARGO_BIN_EXE_ralph");
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir.path());
    cmd.env("RALPH_AUTO_PUSH", "false");
    cmd.env("RUST_LOG", "warn");
    cmd.env("PROMPT_FILES", "");
    cmd.env("SGF_MANAGED", "1");
    cmd.stdin(std::process::Stdio::null());
    cmd
}

#[test]
fn afk_formats_text_blocks() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

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
fn afk_formats_tool_calls_as_one_liners() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Read tool calls (new styled format: "  ─ Read  detail")
    assert!(
        stdout.contains("─ Read  /Users/william/Repos/buddy-ralph/specs/README.md"),
        "should contain Read tool call, got:\n{stdout}"
    );
    // Read with offset and limit
    assert!(
        stdout.contains(
            "─ Read  /Users/william/Repos/buddy-ralph/crates/buddy-llm/src/inference.rs 1:80"
        ),
        "should contain Read with offset:limit, got:\n{stdout}"
    );
    // Edit tool (file path only)
    assert!(
        stdout.contains("─ Edit  /Users/william/Repos/buddy-ralph/specs/tokenizer-embedding.md"),
        "should contain Edit tool call, got:\n{stdout}"
    );
    // TodoWrite (count)
    assert!(
        stdout.contains("─ TodoWrite  3 items"),
        "should contain TodoWrite with count, got:\n{stdout}"
    );
    // Bash tool
    assert!(
        stdout.contains("─ Bash  git diff specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md"),
        "should contain Bash tool call, got:\n{stdout}"
    );
    // Grep tool
    assert!(
        stdout.contains("─ Grep  GgufModelBuilder"),
        "should contain Grep tool call, got:\n{stdout}"
    );
    // Glob tool
    assert!(
        stdout.contains("─ Glob  specs/**/*.md"),
        "should contain Glob tool call, got:\n{stdout}"
    );

    // Must NOT contain raw Edit arg keys
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
    // Must NOT contain content from Edit old_string value
    assert!(
        !stdout.contains("GgufModelBuilder::new"),
        "should not contain content from Edit old_string value"
    );
    // Must NOT contain old jq-style key: value formatting
    assert!(
        !stdout.contains("file_path:"),
        "should not contain old jq-style 'file_path:' formatting"
    );
}

#[test]
fn afk_detects_completion_file() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

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
        stdout.contains("That was the last remaining task"),
        "should contain final text from fixture, got:\n{stdout}"
    );
    assert!(
        !dir.path().join(".ralph-complete").exists(),
        "sentinel file should be cleaned up after ralph exits"
    );
}

#[test]
fn afk_exhausts_iterations_without_promise() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "should exit non-zero when no promise found"
    );
    assert_eq!(output.status.code(), Some(2), "should exit with code 2");
    assert!(
        stdout.contains("reached max iterations"),
        "should contain max iterations message, got:\n{stdout}"
    );
}

#[test]
fn explicit_nonexistent_file_treated_as_inline_text() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    // "nonexistent.md" doesn't exist as a file, so it's treated as inline text
    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "nonexistent.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should NOT error with "not found" — treated as inline text
    assert!(
        !stderr.contains("not found"),
        "explicit nonexistent file should be treated as inline text, got stderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "should exit 0 (mock creates sentinel), got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    assert!(
        stdout.contains("(text)"),
        "banner should show (text) suffix, got:\n{stdout}"
    );
}

#[test]
fn iterations_clamped_to_max() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--max-iterations",
            "5",
            "--command",
            mock.to_str().unwrap(),
            "100",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("reducing iterations to max allowed"),
        "should warn about clamping, got stderr:\n{stderr}"
    );
}

#[test]
fn help_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_ralph"))
        .arg("--help")
        .output()
        .expect("run ralph --help");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "help should exit 0");
    assert!(
        stdout.contains("Iterative Claude Code runner"),
        "help should contain description, got:\n{stdout}"
    );
}

#[test]
fn bash_command_truncation() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The long git add && git commit command (with heredoc newlines) gets truncated
    // at 100 chars by the Bash formatter, which appends "...".
    // Verify the truncation ellipsis appears in the new styled format.
    assert!(
        stdout.contains("─ Bash  git add specs/tokenizer-embedding.md"),
        "should contain the long Bash tool call, got stdout:\n{stdout}"
    );

    // The truncated command should end with "..." somewhere in the output.
    let start = stdout
        .find("─ Bash  git add specs/tokenizer-embedding.md")
        .expect("should find Bash tool call");
    let rest = &stdout[start..];
    assert!(
        rest.contains("..."),
        "truncated Bash command should contain '...', got:\n{rest}"
    );

    // Short Bash commands (like "git log --oneline -5") should NOT be truncated
    assert!(
        stdout.contains("─ Bash  git log --oneline -5"),
        "short Bash command should not be truncated, got stdout:\n{stdout}"
    );
}

#[test]
fn afk_detects_nested_completion_file() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_nested_sentinel(&dir, "complete.ndjson", "sub/project");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

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
        !dir.path().join("sub/project/.ralph-complete").exists(),
        "nested sentinel file should be cleaned up after ralph exits"
    );
}

#[test]
fn inline_text_prompt() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "fix the bug",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should not fail with "not found" error since "fix the bug" is inline text
    assert!(
        !stderr.contains("not found"),
        "inline text should not trigger missing file error, got stderr:\n{stderr}"
    );
    // Should run successfully (mock creates sentinel)
    assert!(
        output.status.success(),
        "should exit 0 with inline text prompt, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    // Banner should show (text) suffix
    assert!(
        stdout.contains("(text)"),
        "banner should show (text) suffix for inline prompt, got:\n{stdout}"
    );
}

#[test]
fn iterations_defaults_to_one() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args(["--afk", "--command", mock.to_str().unwrap()])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0 with default iteration count, got: {:?}\nstdout:\n{stdout}",
        output.status.code()
    );
    assert!(
        stdout.contains("Iteration 1 of 1"),
        "should run exactly 1 iteration by default, got:\n{stdout}"
    );
}

#[test]
fn default_prompt_missing() {
    let dir = TempDir::new().expect("create temp dir");
    // Initialize git but do NOT create prompt.md
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git command");
    };
    run(&["init"]);
    run(&[
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@test.com",
        "commit",
        "--allow-empty",
        "-m",
        "init",
    ]);

    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 when default prompt.md is missing"
    );
    assert!(
        stderr.contains("not found"),
        "stderr should mention 'not found', got:\n{stderr}"
    );
}

#[test]
fn loop_id_in_startup_banner() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--loop-id",
            "build-auth-20260226T143000",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Loop ID:     build-auth-20260226T143000"),
        "startup banner should contain loop ID, got:\n{stdout}"
    );
}

#[test]
fn loop_id_in_iteration_banner() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--loop-id",
            "build-auth-20260226T143000",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Iteration 1 of 1 [build-auth-20260226T143000]"),
        "iteration banner should contain loop ID, got:\n{stdout}"
    );
}

#[test]
fn no_loop_id_when_not_provided() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("Loop ID"),
        "should not contain Loop ID label when --loop-id is not provided, got:\n{stdout}"
    );
    // Iteration banner title should not contain a loop ID in brackets
    assert!(
        stdout.contains("Iteration 1 of 1"),
        "iteration banner should contain iteration info, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Iteration 1 of 1 ["),
        "iteration banner should not contain loop ID, got:\n{stdout}"
    );
}

#[test]
fn agent_cmd_in_banner() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains(&format!("Agent:       {}", mock.display())),
        "startup banner should show agent command, got:\n{stdout}"
    );
}

#[test]
fn explicit_file_prompt_shows_file_suffix() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("(file)"),
        "banner should show (file) suffix for file prompt, got:\n{stdout}"
    );
}

#[test]
fn spec_flag_missing_file_exits_1() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--spec",
            "nonexistent",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 when spec file is missing"
    );
    assert!(
        stderr.contains("spec file not found"),
        "stderr should mention spec file not found, got:\n{stderr}"
    );
}

#[test]
fn spec_flag_existing_file_accepted() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let specs_dir = dir.path().join("specs");
    fs::create_dir_all(&specs_dir).expect("create specs dir");
    fs::write(specs_dir.join("auth.md"), "# Auth spec\n").expect("write spec file");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--spec",
            "auth",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0 with valid spec, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
}

#[test]
fn prompt_file_missing_exits_1() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--prompt-file",
            "./does-not-exist.md",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 when prompt file is missing"
    );
    assert!(
        stderr.contains("prompt file not found"),
        "stderr should mention prompt file not found, got:\n{stderr}"
    );
}

#[test]
fn prompt_file_existing_accepted() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    fs::write(dir.path().join("NOTES.md"), "# Notes\n").expect("write notes file");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--prompt-file",
            "./NOTES.md",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0 with valid prompt file, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
}

#[test]
fn multiple_prompt_files_accepted() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    fs::write(dir.path().join("NOTES.md"), "# Notes\n").expect("write notes");
    fs::write(dir.path().join("EXTRA.md"), "# Extra\n").expect("write extra");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--prompt-file",
            "./NOTES.md",
            "--prompt-file",
            "./EXTRA.md",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0 with multiple prompt files, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
}

#[test]
fn spec_via_env_var() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let specs_dir = dir.path().join("specs");
    fs::create_dir_all(&specs_dir).expect("create specs dir");
    fs::write(specs_dir.join("auth.md"), "# Auth spec\n").expect("write spec file");

    let output = ralph_cmd(&dir)
        .env("SGF_SPEC", "auth")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0 with SGF_SPEC env var, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
}

#[test]
fn prompt_files_default_warns_when_unset() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ralph"));
    cmd.current_dir(dir.path());
    cmd.env("RALPH_AUTO_PUSH", "false");
    cmd.env("RUST_LOG", "warn");
    cmd.env_remove("PROMPT_FILES");
    cmd.args([
        "--afk",
        "--command",
        mock.to_str().unwrap(),
        "1",
        "prompt.md",
    ]);

    let output = cmd.output().expect("run ralph");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("PROMPT_FILES not set"),
        "should warn when PROMPT_FILES is not set, got stderr:\n{stderr}"
    );
}

#[test]
fn prompt_files_resolves_existing_files() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    fs::write(dir.path().join("BACKPRESSURE.md"), "# BP\n").expect("write bp");
    fs::write(dir.path().join("README.md"), "# README\n").expect("write readme");

    let output = ralph_cmd(&dir)
        .env(
            "PROMPT_FILES",
            format!(
                "{}:{}",
                dir.path().join("BACKPRESSURE.md").display(),
                dir.path().join("README.md").display()
            ),
        )
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("not found"),
        "should not warn for existing PROMPT_FILES entries, got stderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "should exit 0 with valid PROMPT_FILES"
    );
}

#[test]
fn prompt_files_warns_on_missing_entries() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env("PROMPT_FILES", "/nonexistent/file.md:./also-missing.md")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("PROMPT_FILES entry not found"),
        "should warn about missing PROMPT_FILES entries, got stderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "should still succeed (missing PROMPT_FILES entries are non-fatal)"
    );
}

#[test]
fn prompt_files_expands_home() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let fake_home = dir.path().join("fakehome");
    fs::create_dir_all(&fake_home).expect("create fake home");
    fs::write(fake_home.join(".ralph-test-prompt-file.md"), "# Test\n").expect("write test file");

    let output = ralph_cmd(&dir)
        .env("HOME", &fake_home)
        .env("PROMPT_FILES", "$HOME/.ralph-test-prompt-file.md")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("not found"),
        "should resolve $HOME in PROMPT_FILES paths, got stderr:\n{stderr}"
    );
}

#[test]
fn prompt_files_expands_tilde() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let fake_home = dir.path().join("fakehome");
    fs::create_dir_all(&fake_home).expect("create fake home");
    fs::write(fake_home.join(".ralph-test-tilde-file.md"), "# Test\n").expect("write test file");

    let output = ralph_cmd(&dir)
        .env("HOME", &fake_home)
        .env("PROMPT_FILES", "~/.ralph-test-tilde-file.md")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("not found"),
        "should resolve ~ in PROMPT_FILES paths, got stderr:\n{stderr}"
    );
}

#[test]
fn prompt_files_empty_value_yields_no_files() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env("PROMPT_FILES", "")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stderr.contains("not found"),
        "empty PROMPT_FILES should not warn about missing files, got stderr:\n{stderr}"
    );
    assert!(
        output.status.success(),
        "should exit 0 with empty PROMPT_FILES"
    );
}

#[test]
fn spec_flag_overrides_env_var() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let specs_dir = dir.path().join("specs");
    fs::create_dir_all(&specs_dir).expect("create specs dir");
    fs::write(specs_dir.join("auth.md"), "# Auth spec\n").expect("write auth spec");

    // SGF_SPEC=auth but --spec=nonexistent should use the flag value
    let output = ralph_cmd(&dir)
        .env("SGF_SPEC", "auth")
        .args([
            "--afk",
            "--spec",
            "nonexistent",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 when --spec flag overrides env with nonexistent spec"
    );
}

fn create_arg_capturing_mock(dir: &TempDir, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let args_file = dir.path().join("captured-args.txt");
    let script_path = dir.path().join("mock.sh");
    let content = format!(
        "#!/bin/bash\nprintf '%s\\n' \"$@\" > {}\ncat {}\ntouch .ralph-complete\n",
        args_file.display(),
        fixture_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");
    script_path
}

#[test]
fn prompt_files_passed_as_append_system_prompt_file_args() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    fs::write(dir.path().join("BACKPRESSURE.md"), "# BP\n").expect("write bp");
    fs::write(dir.path().join("NOTES.md"), "# Notes\n").expect("write notes");

    let bp_path = dir.path().join("BACKPRESSURE.md");
    let notes_path = dir.path().join("NOTES.md");

    let output = ralph_cmd(&dir)
        .env(
            "PROMPT_FILES",
            format!("{}:{}", bp_path.display(), notes_path.display()),
        )
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    // Find the --append-system-prompt value containing study instructions
    let asp_value: Option<&str> = arg_lines
        .windows(2)
        .find(|w| w[0] == "--append-system-prompt")
        .map(|w| w[1]);

    let study = asp_value.expect("should pass --append-system-prompt with study instructions");

    assert!(
        study.contains("study @") && study.contains("BACKPRESSURE.md"),
        "should include study @...BACKPRESSURE.md, got: {study}"
    );
    assert!(
        study.contains("NOTES.md"),
        "should include study @...NOTES.md, got: {study}"
    );
}

#[test]
fn prompt_files_missing_entries_not_passed_as_args() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    fs::write(dir.path().join("EXISTS.md"), "# Exists\n").expect("write exists");

    let output = ralph_cmd(&dir)
        .env(
            "PROMPT_FILES",
            format!(
                "{}:/nonexistent/file.md",
                dir.path().join("EXISTS.md").display()
            ),
        )
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    assert!(
        stderr.contains("PROMPT_FILES entry not found"),
        "should warn about missing entry, got stderr:\n{stderr}"
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    let asp_value: Option<&str> = arg_lines
        .windows(2)
        .find(|w| w[0] == "--append-system-prompt")
        .map(|w| w[1]);

    let study = asp_value.expect("should pass --append-system-prompt with study instructions");

    assert!(
        study.contains("study @") && study.contains("EXISTS.md"),
        "should include study @...EXISTS.md, got: {study}"
    );
    assert!(
        !study.contains("nonexistent"),
        "should NOT include nonexistent file in study instruction, got: {study}"
    );
}

#[test]
fn prompt_files_default_entries_passed_when_unset() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    // Create some of the default PROMPT_FILES entries
    fs::write(dir.path().join("BACKPRESSURE.md"), "# BP\n").expect("write bp");
    let specs_dir = dir.path().join("specs");
    fs::create_dir_all(&specs_dir).expect("create specs dir");
    fs::write(specs_dir.join("README.md"), "# Specs\n").expect("write specs readme");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ralph"));
    cmd.current_dir(dir.path());
    cmd.env("RALPH_AUTO_PUSH", "false");
    cmd.env("RUST_LOG", "warn");
    cmd.env_remove("PROMPT_FILES");
    cmd.env("SGF_MANAGED", "1");
    cmd.stdin(std::process::Stdio::null());
    // Isolate HOME to temp dir so $HOME/.MEMENTO.md won't resolve to host file
    cmd.env("HOME", dir.path());
    cmd.args([
        "--afk",
        "--command",
        mock.to_str().unwrap(),
        "1",
        "prompt.md",
    ]);

    let output = cmd.output().expect("run ralph");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    assert!(
        stderr.contains("PROMPT_FILES not set"),
        "should warn about unset PROMPT_FILES, got stderr:\n{stderr}"
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    let asp_value: Option<&str> = arg_lines
        .windows(2)
        .find(|w| w[0] == "--append-system-prompt")
        .map(|w| w[1]);

    let study = asp_value.expect("should pass --append-system-prompt with study instructions");

    // Default includes ./BACKPRESSURE.md and ./specs/README.md which we created
    assert!(
        study.contains("BACKPRESSURE.md"),
        "default PROMPT_FILES should include BACKPRESSURE.md, got: {study}"
    );
    assert!(
        study.contains("specs/README.md"),
        "default PROMPT_FILES should include specs/README.md, got: {study}"
    );
    // HOME is set to temp dir, so $HOME/.MEMENTO.md should not exist
    assert!(
        !study.contains("MEMENTO.md"),
        "default should skip non-existent $HOME/.MEMENTO.md, got: {study}"
    );
}

fn create_slow_mock_script(dir: &TempDir, fixture_name: &str) -> PathBuf {
    let fixture_path = fixtures_dir().join(fixture_name);
    let script_path = dir.path().join("mock.sh");
    let content = format!(
        "#!/bin/bash\ntrap '' INT\nfor i in $(seq 1 50); do cat {}; sleep 0.1; done\n",
        fixture_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");
    script_path
}

#[test]
fn afk_double_ctrlc_aborts() {
    let dir = setup_test_dir();
    let mock = create_slow_mock_script(&dir, "afk-session.ndjson");

    let child = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "5",
            "prompt.md",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ralph");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child.wait_with_output().expect("wait for ralph");
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
fn interactive_double_sigint_exits_130() {
    let dir = setup_test_dir();
    let script_path = dir.path().join("mock_slow_interactive.sh");
    fs::write(&script_path, "#!/bin/bash\ntrap '' INT\nsleep 5\n").expect("write mock");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");

    let child = ralph_cmd(&dir)
        .args(["--command", script_path.to_str().unwrap(), "1", "prompt.md"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ralph");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send first SIGINT");
    std::thread::sleep(Duration::from_millis(200));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send second SIGINT");

    let output = child.wait_with_output().expect("wait for ralph");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on double SIGINT in interactive mode"
    );
}

#[test]
fn afk_single_ctrlc_resets_after_timeout() {
    let dir = setup_test_dir();
    let mock = create_slow_mock_script(&dir, "afk-session.ndjson");

    let child = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ralph");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send single SIGINT");

    let output = child.wait_with_output().expect("wait for ralph");

    assert_eq!(
        output.status.code(),
        Some(2),
        "should exit 2 (iterations exhausted) after single Ctrl+C timeout"
    );
}

#[test]
fn spec_passes_append_system_prompt_file_arg() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    let specs_dir = dir.path().join("specs");
    fs::create_dir_all(&specs_dir).expect("create specs dir");
    fs::write(specs_dir.join("auth.md"), "# Auth spec\n").expect("write spec file");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--spec",
            "auth",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    let asp_value: Option<&str> = arg_lines
        .windows(2)
        .find(|w| w[0] == "--append-system-prompt")
        .map(|w| w[1]);

    let study = asp_value.expect("should pass --append-system-prompt with study instructions");
    assert!(
        study.contains("study @./specs/auth.md"),
        "study instruction should include spec path, got: {study}"
    );
}

#[test]
fn afk_log_file_captures_output() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");
    let log_path = dir.path().join("logs/test.log");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "--log-file",
            log_path.to_str().unwrap(),
            "1",
        ])
        .output()
        .expect("run ralph");

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );

    assert!(log_path.exists(), "log file should be created");

    let log_content = fs::read_to_string(&log_path).expect("read log file");
    assert!(
        log_content.contains("Iteration 1 of 1"),
        "log should contain iteration banner, got: {log_content}"
    );
    assert!(
        log_content.contains("Ralph COMPLETE"),
        "log should contain completion banner, got: {log_content}"
    );
}

#[test]
fn afk_log_file_creates_parent_dirs() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");
    let log_path = dir.path().join("deeply/nested/dir/test.log");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "--log-file",
            log_path.to_str().unwrap(),
            "1",
        ])
        .output()
        .expect("run ralph");

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );
    assert!(
        log_path.exists(),
        "log file should be created in nested dir"
    );
}

#[test]
fn no_log_file_by_default() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    assert!(output.status.success());

    let log_files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .collect();
    assert!(
        log_files.is_empty(),
        "no log file should be created by default"
    );
}

#[test]
fn agent_cmd_required_without_command_override() {
    let dir = setup_test_dir();

    let output = ralph_cmd(&dir)
        .env_remove("AGENT_CMD")
        .env_remove("RALPH_COMMAND")
        .args(["--afk", "1", "prompt.md"])
        .output()
        .expect("run ralph");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "should exit 1 when AGENT_CMD is not set and --command is not provided"
    );
    assert!(
        stderr.contains("AGENT_CMD not set"),
        "stderr should mention AGENT_CMD not set, got:\n{stderr}"
    );
}

#[test]
fn agent_cmd_not_required_with_command_override() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env_remove("AGENT_CMD")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    assert!(
        output.status.success(),
        "should exit 0 when --command is provided even without AGENT_CMD"
    );
}

#[test]
fn afk_invocation_passes_correct_flags() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
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
    assert!(
        arg_lines.contains(
            &r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#
        ),
        "should pass autoMemoryEnabled:false settings, got args:\n{args}"
    );

    let last_arg = arg_lines.last().expect("should have args");
    assert_eq!(
        *last_arg, "@prompt.md",
        "last arg should be @prompt.md (file prompt), got: {last_arg}"
    );
}

#[test]
fn interactive_invocation_passes_correct_flags() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args(["--command", mock.to_str().unwrap(), "1", "prompt.md"])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
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
    assert_eq!(
        *last_arg, "@prompt.md",
        "last arg should be @prompt.md (file prompt), got: {last_arg}"
    );
}

#[test]
fn inline_text_prompt_not_prefixed_with_at() {
    let dir = setup_test_dir();
    let mock = create_arg_capturing_mock(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "fix the login bug",
        ])
        .output()
        .expect("run ralph");

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );

    let args =
        fs::read_to_string(dir.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();

    let last_arg = arg_lines.last().expect("should have args");
    assert_eq!(
        *last_arg, "fix the login bug",
        "inline text should be passed without @ prefix, got: {last_arg}"
    );
}

#[test]
fn auto_push_pushes_when_head_changes() {
    let dir = setup_test_dir();

    let bare_dir = TempDir::new().expect("create bare repo dir");
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git command")
    };

    run_git(&["init", "--bare"], bare_dir.path());
    run_git(
        &["remote", "add", "origin", bare_dir.path().to_str().unwrap()],
        dir.path(),
    );
    run_git(&["push", "-u", "origin", "master"], dir.path());

    let fixture_path = fixtures_dir().join("complete.ndjson");
    let script_path = dir.path().join("mock.sh");
    let content = format!(
        concat!(
            "#!/bin/bash\n",
            "cat {fixture}\n",
            "echo 'auto-push test' > change.txt\n",
            "git -c user.name=test -c user.email=test@test.com add change.txt\n",
            "git -c user.name=test -c user.email=test@test.com commit -m 'test commit'\n",
            "touch .ralph-complete\n",
        ),
        fixture = fixture_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");

    let output = ralph_cmd(&dir)
        .env("RALPH_AUTO_PUSH", "true")
        .args([
            "--afk",
            "--command",
            script_path.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

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
fn auto_push_disabled_does_not_push() {
    let dir = setup_test_dir();

    let fixture_path = fixtures_dir().join("complete.ndjson");
    let script_path = dir.path().join("mock.sh");
    let content = format!(
        concat!(
            "#!/bin/bash\n",
            "cat {fixture}\n",
            "echo 'no-push test' > change.txt\n",
            "git -c user.name=test -c user.email=test@test.com add change.txt\n",
            "git -c user.name=test -c user.email=test@test.com commit -m 'test commit'\n",
            "touch .ralph-complete\n",
        ),
        fixture = fixture_path.display()
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            script_path.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "should exit 0, got: {:?}",
        output.status.code()
    );

    assert!(
        !stdout.contains("New commits detected, pushing..."),
        "should NOT push when auto_push is false, got stdout:\n{stdout}"
    );
}

#[test]
fn afk_startup_banner_uses_box_format() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ Ralph Loop Starting"),
        "startup banner should use box format, got:\n{stdout}"
    );
}

#[test]
fn afk_iteration_banner_uses_box_format() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ Iteration 1 of"),
        "iteration banner should use box format, got:\n{stdout}"
    );
}

#[test]
fn afk_completion_banner_uses_box_format() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("╭─ Ralph COMPLETE"),
        "completion banner should use box format, got:\n{stdout}"
    );
}

#[test]
fn afk_max_iterations_banner_uses_box_format() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(output.status.code(), Some(2), "should exit with code 2");
    assert!(
        stdout.contains("╭─ Ralph reached max iterations"),
        "max-iterations banner should use box format, got:\n{stdout}"
    );
}

#[test]
fn afk_no_color_disables_ansi() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Check for ANSI color/style codes (\x1b[...m) — not cursor control codes
    // like \r\x1b[2K which are always emitted for line clearing
    let has_ansi_style = stdout.as_bytes().windows(2).enumerate().any(|(i, w)| {
        if w == b"\x1b[" {
            // Look for pattern \x1b[<digits>m (ANSI SGR codes)
            let rest = &stdout.as_bytes()[i + 2..];
            // Skip \x1b[2K (clear line) — that's not a style code
            if rest.first() == Some(&b'2') && rest.get(1) == Some(&b'K') {
                return false;
            }
            // Any \x1b[<digits>m or \x1b[<digits>;<digits>m is a style code
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
    assert!(
        stdout.contains("╭─ Ralph Loop Starting"),
        "should still contain box-drawing chars without ANSI, got:\n{stdout}"
    );
}

#[test]
fn afk_hides_tool_results() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env("NO_COLOR", "1")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("# Buddy Ralph Specifications"),
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
fn afk_tool_calls_have_ansi_colors() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env_remove("NO_COLOR")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Read tool calls should use bold blue (1;34)
    assert!(
        stdout.contains("\x1b[1;34mRead\x1b[0m"),
        "Read tool call should have bold blue ANSI codes, got:\n{stdout}"
    );

    // Bash tool calls should use bold yellow (1;33)
    assert!(
        stdout.contains("\x1b[1;33mBash\x1b[0m"),
        "Bash tool call should have bold yellow ANSI codes, got:\n{stdout}"
    );

    // Edit tool calls should use bold magenta (1;35)
    assert!(
        stdout.contains("\x1b[1;35mEdit\x1b[0m"),
        "Edit tool call should have bold magenta ANSI codes, got:\n{stdout}"
    );

    // Grep tool calls should use bold blue (1;34)
    assert!(
        stdout.contains("\x1b[1;34mGrep\x1b[0m"),
        "Grep tool call should have bold blue ANSI codes, got:\n{stdout}"
    );

    // Glob tool calls should use bold blue (1;34)
    assert!(
        stdout.contains("\x1b[1;34mGlob\x1b[0m"),
        "Glob tool call should have bold blue ANSI codes, got:\n{stdout}"
    );

    // TodoWrite tool calls should use bold cyan (1;36, default for unknown tools)
    assert!(
        stdout.contains("\x1b[1;36mTodoWrite\x1b[0m"),
        "TodoWrite tool call should have bold cyan ANSI codes, got:\n{stdout}"
    );

    // Tool call details should use cyan (36)
    assert!(
        stdout.contains("\x1b[36m"),
        "tool call details should have cyan ANSI codes, got:\n{stdout}"
    );
}

#[test]
fn afk_agent_text_has_bold_cyan_ansi() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .env_remove("NO_COLOR")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Agent text lines should be wrapped in cyan+bold (\x1b[36m\x1b[1m...\x1b[0m\x1b[0m)
    assert!(
        stdout.contains("\x1b[36m\x1b[1mI'll start by studying the required files"),
        "agent text should have cyan+bold ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[36m\x1b[1mNow I can see the cleanup plan."),
        "agent text should have cyan+bold ANSI codes, got:\n{stdout}"
    );
}

#[test]
fn afk_test_result_lines_have_color_ansi() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "test-results.ndjson");

    let output = ralph_cmd(&dir)
        .env_remove("NO_COLOR")
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Lines ending with "... ok" should have green ANSI codes (32)
    assert!(
        stdout.contains("\x1b[32mtest inference::test_load_model ... ok\x1b[0m"),
        "test '... ok' lines should have green ANSI codes, got:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b[32mtest inference::test_tokenize ... ok\x1b[0m"),
        "test '... ok' lines should have green ANSI codes, got:\n{stdout}"
    );

    // Lines ending with "... FAILED" should have red ANSI codes (31)
    assert!(
        stdout.contains("\x1b[31mtest inference::test_generate ... FAILED\x1b[0m"),
        "test '... FAILED' lines should have red ANSI codes, got:\n{stdout}"
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

// AFK mode uses setsid() for child processes. On macOS, when a session leader exits,
// the kernel revokes the pty slave, invalidating fd 0 for subsequent iterations.
// This prevents PTY-based terminal restoration testing in AFK mode.
// The save/restore code is mode-independent (same tcgetattr/tcsetattr in main loop),
// so the interactive test below validates the core functionality for both modes.
#[test]
fn afk_terminal_restore_graceful_with_non_tty_stdin() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "complete.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "2",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    assert_eq!(
        output.status.code(),
        Some(2),
        "should complete 2 iterations — save/restore is a no-op when stdin is not a tty"
    );
}

#[test]
fn interactive_restores_terminal_settings_after_agent_corrupts() {
    let Some((master, slave)) = open_pty() else {
        eprintln!("skipping: PTY allocation unavailable (sandboxed environment)");
        return;
    };
    let _master = master;

    let dir = setup_test_dir();
    let captures_file = dir.path().join("termios-captures.txt");

    let check_cmd = termios_check_script();
    let corrupt_cmd = termios_corrupt_script();
    let script_path = dir.path().join("mock_termcorrupt.sh");
    let content = format!(
        "#!/bin/bash\n{check_cmd} >> {captures} 2>/dev/null\n{corrupt_cmd} 2>/dev/null\n",
        captures = captures_file.display(),
        check_cmd = check_cmd,
        corrupt_cmd = corrupt_cmd,
    );
    fs::write(&script_path, content).expect("write mock script");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");

    let bin = env!("CARGO_BIN_EXE_ralph");
    let output = Command::new(bin)
        .current_dir(dir.path())
        .env("RALPH_AUTO_PUSH", "false")
        .env("RUST_LOG", "warn")
        .env("PROMPT_FILES", "")
        .env("SGF_MANAGED", "1")
        .stdin(std::process::Stdio::from(slave))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .args(["--command", script_path.to_str().unwrap(), "2", "prompt.md"])
        .output()
        .expect("run ralph");

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
