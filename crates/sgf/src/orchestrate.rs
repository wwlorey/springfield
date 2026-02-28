use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;

use crate::loop_mgmt;
use crate::prompt;
use crate::recovery;

pub struct LoopConfig {
    pub stage: String,
    pub spec: Option<String>,
    pub afk: bool,
    pub no_push: bool,
    pub iterations: u32,
    /// Override ralph binary path (defaults to `SGF_RALPH_BINARY` env, then `ralph`).
    pub ralph_binary: Option<String>,
    /// Skip pre-launch recovery and daemon startup (for testing).
    pub skip_preflight: bool,
    /// Override prompt template name (defaults to `stage`).
    pub prompt_template: Option<String>,
}

fn resolve_ralph_binary(config: &LoopConfig) -> String {
    if let Some(ref bin) = config.ralph_binary {
        return bin.clone();
    }
    std::env::var("SGF_RALPH_BINARY").unwrap_or_else(|_| "ralph".to_string())
}

fn build_ralph_args(config: &LoopConfig, loop_id: &str, prompt_path: &Path) -> Vec<String> {
    let mut args = Vec::new();

    if config.afk {
        args.push("-a".to_string());
    }

    args.push("--loop-id".to_string());
    args.push(loop_id.to_string());

    args.push("--template".to_string());
    args.push("ralph-sandbox:latest".to_string());

    args.push("--auto-push".to_string());
    args.push(if config.no_push {
        "false".to_string()
    } else {
        "true".to_string()
    });

    args.push("--max-iterations".to_string());
    args.push("30".to_string());

    args.push(config.iterations.to_string());

    args.push(prompt_path.to_string_lossy().to_string());

    args
}

fn exit_message(code: i32) -> &'static str {
    match code {
        0 => "loop completed successfully",
        1 => "ralph exited with error",
        2 => "iterations exhausted — re-launch to continue or stop",
        130 => "interrupted",
        _ => "ralph exited with unexpected code",
    }
}

pub fn run(root: &Path, config: &LoopConfig) -> io::Result<i32> {
    let loop_id = loop_mgmt::generate_loop_id(&config.stage, config.spec.as_deref());

    let mut vars = HashMap::new();
    if let Some(ref spec) = config.spec {
        vars.insert("spec".to_string(), spec.clone());
    }
    let template_stage = config.prompt_template.as_deref().unwrap_or(&config.stage);
    let prompt_path = prompt::assemble(root, template_stage, &vars)?;

    if !config.skip_preflight {
        recovery::pre_launch_recovery(root)?;
        recovery::ensure_daemon(root)?;
    }

    loop_mgmt::write_pid_file(root, &loop_id)?;

    let binary = resolve_ralph_binary(config);
    let args = build_ralph_args(config, &loop_id, &prompt_path);

    let interrupted = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, Arc::clone(&interrupted))
        .map_err(|e| io::Error::other(format!("failed to register SIGINT handler: {e}")))?;
    flag::register(SIGTERM, Arc::clone(&interrupted))
        .map_err(|e| io::Error::other(format!("failed to register SIGTERM handler: {e}")))?;

    eprintln!("sgf: launching ralph [{loop_id}]");

    let exit_code = if config.afk {
        run_afk(root, &binary, &args, &loop_id, &interrupted)?
    } else {
        run_interactive(&binary, &args, &interrupted)?
    };

    loop_mgmt::remove_pid_file(root, &loop_id);

    let msg = exit_message(exit_code);
    eprintln!("sgf: {msg} [{loop_id}]");

    Ok(exit_code)
}

fn run_afk(
    root: &Path,
    binary: &str,
    args: &[String],
    loop_id: &str,
    interrupted: &AtomicBool,
) -> io::Result<i32> {
    let log_path = loop_mgmt::create_log_file(root, loop_id)?;

    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn ralph: {e}")))?;

    let stdout = child.stdout.take().expect("stdout was piped");

    let log_path_clone = log_path.clone();
    let tee_handle = std::thread::spawn(move || loop_mgmt::tee_output(stdout, &log_path_clone));

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = tee_handle.join();
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {
                if interrupted.load(Ordering::Relaxed) {
                    kill_child(&child);
                    let _ = child.wait();
                    let _ = tee_handle.join();
                    return Ok(130);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = tee_handle.join();
                return Err(e);
            }
        }
    }
}

fn run_interactive(binary: &str, args: &[String], interrupted: &AtomicBool) -> io::Result<i32> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| io::Error::other(format!("failed to spawn ralph: {e}")))?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {
                if interrupted.load(Ordering::Relaxed) {
                    kill_child(&child);
                    let _ = child.wait();
                    return Ok(130);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn kill_child(child: &std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_project(root: &Path, stage: &str, template_content: &str) {
        fs::create_dir_all(root.join(".sgf/prompts/.assembled")).unwrap();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();
        fs::create_dir_all(root.join(".sgf/logs")).unwrap();
        fs::write(
            root.join(format!(".sgf/prompts/{stage}.md")),
            template_content,
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
    fn build_args_build_afk_no_push() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: true,
            iterations: 10,
            ralph_binary: None,
            skip_preflight: false,
            prompt_template: None,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
        );

        assert_eq!(
            args,
            vec![
                "-a",
                "--loop-id",
                "build-auth-20260226T143000",
                "--template",
                "ralph-sandbox:latest",
                "--auto-push",
                "false",
                "--max-iterations",
                "30",
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
            afk: false,
            no_push: false,
            iterations: 30,
            ralph_binary: None,
            skip_preflight: false,
            prompt_template: None,
        };
        let args = build_ralph_args(
            &config,
            "verify-20260226T150000",
            Path::new("/tmp/verify.md"),
        );

        assert!(!args.contains(&"-a".to_string()));
        assert!(args.contains(&"--auto-push".to_string()));
        let auto_push_idx = args.iter().position(|a| a == "--auto-push").unwrap();
        assert_eq!(args[auto_push_idx + 1], "true");
    }

    #[test]
    fn build_args_spec_one_iteration() {
        let config = LoopConfig {
            stage: "spec".to_string(),
            spec: None,
            afk: false,
            no_push: false,
            iterations: 1,
            ralph_binary: None,
            skip_preflight: false,
            prompt_template: None,
        };
        let args = build_ralph_args(&config, "spec-20260226T160000", Path::new("/tmp/spec.md"));

        assert!(!args.contains(&"-a".to_string()));
        assert!(args.contains(&"1".to_string()));
    }

    #[test]
    fn build_args_default_iterations() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: false,
            no_push: false,
            iterations: 30,
            ralph_binary: None,
            skip_preflight: false,
            prompt_template: None,
        };
        let args = build_ralph_args(
            &config,
            "build-auth-20260226T143000",
            Path::new("/tmp/prompt.md"),
        );

        assert!(args.contains(&"30".to_string()));
        assert!(args.contains(&"--max-iterations".to_string()));
        let max_idx = args.iter().position(|a| a == "--max-iterations").unwrap();
        assert_eq!(args[max_idx + 1], "30");
    }

    #[test]
    fn exit_messages() {
        assert_eq!(exit_message(0), "loop completed successfully");
        assert_eq!(exit_message(1), "ralph exited with error");
        assert_eq!(
            exit_message(2),
            "iterations exhausted — re-launch to continue or stop"
        );
        assert_eq!(exit_message(130), "interrupted");
        assert_eq!(exit_message(42), "ralph exited with unexpected code");
    }

    #[test]
    fn resolve_binary_from_config() {
        let config = LoopConfig {
            stage: "build".to_string(),
            spec: None,
            afk: false,
            no_push: false,
            iterations: 30,
            ralph_binary: Some("/custom/ralph".to_string()),
            skip_preflight: false,
            prompt_template: None,
        };
        assert_eq!(resolve_ralph_binary(&config), "/custom/ralph");
    }

    #[test]
    fn run_with_mock_ralph_exit_0() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build {{spec}} now.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"ralph invoked: $@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--loop-id"));
        assert!(args_content.contains("--template ralph-sandbox:latest"));
        assert!(args_content.contains("--auto-push true"));
        assert!(args_content.contains("--max-iterations 30"));
        assert!(args_content.contains("-a"));

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_with_mock_ralph_exit_1() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build {{spec}} now.");
        setup_git_repo(root);

        let mock = mock_ralph_script(root, "#!/bin/sh\nexit 1\n");

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
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
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 2);

        let pid_files = loop_mgmt::list_pid_files(root);
        assert!(pid_files.is_empty());
    }

    #[test]
    fn run_afk_tees_log() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build {{spec}} now.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho 'iteration 1 of 30'\necho 'task complete'\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let logs_dir = root.join(".sgf/logs");
        let log_files: Vec<_> = fs::read_dir(&logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "log"))
            .collect();
        assert_eq!(log_files.len(), 1);
        let log_content = fs::read_to_string(log_files[0].path()).unwrap();
        assert!(log_content.contains("iteration 1 of 30"));
        assert!(log_content.contains("task complete"));
    }

    #[test]
    fn run_no_push_passes_auto_push_false() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build {{spec}} now.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: true,
            iterations: 10,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--auto-push false"));
    }

    #[test]
    fn run_passes_assembled_prompt_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "build", "Build {{spec}} now.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        run(root, &config).unwrap();

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains(".sgf/prompts/.assembled/build.md"));

        let assembled = fs::read_to_string(root.join(".sgf/prompts/.assembled/build.md")).unwrap();
        assert_eq!(assembled, "Build auth now.");
    }

    #[test]
    fn verify_assembles_without_variables() {
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
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("--loop-id"));
        assert!(args_content.contains("verify-"));
        assert!(args_content.contains("-a"));

        let assembled = fs::read_to_string(root.join(".sgf/prompts/.assembled/verify.md")).unwrap();
        assert_eq!(assembled, "Verify all specs against codebase.");
    }

    #[test]
    fn spec_one_iteration_interactive() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "spec", "Interview the developer about specs.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "spec".to_string(),
            spec: None,
            afk: false,
            no_push: false,
            iterations: 1,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            !args_content.starts_with("-a "),
            "should not have --afk flag"
        );
        assert!(args_content.contains(" 1 "));
    }

    #[test]
    fn issues_log_uses_issues_template() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "issues", "Log a bug interactively.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "issues-log".to_string(),
            spec: None,
            afk: false,
            no_push: false,
            iterations: 1,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: Some("issues".to_string()),
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("issues-log-"));
        assert!(
            !args_content.starts_with("-a "),
            "should not have --afk flag"
        );
        assert!(args_content.contains(" 1 "));

        let assembled = fs::read_to_string(root.join(".sgf/prompts/.assembled/issues.md")).unwrap();
        assert_eq!(assembled, "Log a bug interactively.");
    }

    #[test]
    fn test_stage_substitutes_spec() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "test", "Test {{spec}} items.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "test".to_string(),
            spec: Some("auth".to_string()),
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let assembled = fs::read_to_string(root.join(".sgf/prompts/.assembled/test.md")).unwrap();
        assert_eq!(assembled, "Test auth items.");
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
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("test-plan-"));
    }

    #[test]
    fn issues_plan_no_variables() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_project(root, "issues-plan", "Plan fixes for open bugs.");
        setup_git_repo(root);

        let mock = mock_ralph_script(
            root,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/ralph_args.txt\"\nexit 0\n",
        );

        let config = LoopConfig {
            stage: "issues-plan".to_string(),
            spec: None,
            afk: true,
            no_push: false,
            iterations: 30,
            ralph_binary: Some(mock),
            skip_preflight: true,
            prompt_template: None,
        };

        let exit_code = run(root, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args_content = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(args_content.contains("issues-plan-"));
    }
}
