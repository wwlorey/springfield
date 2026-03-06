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

    // Read tool calls
    assert!(
        stdout.contains("-> Read(/Users/william/Repos/buddy-ralph/specs/README.md)"),
        "should contain Read tool call, got:\n{stdout}"
    );
    // Read with offset and limit
    assert!(
        stdout.contains(
            "-> Read(/Users/william/Repos/buddy-ralph/crates/buddy-llm/src/inference.rs 1:80)"
        ),
        "should contain Read with offset:limit, got:\n{stdout}"
    );
    // Edit tool (file path only)
    assert!(
        stdout.contains("-> Edit(/Users/william/Repos/buddy-ralph/specs/tokenizer-embedding.md)"),
        "should contain Edit tool call, got:\n{stdout}"
    );
    // TodoWrite (count)
    assert!(
        stdout.contains("-> TodoWrite(3 items)"),
        "should contain TodoWrite with count, got:\n{stdout}"
    );
    // Bash tool
    assert!(
        stdout
            .contains("-> Bash(git diff specs/tokenizer-embedding.md plans/cleanup/buddy-llm.md)"),
        "should contain Bash tool call, got:\n{stdout}"
    );
    // Grep tool
    assert!(
        stdout.contains("-> Grep(GgufModelBuilder)"),
        "should contain Grep tool call, got:\n{stdout}"
    );
    // Glob tool
    assert!(
        stdout.contains("-> Glob(specs/**/*.md)"),
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
    // Since the command contains embedded newlines, the truncated output spans
    // multiple printed lines. Verify the truncation ellipsis appears.
    assert!(
        stdout.contains("-> Bash(git add specs/tokenizer-embedding.md"),
        "should contain the long Bash tool call, got stdout:\n{stdout}"
    );

    // The truncated command should end with "...)" somewhere in the output.
    // Find the "-> Bash(git add specs/" portion and check that the full
    // formatted block ends with "...)"
    let start = stdout
        .find("-> Bash(git add specs/tokenizer-embedding.md")
        .expect("should find Bash tool call");
    let rest = &stdout[start..];
    // The formatted tool call ends with "...)\n" since truncation adds "..."
    // and format_tool_call wraps in "-> Name(detail)"
    assert!(
        rest.contains("...)"),
        "truncated Bash command should contain '...)', got:\n{rest}"
    );

    // Short Bash commands (like "git log --oneline -5") should NOT be truncated
    assert!(
        stdout.contains("-> Bash(git log --oneline -5)"),
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
    // Iteration banner should be plain "Iteration N of M" without brackets
    assert!(
        stdout.contains("Iteration 1 of 1\n"),
        "iteration banner should not contain loop ID, got:\n{stdout}"
    );
}

#[test]
fn default_template_in_banner() {
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
        stdout.contains("ralph-sandbox:latest"),
        "startup banner should show default template ralph-sandbox:latest, got:\n{stdout}"
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
fn system_file_missing_exits_1() {
    let dir = setup_test_dir();
    let mock = create_mock_script(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--system-file",
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
        "should exit 1 when system file is missing"
    );
    assert!(
        stderr.contains("system file not found"),
        "stderr should mention system file not found, got:\n{stderr}"
    );
}

#[test]
fn system_file_existing_accepted() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    fs::write(dir.path().join("NOTES.md"), "# Notes\n").expect("write notes file");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--system-file",
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
        "should exit 0 with valid system file, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
}

#[test]
fn multiple_system_files_accepted() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "complete.ndjson");

    fs::write(dir.path().join("NOTES.md"), "# Notes\n").expect("write notes");
    fs::write(dir.path().join("EXTRA.md"), "# Extra\n").expect("write extra");

    let output = ralph_cmd(&dir)
        .args([
            "--afk",
            "--system-file",
            "./NOTES.md",
            "--system-file",
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
        "should exit 0 with multiple system files, got: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
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

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let test_file = PathBuf::from(&home).join(".ralph-test-prompt-file.md");
    fs::write(&test_file, "# Test\n").expect("write test file in HOME");

    let output = ralph_cmd(&dir)
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

    let _ = fs::remove_file(&test_file);

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

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let test_file = PathBuf::from(&home).join(".ralph-test-tilde-file.md");
    fs::write(&test_file, "# Test\n").expect("write test file in HOME");

    let output = ralph_cmd(&dir)
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

    let _ = fs::remove_file(&test_file);

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
    assert!(
        stdout.contains("Press Ctrl+C again to stop"),
        "should show double-press prompt, got:\n{stdout}"
    );
}

#[test]
fn interactive_sigint_exits_130() {
    let dir = setup_test_dir();
    let script_path = dir.path().join("mock_slow_interactive.sh");
    fs::write(&script_path, "#!/bin/bash\ntrap '' INT\nsleep 1\n").expect("write mock");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod");

    let child = ralph_cmd(&dir)
        .args(["--command", script_path.to_str().unwrap(), "1", "prompt.md"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ralph");

    let pid = nix::unistd::Pid::from_raw(child.id() as i32);

    std::thread::sleep(Duration::from_millis(500));
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT).expect("send SIGINT");

    let output = child.wait_with_output().expect("wait for ralph");

    assert_eq!(
        output.status.code(),
        Some(130),
        "should exit 130 on SIGINT in interactive mode"
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
fn command_mode_does_not_create_daemon_url() {
    let dir = setup_test_dir();
    let pensa_dir = dir.path().join(".pensa");
    fs::create_dir_all(&pensa_dir).unwrap();
    fs::write(pensa_dir.join("daemon.port"), "12345").unwrap();

    let mock = create_mock_script_with_sentinel(&dir, "afk-session.ndjson");
    let output = ralph_cmd(&dir)
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    assert!(output.status.success());
    assert!(
        !pensa_dir.join("daemon.url").exists(),
        "daemon.url should NOT be created in --command mode"
    );
}

#[test]
fn no_sandbox_already_exists_output_leak() {
    let dir = setup_test_dir();
    let mock = create_mock_script_with_sentinel(&dir, "afk-session.ndjson");

    let output = ralph_cmd(&dir)
        .args(["--afk", "--command", mock.to_str().unwrap(), "1"])
        .output()
        .expect("run ralph");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !stdout.contains("already exists"),
        "stdout must not contain 'already exists' message, got:\n{stdout}"
    );
    assert!(
        !stderr.contains("already exists"),
        "stderr must not contain 'already exists' message, got:\n{stderr}"
    );
}

#[test]
fn external_prompt_file_staged_into_workspace() {
    let workspace = setup_test_dir();
    let external_dir = TempDir::new().expect("create external temp dir");
    let mock = create_arg_capturing_mock(&workspace, "complete.ndjson");

    let memento_path = external_dir.path().join(".MEMENTO.md");
    fs::write(&memento_path, "# Memento\npn cheat sheet here\n").expect("write memento");

    let output = ralph_cmd(&workspace)
        .env("PROMPT_FILES", memento_path.to_str().unwrap())
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

    let staged = workspace.path().join(".sgf/prompts/.MEMENTO.md");
    assert!(
        staged.exists(),
        "external file should be staged into .sgf/prompts/"
    );
    let content = fs::read_to_string(&staged).expect("read staged file");
    assert!(
        content.contains("pn cheat sheet here"),
        "staged file should have original content"
    );

    let args =
        fs::read_to_string(workspace.path().join("captured-args.txt")).expect("read captured args");
    let arg_lines: Vec<&str> = args.lines().collect();
    let asp_value: Option<&str> = arg_lines
        .windows(2)
        .find(|w| w[0] == "--append-system-prompt")
        .map(|w| w[1]);

    let study = asp_value.expect("should pass --append-system-prompt with study instructions");

    assert!(
        study.contains(".sgf/prompts/.MEMENTO.md"),
        "should pass staged workspace path, not original external path, got: {study}"
    );
    assert!(
        !study.contains(external_dir.path().to_str().unwrap()),
        "should NOT pass original external path, got: {study}"
    );
}

#[test]
fn workspace_relative_files_not_staged() {
    let workspace = setup_test_dir();
    let mock = create_arg_capturing_mock(&workspace, "complete.ndjson");

    fs::write(workspace.path().join("BACKPRESSURE.md"), "# BP\n").expect("write bp");

    let bp_path = workspace.path().join("BACKPRESSURE.md");
    let output = ralph_cmd(&workspace)
        .env("PROMPT_FILES", bp_path.to_str().unwrap())
        .args([
            "--afk",
            "--command",
            mock.to_str().unwrap(),
            "1",
            "prompt.md",
        ])
        .output()
        .expect("run ralph");

    assert!(output.status.success());

    let prompts_dir = workspace.path().join(".sgf/prompts");
    let has_dotfiles = prompts_dir.exists()
        && fs::read_dir(&prompts_dir).unwrap().any(|e| {
            e.ok()
                .and_then(|e| e.file_name().to_str().map(|s| s.starts_with('.')))
                .unwrap_or(false)
        });
    assert!(
        !has_dotfiles,
        "no dotfiles should be staged for workspace-relative files"
    );
}
