use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::Utc;
use shutdown::{ShutdownConfig, ShutdownController, ShutdownStatus};
use uuid::Uuid;

use crate::config::Mode;
use crate::loop_mgmt::{self, IterationRecord, SessionMetadata};
use crate::prompt;
use crate::recovery;
use crate::style;

pub struct LoopConfig {
    pub stage: String,
    pub spec: Option<String>,
    pub mode: Mode,
    pub no_push: bool,
    pub iterations: u32,
    /// Override agent command (defaults to `SGF_AGENT_COMMAND` env, then `ralph`).
    pub agent_command: Option<String>,
    /// Skip pre-launch recovery and daemon startup (for testing).
    pub skip_preflight: bool,
}

fn resolve_agent_command(config: &LoopConfig) -> String {
    if let Some(ref bin) = config.agent_command {
        return bin.clone();
    }
    std::env::var("SGF_AGENT_COMMAND").unwrap_or_else(|_| "ralph".to_string())
}

fn export_pensa() {
    match Command::new("pn").arg("export").output() {
        Ok(out) if out.status.success() => {
            style::print_success("pn export ok");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            style::print_error(&format!("pn export failed: {}", stderr.trim()));
        }
        Err(e) => {
            style::print_warning(&format!("pn export skipped (pn not found: {e})"));
        }
    }
}

fn build_ralph_args(
    config: &LoopConfig,
    loop_id: &str,
    prompt_path: &Path,
    log_path: Option<&Path>,
    session_id: &str,
) -> Vec<String> {
    let mut args = Vec::new();

    if config.mode == Mode::Afk {
        args.push("-a".to_string());
    }

    args.push("--loop-id".to_string());
    args.push(loop_id.to_string());

    args.push("--session-id".to_string());
    args.push(session_id.to_string());

    args.push("--auto-push".to_string());
    args.push(if config.no_push {
        "false".to_string()
    } else {
        "true".to_string()
    });

    if let Some(lp) = log_path {
        args.push("--log-file".to_string());
        args.push(lp.to_string_lossy().to_string());
    }

    args.push(config.iterations.to_string());

    args.push(prompt_path.to_string_lossy().to_string());

    args
}

fn exit_code_to_status(code: i32) -> &'static str {
    match code {
        0 => "completed",
        2 => "exhausted",
        _ => "interrupted",
    }
}

fn print_exit_message(code: i32, loop_id: &str) {
    match code {
        0 => style::print_success(&format!("loop complete [{loop_id}]")),
        1 => style::print_error(&format!("ralph exited with error [{loop_id}]")),
        2 => style::print_warning(&format!("iterations exhausted [{loop_id}]")),
        130 => style::print_warning(&format!("interrupted [{loop_id}]")),
        _ => style::print_error(&format!("ralph exited with unexpected code [{loop_id}]")),
    }
}

fn write_initial_metadata(root: &Path, loop_id: &str, config: &LoopConfig, prompt_path: &Path) {
    let now = Utc::now().to_rfc3339();
    let mode_str = match config.mode {
        Mode::Interactive => "interactive",
        Mode::Afk => "afk",
    };
    let metadata = SessionMetadata {
        loop_id: loop_id.to_string(),
        iterations: Vec::new(),
        stage: config.stage.clone(),
        spec: config.spec.clone(),
        cursus: None,
        mode: mode_str.to_string(),
        prompt: prompt_path.to_string_lossy().to_string(),
        iterations_total: config.iterations,
        status: "running".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    if let Err(e) = loop_mgmt::write_session_metadata(root, &metadata) {
        style::print_warning(&format!("failed to write session metadata: {e}"));
    }
}

fn append_iteration_to_metadata(root: &Path, loop_id: &str, iteration: u32, session_id: &str) {
    match loop_mgmt::read_session_metadata(root, loop_id) {
        Ok(Some(mut meta)) => {
            meta.iterations.push(IterationRecord {
                iteration,
                session_id: session_id.to_string(),
                completed_at: Utc::now().to_rfc3339(),
            });
            meta.updated_at = Utc::now().to_rfc3339();
            if let Err(e) = loop_mgmt::write_session_metadata(root, &meta) {
                style::print_warning(&format!("failed to append iteration to metadata: {e}"));
            }
        }
        Ok(None) => {
            style::print_warning(&format!(
                "session metadata not found for {loop_id}, skipping iteration append"
            ));
        }
        Err(e) => {
            style::print_warning(&format!(
                "failed to read session metadata for {loop_id}: {e}"
            ));
        }
    }
}

fn update_metadata_on_exit(root: &Path, loop_id: &str, exit_code: i32) {
    match loop_mgmt::read_session_metadata(root, loop_id) {
        Ok(Some(mut meta)) => {
            meta.status = exit_code_to_status(exit_code).to_string();
            meta.updated_at = Utc::now().to_rfc3339();
            if let Err(e) = loop_mgmt::write_session_metadata(root, &meta) {
                style::print_warning(&format!("failed to update session metadata: {e}"));
            }
        }
        Ok(None) => {
            style::print_warning(&format!(
                "session metadata not found for {loop_id}, skipping update"
            ));
        }
        Err(e) => {
            style::print_warning(&format!(
                "failed to read session metadata for {loop_id}: {e}"
            ));
        }
    }
}

pub fn run(root: &Path, config: &LoopConfig) -> io::Result<i32> {
    let prompt_path = prompt::validate(root, &config.stage, config.spec.as_deref())?;
    let session_id = Uuid::new_v4().to_string();

    if config.mode == Mode::Interactive {
        let loop_id = loop_mgmt::generate_loop_id(&config.stage, config.spec.as_deref());

        if !config.skip_preflight {
            recovery::ensure_daemons(root)?;
        }

        write_initial_metadata(root, &loop_id, config, &prompt_path);

        style::print_action_detail(
            &format!("launching interactive session [{loop_id}]"),
            &format!("stage: {}", config.stage),
        );

        let head_before = if !config.no_push {
            vcs_utils::git_head()
        } else {
            None
        };

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })?;

        let saved_termios = crate::iter_runner::save_terminal_settings();
        let exit_code = run_interactive_claude(&prompt_path, &session_id, &controller)?;
        if let Some(ref termios) = saved_termios {
            crate::iter_runner::restore_terminal_settings(termios);
        }

        append_iteration_to_metadata(root, &loop_id, 1, &session_id);
        update_metadata_on_exit(root, &loop_id, exit_code);

        if let Some(ref before) = head_before {
            vcs_utils::auto_push_if_changed(before, |msg| {
                if msg.contains("failed") {
                    style::print_warning(msg);
                } else {
                    style::print_action(msg);
                }
            });
        }

        return Ok(exit_code);
    }

    let loop_id = loop_mgmt::generate_loop_id(&config.stage, config.spec.as_deref());
    let is_afk = config.mode == Mode::Afk;

    if !config.skip_preflight {
        recovery::pre_launch_recovery(root)?;
        if std::env::var("SGF_SKIP_PREFLIGHT").is_err() {
            recovery::ensure_daemons(root)?;
        }
    }

    export_pensa();

    loop_mgmt::write_pid_file(root, &loop_id)?;

    write_initial_metadata(root, &loop_id, config, &prompt_path);

    let binary = resolve_agent_command(config);

    let log_path = if is_afk {
        Some(loop_mgmt::create_log_file(root, &loop_id)?)
    } else {
        None
    };

    let args = build_ralph_args(
        config,
        &loop_id,
        &prompt_path,
        log_path.as_deref(),
        &session_id,
    );

    let monitor_stdin = if is_afk {
        std::env::var("SGF_MONITOR_STDIN")
            .map_or_else(|_| std::io::stdin().is_terminal(), |v| v != "0")
    } else {
        false
    };
    let controller = ShutdownController::new(ShutdownConfig {
        monitor_stdin,
        ..Default::default()
    })?;

    if let Ok(ready_path) = std::env::var("SGF_READY_FILE") {
        let _ = std::fs::write(&ready_path, "");
    }

    {
        let mut parts = Vec::new();
        if let Some(ref spec) = config.spec {
            parts.push(format!("stage: {spec}"));
        }
        parts.push(format!("iterations: {}", config.iterations));
        if is_afk {
            parts.push("mode: afk".to_string());
        }
        style::print_action_detail(&format!("launching ralph [{loop_id}]"), &parts.join(" · "));
    }

    let saved_termios = crate::iter_runner::save_terminal_settings();
    let exit_code = run_ralph(&binary, &args, is_afk, &controller)?;
    if let Some(ref termios) = saved_termios {
        crate::iter_runner::restore_terminal_settings(termios);
    }

    append_iteration_to_metadata(root, &loop_id, 1, &session_id);
    update_metadata_on_exit(root, &loop_id, exit_code);

    loop_mgmt::remove_pid_file(root, &loop_id);

    print_exit_message(exit_code, &loop_id);

    Ok(exit_code)
}

fn run_interactive_claude(
    prompt_path: &Path,
    session_id: &str,
    controller: &ShutdownController,
) -> io::Result<i32> {
    let prompt_arg = format!("@{}", prompt_path.display());

    let mut child = Command::new("cl")
        .args(["--verbose", "--session-id", session_id, &prompt_arg])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn cl: {e}")))?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {
                if controller.poll() == ShutdownStatus::Shutdown {
                    kill_child(&child);
                    let _ = child.wait();
                    return Ok(130);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn run_ralph(
    binary: &str,
    args: &[String],
    afk: bool,
    controller: &ShutdownController,
) -> io::Result<i32> {
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .stdin(if afk { Stdio::null() } else { Stdio::inherit() })
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("SGF_MANAGED", "1");
    if afk && std::env::var("SGF_TEST_NO_SETSID").is_err() {
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn ralph: {e}")))?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {
                if controller.poll() == ShutdownStatus::Shutdown {
                    kill_child(&child);
                    let _ = child.wait();
                    return Ok(130);
                }

                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn kill_child(child: &std::process::Child) {
    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
}

fn humanize_relative_time(updated_at: &str) -> String {
    let Ok(updated) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return "unknown".to_string();
    };
    let now = Utc::now();
    let delta = now.signed_duration_since(updated);
    let secs = delta.num_seconds();
    if secs < 0 {
        return "just now".to_string();
    }
    if secs < 60 {
        return format!("{secs}s ago");
    }
    let mins = delta.num_minutes();
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = delta.num_days();
    format!("{days}d ago")
}

fn run_resume_session(root: &Path, meta: &SessionMetadata, session_id: &str) -> io::Result<i32> {
    let loop_id = &meta.loop_id;

    style::print_action_detail(
        &format!("resuming session [{loop_id}]"),
        &format!("session: {session_id}"),
    );

    let controller = ShutdownController::new(ShutdownConfig {
        monitor_stdin: false,
        ..Default::default()
    })?;

    let mut child = Command::new("cl")
        .args([
            "--resume",
            session_id,
            "--verbose",
            "--dangerously-skip-permissions",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn cl: {e}")))?;

    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(1),
            Ok(None) => {
                if controller.poll() == ShutdownStatus::Shutdown {
                    kill_child(&child);
                    let _ = child.wait();
                    break 130;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    };

    update_metadata_on_exit(root, loop_id, exit_code);

    Ok(exit_code)
}

fn prompt_and_select(entries_len: usize) -> io::Result<Option<usize>> {
    eprint!("Select session (1-{entries_len}): ");

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
        return Ok(None);
    }

    let choice: usize = input
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid selection"))?;

    if choice < 1 || choice > entries_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Selection out of range: {choice}"),
        ));
    }

    Ok(Some(choice))
}

struct DisplayEntry<'a> {
    meta: &'a SessionMetadata,
    iteration: &'a IterationRecord,
}

fn print_entries(entries: &[DisplayEntry]) {
    eprintln!("Recent sessions:");
    for (i, e) in entries.iter().enumerate() {
        let relative = humanize_relative_time(&e.iteration.completed_at);
        eprintln!(
            "  {:>2}. {:<40} iter {:<4} {:<12} {:<12} {}",
            i + 1,
            e.meta.loop_id,
            e.iteration.iteration,
            e.meta.mode,
            e.meta.status,
            relative
        );
    }
}

pub fn run_resume(root: &Path, loop_id: Option<&str>) -> io::Result<i32> {
    if let Some(id) = loop_id {
        let meta = loop_mgmt::read_session_metadata(root, id)?;
        let m = match meta {
            Some(m) => m,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Session not found: {id}"),
                ));
            }
        };

        if m.iterations.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("No iterations found for session: {id}"),
            ));
        }

        if m.iterations.len() == 1 {
            return run_resume_session(root, &m, &m.iterations[0].session_id);
        }

        let entries: Vec<DisplayEntry> = m
            .iterations
            .iter()
            .map(|it| DisplayEntry {
                meta: &m,
                iteration: it,
            })
            .collect();

        print_entries(&entries);

        match prompt_and_select(entries.len())? {
            Some(choice) => {
                let selected = &entries[choice - 1];
                return run_resume_session(root, &m, &selected.iteration.session_id);
            }
            None => return Ok(0),
        }
    }

    let sessions = loop_mgmt::list_session_metadata(root)?;
    if sessions.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No sessions found.",
        ));
    }

    let mut entries: Vec<DisplayEntry> = Vec::new();
    for s in &sessions {
        for it in &s.iterations {
            entries.push(DisplayEntry {
                meta: s,
                iteration: it,
            });
        }
    }
    entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));
    entries.truncate(20);

    if entries.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No sessions found.",
        ));
    }

    print_entries(&entries);

    match prompt_and_select(entries.len())? {
        Some(choice) => {
            let selected = &entries[choice - 1];
            run_resume_session(root, selected.meta, &selected.iteration.session_id)
        }
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_project(root: &Path, stage: &str, template_content: &str) {
        fs::create_dir_all(root.join(".sgf/prompts")).unwrap();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();
        fs::create_dir_all(root.join(".sgf/logs")).unwrap();
        fs::write(
            root.join(format!(".sgf/prompts/{stage}.md")),
            template_content,
        )
        .unwrap();
    }

    fn setup_spec(root: &Path, stem: &str) {
        fs::create_dir_all(root.join("specs")).unwrap();
        fs::write(
            root.join(format!("specs/{stem}.md")),
            format!("# {stem} spec"),
        )
        .unwrap();
    }

    fn setup_git_repo(root: &Path) {
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
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial", "--allow-empty"])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
    }

    fn mock_agent_script(root: &Path, script: &str) -> String {
        let mock_path = root.join("mock_agent.sh");
        fs::write(&mock_path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        mock_path.to_string_lossy().to_string()
    }

    const TEST_SESSION_ID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    #[test]
    fn build_args_build_afk_no_push() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: true,
            iterations: 10,
            agent_command: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
            None,
            TEST_SESSION_ID,
        );

        assert_eq!(
            args,
            vec![
                "-a",
                "--loop-id",
                "build-auth-20260226T143000",
                "--session-id",
                TEST_SESSION_ID,
                "--auto-push",
                "false",
                "10",
                "/tmp/prompt.md",
            ]
        );
    }

    #[test]
    fn build_args_verify_interactive() {
        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Interactive,
            no_push: false,
            iterations: 30,
            agent_command: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "verify-20260226T150000",
            Path::new("/tmp/verify.md"),
            None,
            TEST_SESSION_ID,
        );

        assert!(!args.contains(&"-a".to_string()));
        assert!(args.contains(&"--auto-push".to_string()));
        let auto_push_idx = args.iter().position(|a| a == "--auto-push").unwrap();
        assert_eq!(args[auto_push_idx + 1], "true");
        assert!(args.contains(&"--session-id".to_string()));
    }

    #[test]
    fn build_args_default_iterations() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Interactive,
            no_push: false,
            iterations: 30,
            agent_command: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
            None,
            TEST_SESSION_ID,
        );

        assert!(args.contains(&"30".to_string()));
        assert!(!args.contains(&"--max-iterations".to_string()));
    }

    #[test]
    fn exit_messages_all_codes() {
        print_exit_message(0, "test-loop");
        print_exit_message(1, "test-loop");
        print_exit_message(2, "test-loop");
        print_exit_message(130, "test-loop");
        print_exit_message(42, "test-loop");
    }

    #[test]
    fn resolve_binary_from_config() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: None,
            mode: Mode::Interactive,
            no_push: false,
            iterations: 30,
            agent_command: Some("/custom/ralph".to_string()),
            skip_preflight: false,
        };
        assert_eq!(resolve_agent_command(&config), "/custom/ralph");
    }

    #[test]
    fn run_with_mock_agent_exit_0() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"ralph invoked: $@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--loop-id"));
        assert!(args_content.contains("--auto-push true"));
        assert!(!args_content.contains("--max-iterations"));
        assert!(args_content.contains("-a"));

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_with_mock_agent_exit_1() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 1\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 1);

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_with_mock_agent_exit_2() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "verify", "Verify everything.");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 2\n");

        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 2);

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_afk_passes_log_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
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
    fn run_no_push_passes_auto_push_false() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: true,
            iterations: 10,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--auto-push false"));
    }

    #[test]
    fn run_passes_raw_prompt_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        run(root, &config).unwrap();

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            args_content.contains(".sgf/prompts/build.md"),
            "should pass raw template path, got: {args_content}"
        );
        assert!(
            !args_content.contains(".assembled"),
            "should NOT pass assembled path, got: {args_content}"
        );
    }

    #[test]
    fn verify_passes_raw_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "verify", "Verify all specs against codebase.");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--loop-id"));
        assert!(args_content.contains("verify-"));
        assert!(args_content.contains("-a"));
        assert!(args_content.contains(".sgf/prompts/verify.md"));
    }

    #[test]
    fn test_stage_does_not_pass_spec_flag() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "test", "Test items.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "test".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            !args_content.contains("--spec"),
            "should NOT pass --spec to ralph, got: {args_content}"
        );
    }

    #[test]
    fn test_plan_no_variables() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "test-plan", "Generate a testing plan.");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "test-plan".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("test-plan-"));
    }

    #[test]
    fn run_afk_passes_afk_flag() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        let args: Vec<&str> = args_content.split_whitespace().collect();
        assert!(
            args.contains(&"-a"),
            "AFK mode should pass -a flag, got: {args_content}"
        );
    }

    #[test]
    fn exit_code_to_status_mappings() {
        assert_eq!(exit_code_to_status(0), "completed");
        assert_eq!(exit_code_to_status(2), "exhausted");
        assert_eq!(exit_code_to_status(130), "interrupted");
        assert_eq!(exit_code_to_status(1), "interrupted");
        assert_eq!(exit_code_to_status(42), "interrupted");
    }

    #[test]
    fn run_afk_writes_session_metadata_completed() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 0\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let sessions = loop_mgmt::list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert!(meta.loop_id.starts_with("build-auth-"));
        assert_eq!(meta.iterations.len(), 1);
        assert_eq!(meta.iterations[0].iteration, 1);
        assert!(!meta.iterations[0].session_id.is_empty());
        assert_eq!(meta.stage, "build");
        assert_eq!(meta.spec.as_deref(), Some("auth"));
        assert_eq!(meta.mode, "afk");
        assert_eq!(meta.status, "completed");
        assert_eq!(meta.iterations_total, 30);
        assert!(meta.prompt.contains(".sgf/prompts/build.md"));
    }

    #[test]
    fn run_afk_writes_session_metadata_exhausted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 2\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 10,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 2);

        let sessions = loop_mgmt::list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, "exhausted");
    }

    #[test]
    fn run_afk_writes_session_metadata_interrupted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 1\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 10,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 1);

        let sessions = loop_mgmt::list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, "interrupted");
    }

    #[test]
    fn run_afk_passes_session_id_to_ralph() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_agent_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        run(root, &config).unwrap();

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            args_content.contains("--session-id"),
            "should pass --session-id to ralph, got: {args_content}"
        );

        let sessions = loop_mgmt::list_session_metadata(root).unwrap();
        let meta = &sessions[0];
        assert_eq!(meta.iterations.len(), 1);
        assert!(
            args_content.contains(&meta.iterations[0].session_id),
            "ralph should receive the same session_id as metadata, got: {args_content}"
        );
    }

    #[test]
    fn run_afk_no_spec_writes_metadata() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "verify", "Verify everything.");
        setup_git_repo(root);

        let mock = mock_agent_script(root, "#!/bin/sh\nexit 0\n");

        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            agent_command: Some(mock),
            skip_preflight: true,
        };

        run(root, &config).unwrap();

        let sessions = loop_mgmt::list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].loop_id.starts_with("verify-"));
        assert!(sessions[0].spec.is_none());
        assert_eq!(sessions[0].mode, "afk");
    }

    #[test]
    fn humanize_seconds() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("s ago"), "got: {result}");
    }

    #[test]
    fn humanize_minutes() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::minutes(5)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("m ago"), "got: {result}");
    }

    #[test]
    fn humanize_hours() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::hours(3)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("h ago"), "got: {result}");
    }

    #[test]
    fn humanize_days() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::days(2)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("d ago"), "got: {result}");
    }

    #[test]
    fn humanize_invalid_timestamp() {
        assert_eq!(humanize_relative_time("not-a-date"), "unknown");
    }

    #[test]
    fn humanize_future_timestamp() {
        let now = Utc::now();
        let ts = (now + chrono::Duration::hours(1)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert_eq!(result, "just now");
    }

    #[test]
    fn resume_no_sessions_returns_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let err = run_resume(root, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("No sessions found"));
    }

    #[test]
    fn resume_unknown_loop_id_returns_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let err = run_resume(root, Some("nonexistent-id")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(
            err.to_string()
                .contains("Session not found: nonexistent-id"),
            "got: {}",
            err
        );
    }

    #[test]
    fn resume_with_valid_loop_id_launches_cl_with_resume_flag() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let meta = SessionMetadata {
            loop_id: "build-auth-20260316T120000".to_string(),
            iterations: vec![IterationRecord {
                iteration: 1,
                session_id: session_id.to_string(),
                completed_at: "2026-03-16T12:02:30Z".to_string(),
            }],
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            cursus: None,
            mode: "interactive".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "interrupted".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:05:30Z".to_string(),
        };
        loop_mgmt::write_session_metadata(root, &meta).unwrap();

        let mock_cl = root.join("mock_cl.sh");
        fs::write(
            &mock_cl,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/cl_args.txt\"\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_cl, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", root.display(), original_path);
        unsafe { std::env::set_var("PATH", &new_path) };

        // Rename mock to `cl`
        let cl_path = root.join("cl");
        fs::rename(&mock_cl, &cl_path).unwrap();

        let result = run_resume(root, Some("build-auth-20260316T120000"));

        unsafe { std::env::set_var("PATH", &original_path) };

        let exit_code = result.unwrap();
        assert_eq!(exit_code, 0);

        let cl_args = fs::read_to_string(root.join("cl_args.txt")).unwrap();
        assert!(
            cl_args.contains("--resume"),
            "cl should receive --resume, got: {cl_args}"
        );
        assert!(
            cl_args.contains(session_id),
            "cl should receive session_id, got: {cl_args}"
        );
        assert!(
            cl_args.contains("--verbose"),
            "cl should receive --verbose, got: {cl_args}"
        );

        let updated = loop_mgmt::read_session_metadata(root, "build-auth-20260316T120000")
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "completed");
    }

    #[test]
    fn resume_loop_id_with_no_iterations_returns_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let meta = SessionMetadata {
            loop_id: "build-auth-20260316T120000".to_string(),
            iterations: Vec::new(),
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            cursus: None,
            mode: "interactive".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "running".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:00:00Z".to_string(),
        };
        loop_mgmt::write_session_metadata(root, &meta).unwrap();

        let err = run_resume(root, Some("build-auth-20260316T120000")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("No iterations found"),
            "got: {}",
            err
        );
    }

    fn make_session(
        loop_id: &str,
        mode: &str,
        status: &str,
        iterations: Vec<(u32, &str, &str)>,
    ) -> SessionMetadata {
        SessionMetadata {
            loop_id: loop_id.to_string(),
            iterations: iterations
                .into_iter()
                .map(|(iter, sid, completed)| IterationRecord {
                    iteration: iter,
                    session_id: sid.to_string(),
                    completed_at: completed.to_string(),
                })
                .collect(),
            stage: "build".to_string(),
            spec: None,
            cursus: None,
            mode: mode.to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 30,
            status: status.to_string(),
            created_at: "2026-03-16T10:00:00Z".to_string(),
            updated_at: "2026-03-16T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn picker_entries_sort_newest_first() {
        let s1 = make_session(
            "loop-a",
            "afk",
            "completed",
            vec![
                (1, "sid-a1", "2026-03-16T10:00:00Z"),
                (2, "sid-a2", "2026-03-16T12:00:00Z"),
            ],
        );
        let s2 = make_session(
            "loop-b",
            "interactive",
            "interrupted",
            vec![(1, "sid-b1", "2026-03-16T11:00:00Z")],
        );
        let sessions = vec![s1, s2];

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].iteration.completed_at, "2026-03-16T12:00:00Z");
        assert_eq!(entries[0].meta.loop_id, "loop-a");
        assert_eq!(entries[0].iteration.iteration, 2);

        assert_eq!(entries[1].iteration.completed_at, "2026-03-16T11:00:00Z");
        assert_eq!(entries[1].meta.loop_id, "loop-b");
        assert_eq!(entries[1].iteration.iteration, 1);

        assert_eq!(entries[2].iteration.completed_at, "2026-03-16T10:00:00Z");
        assert_eq!(entries[2].meta.loop_id, "loop-a");
        assert_eq!(entries[2].iteration.iteration, 1);
    }

    #[test]
    fn picker_entries_truncate_drops_oldest() {
        let mut sessions = Vec::new();
        for i in 0..25u32 {
            sessions.push(make_session(
                &format!("loop-{i:02}"),
                "afk",
                "completed",
                vec![(
                    1,
                    &format!("sid-{i:02}"),
                    &format!("2026-03-{:02}T10:00:00Z", (i % 28) + 1),
                )],
            ));
        }

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));
        entries.truncate(20);

        assert_eq!(entries.len(), 20);
        // Newest should be first (day 25)
        assert_eq!(entries[0].meta.loop_id, "loop-24");
        assert_eq!(entries[0].iteration.completed_at, "2026-03-25T10:00:00Z");
        // Oldest kept should be day 6 (loop-05)
        assert_eq!(entries[19].meta.loop_id, "loop-05");
        assert_eq!(entries[19].iteration.completed_at, "2026-03-06T10:00:00Z");
        // The 5 oldest (loop-00..loop-04, days 1-5) should be dropped
        let loop_ids: Vec<&str> = entries.iter().map(|e| e.meta.loop_id.as_str()).collect();
        for i in 0..5 {
            assert!(
                !loop_ids.contains(&format!("loop-{i:02}").as_str()),
                "loop-{i:02} (oldest) should be truncated"
            );
        }
    }

    #[test]
    fn print_entries_display_format() {
        let session = make_session(
            "build-auth-20260316T120000",
            "afk",
            "completed",
            vec![(3, "sid-abc", "2026-01-01T00:00:00Z")],
        );
        let entries: Vec<DisplayEntry> = session
            .iterations
            .iter()
            .map(|it| DisplayEntry {
                meta: &session,
                iteration: it,
            })
            .collect();

        // print_entries writes to stderr; verify it doesn't panic and exercises the format
        print_entries(&entries);

        // Verify the format string components are what we expect
        let e = &entries[0];
        let relative = humanize_relative_time(&e.iteration.completed_at);
        let line = format!(
            "  {:>2}. {:<40} iter {:<4} {:<12} {:<12} {}",
            1, e.meta.loop_id, e.iteration.iteration, e.meta.mode, e.meta.status, relative
        );
        assert!(line.contains("build-auth-20260316T120000"));
        assert!(line.contains("iter 3"));
        assert!(line.contains("afk"));
        assert!(line.contains("completed"));
        assert!(line.contains("ago") || line.contains("unknown"));
    }

    #[test]
    fn print_entries_multi_session_display() {
        let s1 = make_session(
            "build-auth-20260316T120000",
            "afk",
            "completed",
            vec![(1, "sid-1", "2026-03-16T12:00:00Z")],
        );
        let s2 = make_session(
            "verify-20260316T130000",
            "interactive",
            "interrupted",
            vec![(2, "sid-2", "2026-03-16T13:00:00Z")],
        );
        let sessions = vec![s1, s2];

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));

        // Verify different sessions appear with correct numbering
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].meta.loop_id, "verify-20260316T130000");
        assert_eq!(entries[0].meta.mode, "interactive");
        assert_eq!(entries[0].meta.status, "interrupted");
        assert_eq!(entries[1].meta.loop_id, "build-auth-20260316T120000");
        assert_eq!(entries[1].meta.mode, "afk");
        assert_eq!(entries[1].meta.status, "completed");

        // Verify print doesn't panic with multiple entries
        print_entries(&entries);
    }

    #[test]
    fn setsid_skipped_when_env_var_set() {
        unsafe { std::env::set_var("SGF_TEST_NO_SETSID", "1") };
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let child_pid = child.id() as libc::pid_t;
        let parent_pgid = unsafe { libc::getpgid(0) };
        let child_pgid = unsafe { libc::getpgid(child_pid) };
        assert_eq!(
            child_pgid, parent_pgid,
            "with SGF_TEST_NO_SETSID set, child should share parent's process group"
        );
        let _ = child.kill();
        let _ = child.wait();
        unsafe { std::env::remove_var("SGF_TEST_NO_SETSID") };
    }

    #[test]
    fn setsid_applied_without_env_var() {
        unsafe { std::env::remove_var("SGF_TEST_NO_SETSID") };
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        let child_pid = child.id() as libc::pid_t;
        let parent_pgid = unsafe { libc::getpgid(0) };
        let child_pgid = unsafe { libc::getpgid(child_pid) };
        assert_ne!(
            child_pgid, parent_pgid,
            "without SGF_TEST_NO_SETSID, child with setsid should be in its own session"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
