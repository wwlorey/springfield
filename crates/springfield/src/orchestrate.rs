use std::io::{self, IsTerminal as _};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use shutdown::{ShutdownConfig, ShutdownController, ShutdownStatus};

use crate::config::Mode;
use crate::loop_mgmt;
use crate::prompt;
use crate::recovery;
use crate::style;

pub struct LoopConfig {
    pub stage: String,
    pub spec: Option<String>,
    pub mode: Mode,
    pub no_push: bool,
    pub iterations: u32,
    /// Override ralph binary path (defaults to `SGF_RALPH_BINARY` env, then `ralph`).
    pub ralph_binary: Option<String>,
    /// Skip pre-launch recovery and daemon startup (for testing).
    pub skip_preflight: bool,
}

fn resolve_ralph_binary(config: &LoopConfig) -> String {
    if let Some(ref bin) = config.ralph_binary {
        return bin.clone();
    }
    std::env::var("SGF_RALPH_BINARY").unwrap_or_else(|_| "ralph".to_string())
}

fn resolve_agent_cmd() -> io::Result<String> {
    resolve_agent_cmd_from(std::env::var("AGENT_CMD").ok())
}

fn resolve_agent_cmd_from(val: Option<String>) -> io::Result<String> {
    match val {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(io::Error::other(
            "AGENT_CMD not set. Set AGENT_CMD to the path of the agent binary (e.g., AGENT_CMD=claude).",
        )),
    }
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
) -> Vec<String> {
    let mut args = Vec::new();

    if config.mode == Mode::Afk {
        args.push("-a".to_string());
    }

    args.push("--loop-id".to_string());
    args.push(loop_id.to_string());

    args.push("--auto-push".to_string());
    args.push(if config.no_push {
        "false".to_string()
    } else {
        "true".to_string()
    });

    if let Some(ref spec) = config.spec {
        args.push("--spec".to_string());
        args.push(spec.clone());
    }

    if let Some(lp) = log_path {
        args.push("--log-file".to_string());
        args.push(lp.to_string_lossy().to_string());
    }

    args.push(config.iterations.to_string());

    args.push(prompt_path.to_string_lossy().to_string());

    args
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

pub fn run(root: &Path, config: &LoopConfig) -> io::Result<i32> {
    let prompt_path = prompt::validate(root, &config.stage, config.spec.as_deref())?;

    if config.mode == Mode::Interactive {
        if !config.skip_preflight {
            recovery::ensure_daemon(root)?;
        }

        style::print_action_detail(
            &format!("launching interactive session [{}]", config.stage),
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

        let exit_code = run_interactive_claude(&prompt_path, &controller)?;

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

    resolve_agent_cmd()?;

    let loop_id = loop_mgmt::generate_loop_id(&config.stage, config.spec.as_deref());
    let is_afk = config.mode == Mode::Afk;

    if !config.skip_preflight {
        recovery::pre_launch_recovery(root)?;
        if std::env::var("SGF_SKIP_PREFLIGHT").is_err() {
            recovery::ensure_daemon(root)?;
        }
    }

    export_pensa();

    loop_mgmt::write_pid_file(root, &loop_id)?;

    let binary = resolve_ralph_binary(config);

    let log_path = if is_afk {
        Some(loop_mgmt::create_log_file(root, &loop_id)?)
    } else {
        None
    };

    let args = build_ralph_args(config, &loop_id, &prompt_path, log_path.as_deref());

    let monitor_stdin =
        is_afk && (std::env::var("SGF_MONITOR_STDIN").is_ok() || io::stdin().is_terminal());
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

    let exit_code = run_ralph(&binary, &args, is_afk, config.spec.as_deref(), &controller)?;

    loop_mgmt::remove_pid_file(root, &loop_id);

    print_exit_message(exit_code, &loop_id);

    Ok(exit_code)
}

fn run_interactive_claude(prompt_path: &Path, controller: &ShutdownController) -> io::Result<i32> {
    let cmd = resolve_agent_cmd()?;
    let prompt_arg = format!("@{}", prompt_path.display());

    let mut child = Command::new(&cmd)
        .args(["--verbose", &prompt_arg])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn {cmd}: {e}")))?;

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
    spec: Option<&str>,
    controller: &ShutdownController,
) -> io::Result<i32> {
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .stdin(if afk { Stdio::null() } else { Stdio::inherit() })
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("SGF_MANAGED", "1");
    if let Some(stem) = spec {
        cmd.env("SGF_SPEC", stem);
    }
    if afk {
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

    fn mock_ralph_script(root: &Path, script: &str) -> String {
        let mock_path = root.join("mock_ralph.sh");
        fs::write(&mock_path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        mock_path.to_string_lossy().to_string()
    }

    #[test]
    fn resolve_agent_cmd_missing() {
        let result = resolve_agent_cmd_from(None);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("AGENT_CMD not set"),
            "expected AGENT_CMD error, got: {err_msg}"
        );
    }

    #[test]
    fn resolve_agent_cmd_empty() {
        let result = resolve_agent_cmd_from(Some(String::new()));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_agent_cmd_set() {
        let result = resolve_agent_cmd_from(Some("my-agent".to_string()));
        assert_eq!(result.unwrap(), "my-agent");
    }

    #[test]
    fn build_args_build_afk_no_push() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: true,
            iterations: 10,
            ralph_binary: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
            None,
        );

        assert_eq!(
            args,
            vec![
                "-a",
                "--loop-id",
                "build-auth-20260226T143000",
                "--auto-push",
                "false",
                "--spec",
                "auth",
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
            ralph_binary: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "verify-20260226T150000",
            Path::new("/tmp/verify.md"),
            None,
        );

        assert!(!args.contains(&"-a".to_string()));
        assert!(args.contains(&"--auto-push".to_string()));
        let auto_push_idx = args.iter().position(|a| a == "--auto-push").unwrap();
        assert_eq!(args[auto_push_idx + 1], "true");
    }

    #[test]
    fn build_args_default_iterations() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Interactive,
            no_push: false,
            iterations: 30,
            ralph_binary: None,
            skip_preflight: false,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
            None,
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
            ralph_binary: Some("/custom/ralph".to_string()),
            skip_preflight: false,
        };
        assert_eq!(resolve_ralph_binary(&config), "/custom/ralph");
    }

    #[test]
    fn run_with_mock_ralph_exit_0() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"ralph invoked: $@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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
    fn run_with_mock_ralph_exit_1() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build the spec now.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_ralph_script(root, "#!/bin/sh\nexit 1\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 1);

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_with_mock_ralph_exit_2() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "verify", "Verify everything.");
        setup_git_repo(root);

        let mock = mock_ralph_script(root, "#!/bin/sh\nexit 2\n");

        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: true,
            iterations: 10,
            ralph_binary: Some(mock),
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

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "verify".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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
    fn test_stage_passes_spec_flag() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "test", "Test $SGF_SPEC items.");
        setup_spec(root, "auth");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "test".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            args_content.contains("--spec auth"),
            "should pass --spec to ralph, got: {args_content}"
        );
    }

    #[test]
    fn test_plan_no_variables() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "test-plan", "Generate a testing plan.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "test-plan".to_string(),
            spec: None,
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
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

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: Mode::Afk,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        let args: Vec<&str> = args_content.trim().split_whitespace().collect();
        assert!(
            args.contains(&"-a"),
            "AFK mode should pass -a flag, got: {args_content}"
        );
    }
}
