use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
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
        stderr.contains("Warning: Reducing iterations from 100 to max allowed (5)"),
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
