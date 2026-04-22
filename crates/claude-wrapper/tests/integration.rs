use shutdown::{ChildGuard, ProcessSemaphore};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;
use tempfile::TempDir;

static CL_PERMITS: LazyLock<ProcessSemaphore> =
    LazyLock::new(|| ProcessSemaphore::from_env("SGF_TEST_MAX_CONCURRENT", 8));

fn run_cl(cmd: &mut Command) -> std::process::Output {
    let _permit = CL_PERMITS
        .acquire_timeout(Duration::from_secs(60))
        .expect("semaphore timed out");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    ChildGuard::spawn(cmd)
        .expect("spawn cl")
        .wait_with_output_timeout(Duration::from_secs(30))
        .expect("failed to run cl")
}

fn cl_binary() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("cl");
    path.to_string_lossy().into_owned()
}

fn create_mock_secret(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("claude-wrapper-secret");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CL_TEST_OUTPUT\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn prepend_to_path(dir: &std::path::Path) -> String {
    format!(
        "{}:{}",
        dir.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn invokes_claude_wrapper_secret_not_claude() {
    let workdir = TempDir::new().unwrap();
    let mock_dir = TempDir::new().unwrap();

    create_mock_secret(mock_dir.path());

    let claude_marker = mock_dir.path().join("claude_was_called");
    let claude_script = mock_dir.path().join("claude");
    fs::write(
        &claude_script,
        format!("#!/bin/sh\ntouch '{}'\n", claude_marker.to_string_lossy()),
    )
    .unwrap();
    fs::set_permissions(&claude_script, fs::Permissions::from_mode(0o755)).unwrap();

    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(workdir.path())
            .env("HOME", workdir.path().to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        output_file.exists(),
        "claude-wrapper-secret was not invoked"
    );
    assert!(
        !claude_marker.exists(),
        "claude binary was invoked directly — cl must call claude-wrapper-secret, not claude"
    );
}

#[test]
fn context_files_appear_in_append_system_prompt() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();
    fs::write(cwd.join(".sgf/BACKPRESSURE.md"), "b").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());

    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(cwd)
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = fs::read_to_string(&output_file).unwrap();
    assert!(output.contains("--append-system-prompt"));
    assert!(output.contains("study @"));
    assert!(output.contains("MEMENTO.md"));
    assert!(output.contains("BACKPRESSURE.md"));
}

#[test]
fn local_override_takes_precedence_over_global() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();
    let home_dir = TempDir::new().unwrap();
    let home = home_dir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "local").unwrap();

    fs::create_dir_all(home.join(".sgf")).unwrap();
    fs::write(home.join(".sgf/MEMENTO.md"), "global").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(cwd)
            .env("HOME", home.to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = fs::read_to_string(&output_file).unwrap();
    let prompt_line = output.lines().find(|l| l.contains("MEMENTO")).unwrap();
    assert!(
        prompt_line.contains(cwd.to_str().unwrap()),
        "expected local path in prompt, got: {prompt_line}"
    );
}

#[test]
fn missing_context_files_are_skipped_no_error_exit() {
    let workdir = TempDir::new().unwrap();
    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(workdir.path())
            .env("HOME", workdir.path().to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(result.status.success());

    let output = fs::read_to_string(&output_file).unwrap();
    assert!(!output.contains("--append-system-prompt"));
}

#[test]
fn passthrough_args_forwarded_unchanged() {
    let workdir = TempDir::new().unwrap();
    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .args(["--print", "hello world", "-v"])
            .current_dir(workdir.path())
            .env("HOME", workdir.path().to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(result.status.success());

    let output = fs::read_to_string(&output_file).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.contains(&"--print"));
    assert!(lines.contains(&"hello world"));
    assert!(lines.contains(&"-v"));
}

#[test]
fn multiple_append_system_prompt_coexist() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .args(["--append-system-prompt", "caller prompt"])
            .current_dir(cwd)
            .env("HOME", workdir.path().to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = fs::read_to_string(&output_file).unwrap();
    let prompt_count = output
        .lines()
        .filter(|l| *l == "--append-system-prompt")
        .count();
    assert_eq!(
        prompt_count, 2,
        "expected two --append-system-prompt args, got: {output}"
    );
}

#[test]
fn lookbook_appears_in_append_system_prompt_when_present() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();
    fs::write(cwd.join("LOOKBOOK.html"), "<html>lookbook</html>").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(cwd)
            .env("HOME", cwd.to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = fs::read_to_string(&output_file).unwrap();
    assert!(output.contains("LOOKBOOK.html"));
}

#[test]
fn lookbook_absent_does_not_cause_error() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(cwd)
            .env("HOME", cwd.to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(result.status.success());

    let output = fs::read_to_string(&output_file).unwrap();
    assert!(!output.contains("LOOKBOOK.html"));
    assert!(output.contains("MEMENTO.md"));
}

#[test]
fn lookbook_appears_last_in_study_string() {
    let workdir = TempDir::new().unwrap();
    let cwd = workdir.path();

    fs::create_dir_all(cwd.join(".sgf")).unwrap();
    fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();
    fs::write(cwd.join(".sgf/BACKPRESSURE.md"), "b").unwrap();
    fs::write(cwd.join("LOOKBOOK.html"), "lb").unwrap();

    let mock_dir = TempDir::new().unwrap();
    create_mock_secret(mock_dir.path());
    let output_file = mock_dir.path().join("output.txt");

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(cwd)
            .env("HOME", cwd.to_str().unwrap())
            .env("PATH", prepend_to_path(mock_dir.path()))
            .env("CL_TEST_OUTPUT", &output_file),
    );

    assert!(
        result.status.success(),
        "cl failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = fs::read_to_string(&output_file).unwrap();
    let study_line = output
        .lines()
        .find(|l| l.contains("study @"))
        .expect("should have study line");

    let memento_pos = study_line.find("MEMENTO.md").expect("MEMENTO.md in study");
    let bp_pos = study_line
        .find("BACKPRESSURE.md")
        .expect("BACKPRESSURE.md in study");
    let lb_pos = study_line
        .find("LOOKBOOK.html")
        .expect("LOOKBOOK.html in study");

    assert!(
        lb_pos > memento_pos,
        "LOOKBOOK.html should appear after MEMENTO.md"
    );
    assert!(
        lb_pos > bp_pos,
        "LOOKBOOK.html should appear after BACKPRESSURE.md"
    );
}

#[test]
fn exits_1_when_downstream_binary_missing() {
    let workdir = TempDir::new().unwrap();

    let result = run_cl(
        Command::new(cl_binary())
            .current_dir(workdir.path())
            .env("HOME", workdir.path().to_str().unwrap())
            .env("PATH", "/nonexistent"),
    );

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("claude-wrapper-secret"), "stderr: {stderr}");
}
