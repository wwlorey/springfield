use std::fs;
use std::io;
use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use shutdown::{ShutdownConfig, ShutdownController, ShutdownStatus};
use tracing::warn;
use uuid::Uuid;

use crate::cursus::context;
use crate::cursus::state::{self, CompletedIter, RunMetadata, RunStatus};
use crate::cursus::toml::{CursusDefinition, IterDefinition, Mode};
use crate::loop_mgmt;
use crate::style;

const SENTINEL_MAX_DEPTH: usize = 2;

const SENTINELS: &[&str] = &[".iter-complete", ".iter-reject", ".iter-revise"];

pub struct CursusConfig {
    pub spec: Option<String>,
    pub mode_override: Option<Mode>,
    pub no_push: bool,
    pub ralph_binary: Option<String>,
    pub skip_preflight: bool,
    pub monitor_stdin_override: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterOutcome {
    Complete,
    Reject,
    Revise,
    Exhausted,
}

impl std::fmt::Display for IterOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete => write!(f, "complete"),
            Self::Reject => write!(f, "reject"),
            Self::Revise => write!(f, "revise"),
            Self::Exhausted => write!(f, "exhausted"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextIter {
    Advance,
    Named(String),
    Stalled,
}

fn find_sentinel(dir: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.exists() {
        return Some(candidate);
    }
    if max_depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().ok().is_some_and(|ft| ft.is_dir())
            && let Some(found) = find_sentinel(&entry.path(), name, max_depth - 1)
        {
            return Some(found);
        }
    }
    None
}

pub fn detect_outcome(
    root: &Path,
    iter: &IterDefinition,
    effective_mode: &Mode,
    exit_code: i32,
) -> IterOutcome {
    if find_sentinel(root, ".iter-complete", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Complete;
    }
    if find_sentinel(root, ".iter-reject", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Reject;
    }
    if find_sentinel(root, ".iter-revise", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Revise;
    }
    if *effective_mode == Mode::Interactive && iter.iterations <= 1 {
        return IterOutcome::Complete;
    }
    if exit_code == 0 {
        return IterOutcome::Complete;
    }
    IterOutcome::Exhausted
}

pub fn clean_sentinels(root: &Path) {
    for name in SENTINELS {
        while let Some(path) = find_sentinel(root, name, SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn resolve_transition(iter: &IterDefinition, outcome: &IterOutcome) -> io::Result<NextIter> {
    match outcome {
        IterOutcome::Complete => {
            if let Some(ref target) = iter.next {
                Ok(NextIter::Named(target.clone()))
            } else {
                Ok(NextIter::Advance)
            }
        }
        IterOutcome::Reject => match iter.transitions.on_reject {
            Some(ref target) => Ok(NextIter::Named(target.clone())),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter '{}' signaled reject but no on_reject transition is defined",
                    iter.name
                ),
            )),
        },
        IterOutcome::Revise => match iter.transitions.on_revise {
            Some(ref target) => Ok(NextIter::Named(target.clone())),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter '{}' signaled revise but no on_revise transition is defined",
                    iter.name
                ),
            )),
        },
        IterOutcome::Exhausted => Ok(NextIter::Stalled),
    }
}

pub fn resolve_iter_index(
    iters: &[IterDefinition],
    current_index: usize,
    next: &NextIter,
) -> Option<usize> {
    match next {
        NextIter::Advance => {
            let next_idx = current_index + 1;
            if next_idx < iters.len() {
                Some(next_idx)
            } else {
                None
            }
        }
        NextIter::Named(name) => iters.iter().position(|i| i.name == *name),
        NextIter::Stalled => None,
    }
}

fn resolve_prompt(root: &Path, prompt: &str) -> Option<PathBuf> {
    let local = root.join(format!(".sgf/prompts/{prompt}"));
    if local.exists() {
        return Some(local);
    }
    let global = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(format!(".sgf/prompts/{prompt}")))?;
    if global.exists() {
        return Some(global);
    }
    None
}

fn resolve_ralph_binary(config: &CursusConfig) -> String {
    if let Some(ref bin) = config.ralph_binary {
        return bin.clone();
    }
    std::env::var("SGF_RALPH_BINARY").unwrap_or_else(|_| "ralph".to_string())
}

struct RalphInvocation<'a> {
    root: &'a Path,
    run_id: &'a str,
    iter: &'a IterDefinition,
    config: &'a CursusConfig,
    session_id: &'a str,
    prompt_path: &'a Path,
    consumed_content: &'a str,
    auto_push: bool,
}

const SPAWN_RETRY_ATTEMPTS: u32 = 3;
const SPAWN_RETRY_BASE_DELAY: Duration = Duration::from_secs(5);

fn spawn_with_retry(
    cmd: &mut Command,
    label: &str,
    controller: &ShutdownController,
) -> io::Result<std::process::Child> {
    for attempt in 0..SPAWN_RETRY_ATTEMPTS {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(e)
                if e.raw_os_error() == Some(libc::EAGAIN) && attempt + 1 < SPAWN_RETRY_ATTEMPTS =>
            {
                let delay = SPAWN_RETRY_BASE_DELAY * 2u32.pow(attempt);
                warn!(
                    attempt = attempt + 1,
                    max = SPAWN_RETRY_ATTEMPTS,
                    delay_secs = delay.as_secs(),
                    error = %e,
                    "resource pressure spawning {label}, retrying"
                );
                let deadline = Instant::now() + delay;
                while Instant::now() < deadline {
                    if controller.poll() == ShutdownStatus::Shutdown {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "shutdown"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(e) => {
                return Err(io::Error::other(format!("failed to spawn {label}: {e}")));
            }
        }
    }
    unreachable!()
}

fn invoke_ralph(inv: &RalphInvocation<'_>, controller: &ShutdownController) -> io::Result<i32> {
    let binary = resolve_ralph_binary(inv.config);

    let mut args = vec!["-a".to_string()];

    args.push("--loop-id".to_string());
    args.push(inv.run_id.to_string());

    args.push("--session-id".to_string());
    args.push(inv.session_id.to_string());

    args.push("--auto-push".to_string());
    args.push(inv.auto_push.to_string());

    if inv.iter.banner {
        args.push("--banner".to_string());
    }

    if !inv.consumed_content.is_empty() {
        let ctx_file = context::context_file_path(inv.root, inv.run_id, "_consumed");
        fs::write(&ctx_file, inv.consumed_content)?;
        args.push("--prompt-file".to_string());
        args.push(ctx_file.to_string_lossy().to_string());
    }

    let log_path = loop_mgmt::create_log_file(inv.root, inv.run_id)?;
    args.push("--log-file".to_string());
    args.push(log_path.to_string_lossy().to_string());

    args.push(inv.iter.iterations.to_string());
    args.push(inv.prompt_path.to_string_lossy().to_string());

    let mut cmd = Command::new(&binary);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("SGF_MANAGED", "1");

    let (ctx_env_name, ctx_env_val) = context::context_env_var(inv.run_id);
    cmd.env(&ctx_env_name, &ctx_env_val);

    if std::env::var("SGF_TEST_NO_SETSID").is_err() {
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut child = spawn_with_retry(&mut cmd, "ralph", controller)?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(1)),
            Ok(None) => {
                if controller.poll() == ShutdownStatus::Shutdown {
                    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
                    let _ = child.wait();
                    return Ok(130);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn invoke_cl(
    prompt_path: &Path,
    session_id: &str,
    consumed_content: &str,
    run_id: &str,
    controller: &ShutdownController,
) -> io::Result<i32> {
    let prompt_arg = format!("@{}", prompt_path.display());
    let mut args = vec![
        "--verbose".to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
    ];

    if !consumed_content.is_empty() {
        args.push("--append-system-prompt".to_string());
        args.push(consumed_content.to_string());
    }

    args.push(prompt_arg);

    let mut cmd = Command::new("cl");
    cmd.args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let (ctx_env_name, ctx_env_val) = context::context_env_var(run_id);
    cmd.env(&ctx_env_name, &ctx_env_val);

    let mut child = spawn_with_retry(&mut cmd, "cl", controller)?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(1)),
            Ok(None) => {
                if controller.poll() == ShutdownStatus::Shutdown {
                    shutdown::kill_process_group(child.id(), Duration::from_millis(200));
                    let _ = child.wait();
                    return Ok(130);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn print_stall_banner(cursus_name: &str, iter_name: &str, iterations: u32, run_id: &str) {
    let detail = format!(
        "iter: {} · reason: iterations exhausted ({}/{}) · resume: sgf resume {}",
        iter_name, iterations, iterations, run_id
    );
    style::print_warning_detail(&format!("cursus STALLED [{cursus_name}]"), &detail);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAction {
    Retry,
    Skip,
    Abort,
}

fn prompt_resume_action(meta: &RunMetadata) -> io::Result<ResumeAction> {
    eprintln!();
    style::print_warning_detail(
        &format!("cursus {} [{run_id}]", meta.status, run_id = meta.run_id),
        &format!(
            "iter: {} · completed: {} of {} iters",
            meta.current_iter,
            meta.iters_completed.len(),
            meta.current_iter_index as usize + meta.iters_completed.len() + 1
        ),
    );
    eprintln!();
    eprintln!("  1. Retry  — re-run the stalled iter");
    eprintln!("  2. Skip   — advance to the next iter");
    eprintln!("  3. Abort  — mark run as interrupted and exit");
    eprintln!();
    eprint!("Select action (1-3): ");

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim() {
        "1" | "retry" => Ok(ResumeAction::Retry),
        "2" | "skip" => Ok(ResumeAction::Skip),
        "3" | "abort" => Ok(ResumeAction::Abort),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid selection: {other}"),
        )),
    }
}

fn run_cursus_loop(
    root: &Path,
    cursus_name: &str,
    def: &CursusDefinition,
    config: &CursusConfig,
    metadata: &mut RunMetadata,
    start_index: usize,
) -> io::Result<i32> {
    if let Ok(ready_path) = std::env::var("SGF_READY_FILE") {
        let _ = fs::write(&ready_path, "");
    }

    let mut current_index = start_index;

    let exit_code = loop {
        let iter = &def.iters[current_index];

        metadata.current_iter = iter.name.clone();
        metadata.current_iter_index = current_index as u32;
        metadata.status = RunStatus::Running;
        metadata.touch();
        state::write_metadata(root, metadata)?;

        clean_sentinels(root);

        let consumed_content =
            context::resolve_consumes(root, &metadata.run_id, &iter.consumes, def);

        let effective_mode = config
            .mode_override
            .clone()
            .unwrap_or_else(|| iter.mode.clone());

        let is_afk = effective_mode == Mode::Afk;
        let monitor_stdin = config.monitor_stdin_override.unwrap_or_else(|| {
            if is_afk {
                std::env::var("SGF_MONITOR_STDIN")
                    .map_or_else(|_| std::io::stdin().is_terminal(), |v| v != "0")
            } else {
                false
            }
        });
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin,
            ..Default::default()
        })?;
        let session_id = Uuid::new_v4().to_string();

        let prompt_path = resolve_prompt(root, &iter.prompt).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("prompt not found: {}", iter.prompt),
            )
        })?;

        let auto_push = !config.no_push && def.effective_auto_push(iter);

        style::print_action_detail(
            &format!(
                "cursus [{cursus_name}] iter: {} ({}/{})",
                iter.name,
                current_index + 1,
                def.iters.len()
            ),
            &format!(
                "mode: {} · iterations: {}",
                match effective_mode {
                    Mode::Afk => "afk",
                    Mode::Interactive => "interactive",
                },
                iter.iterations
            ),
        );

        let head_before = if auto_push && effective_mode == Mode::Interactive {
            vcs_utils::git_head()
        } else {
            None
        };

        let exit_code = match effective_mode {
            Mode::Afk => invoke_ralph(
                &RalphInvocation {
                    root,
                    run_id: &metadata.run_id,
                    iter,
                    config,
                    session_id: &session_id,
                    prompt_path: &prompt_path,
                    consumed_content: &consumed_content,
                    auto_push,
                },
                &controller,
            )?,
            Mode::Interactive => invoke_cl(
                &prompt_path,
                &session_id,
                &consumed_content,
                &metadata.run_id,
                &controller,
            )?,
        };

        if let Some(ref before) = head_before {
            vcs_utils::auto_push_if_changed(before, |msg| {
                style::print_action(msg);
            });
        }

        if exit_code == 130 {
            metadata.status = RunStatus::Interrupted;
            metadata.touch();
            let _ = state::write_metadata(root, metadata);
            state::remove_pid_file(root, &metadata.run_id);
            return Ok(130);
        }

        let outcome = detect_outcome(root, iter, &effective_mode, exit_code);
        clean_sentinels(root);

        metadata.iters_completed.push(CompletedIter {
            name: iter.name.clone(),
            session_id,
            completed_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            outcome: outcome.to_string(),
        });

        if let Some(ref key) = iter.produces {
            context::check_produces(root, &metadata.run_id, key);
        }

        let transition = resolve_transition(iter, &outcome)?;

        match transition {
            NextIter::Stalled => {
                metadata.status = RunStatus::Stalled;
                metadata.touch();
                state::write_metadata(root, metadata)?;
                print_stall_banner(cursus_name, &iter.name, iter.iterations, &metadata.run_id);
                state::remove_pid_file(root, &metadata.run_id);
                break 2;
            }
            _ => match resolve_iter_index(&def.iters, current_index, &transition) {
                Some(next_idx) => {
                    current_index = next_idx;
                }
                None => {
                    metadata.status = RunStatus::Completed;
                    metadata.touch();
                    state::write_metadata(root, metadata)?;
                    style::print_success(&format!("cursus complete [{cursus_name}]"));
                    state::remove_pid_file(root, &metadata.run_id);
                    break 0;
                }
            },
        }
    };

    Ok(exit_code)
}

pub fn run_cursus(
    root: &Path,
    cursus_name: &str,
    def: &CursusDefinition,
    config: &CursusConfig,
) -> io::Result<i32> {
    if def.iters.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cursus has no iters defined",
        ));
    }

    state::mark_stale_runs_interrupted(root)?;

    let mode_override_str = config.mode_override.as_ref().map(|m| match m {
        Mode::Afk => "afk",
        Mode::Interactive => "interactive",
    });

    let mut metadata = RunMetadata::new(
        cursus_name,
        &def.iters[0].name,
        config.spec.as_deref(),
        mode_override_str,
    );

    state::create_run_dir(root, &metadata.run_id)?;
    state::write_pid_file(root, &metadata.run_id)?;
    state::write_metadata(root, &metadata)?;

    run_cursus_loop(root, cursus_name, def, config, &mut metadata, 0)
}

pub fn resume_cursus(root: &Path, run_id: &str) -> io::Result<i32> {
    state::mark_stale_runs_interrupted(root)?;

    let mut metadata = state::read_metadata(root, run_id)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("run not found: {run_id}"))
    })?;

    if metadata.status != RunStatus::Stalled && metadata.status != RunStatus::Interrupted {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "run {} is not resumable (status: {})",
                run_id, metadata.status
            ),
        ));
    }

    let resolved = crate::cursus::resolve_cursus(root, &metadata.cursus).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("cursus definition not found: {}", metadata.cursus),
        )
    })?;

    let def = &resolved.definition;

    let stalled_index = def
        .iters
        .iter()
        .position(|i| i.name == metadata.current_iter)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter '{}' not found in cursus '{}'",
                    metadata.current_iter, metadata.cursus
                ),
            )
        })?;

    let action = prompt_resume_action(&metadata)?;

    let cursus_name = metadata.cursus.clone();

    let mode_override = metadata.mode_override.as_deref().map(|m| match m {
        "afk" => Mode::Afk,
        _ => Mode::Interactive,
    });

    let config = CursusConfig {
        spec: metadata.spec.clone(),
        mode_override,
        no_push: false,
        ralph_binary: None,
        skip_preflight: true,
        monitor_stdin_override: None,
    };

    match action {
        ResumeAction::Abort => {
            metadata.status = RunStatus::Interrupted;
            metadata.touch();
            state::write_metadata(root, &metadata)?;
            style::print_warning(&format!("run aborted [{run_id}]"));
            Ok(1)
        }
        ResumeAction::Retry => {
            state::write_pid_file(root, run_id)?;
            style::print_action(&format!(
                "retrying iter '{}' [{run_id}]",
                metadata.current_iter
            ));
            run_cursus_loop(
                root,
                &cursus_name,
                def,
                &config,
                &mut metadata,
                stalled_index,
            )
        }
        ResumeAction::Skip => {
            let next_index = stalled_index + 1;
            if next_index >= def.iters.len() {
                metadata.status = RunStatus::Completed;
                metadata.touch();
                state::write_metadata(root, &metadata)?;
                style::print_success(&format!(
                    "cursus complete [{cursus_name}] (skipped final iter)"
                ));
                return Ok(0);
            }

            state::write_pid_file(root, run_id)?;
            style::print_action(&format!(
                "skipping to iter '{}' [{run_id}]",
                def.iters[next_index].name
            ));
            run_cursus_loop(root, &cursus_name, def, &config, &mut metadata, next_index)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursus::toml::Transitions;
    use tempfile::TempDir;

    fn make_iter(
        name: &str,
        mode: Mode,
        iterations: u32,
        next: Option<&str>,
        on_reject: Option<&str>,
        on_revise: Option<&str>,
    ) -> IterDefinition {
        IterDefinition {
            name: name.to_string(),
            prompt: format!("{name}.md"),
            mode,
            iterations,
            produces: None,
            consumes: vec![],
            auto_push: None,
            next: next.map(|s| s.to_string()),
            banner: false,
            transitions: Transitions {
                on_reject: on_reject.map(|s| s.to_string()),
                on_revise: on_revise.map(|s| s.to_string()),
            },
        }
    }

    #[test]
    fn detect_complete_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Complete
        );
    }

    #[test]
    fn detect_reject_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-reject"), "").unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, Some("draft"), None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Reject
        );
    }

    #[test]
    fn detect_revise_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, None, Some("revise"));
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Revise
        );
    }

    #[test]
    fn complete_wins_over_reject_and_revise() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-complete"), "").unwrap();
        fs::write(tmp.path().join(".iter-reject"), "").unwrap();
        fs::write(tmp.path().join(".iter-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Afk, 10, None, Some("draft"), Some("fix"));
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Complete
        );
    }

    #[test]
    fn reject_wins_over_revise() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-reject"), "").unwrap();
        fs::write(tmp.path().join(".iter-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Afk, 10, None, Some("draft"), Some("fix"));
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Reject
        );
    }

    #[test]
    fn interactive_no_sentinel_is_complete() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Complete
        );
    }

    #[test]
    fn afk_no_sentinel_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Exhausted
        );
    }

    #[test]
    fn interactive_multi_iteration_no_sentinel_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("review", Mode::Interactive, 5, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Exhausted
        );
    }

    #[test]
    fn nested_sentinel_detected() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".iter-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Complete
        );
    }

    #[test]
    fn sentinel_too_deep_not_detected() {
        let tmp = TempDir::new().unwrap();
        let deep = tmp.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(".iter-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Interactive, 5, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Exhausted
        );
    }

    #[test]
    fn clean_sentinels_removes_all() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-complete"), "").unwrap();
        fs::write(tmp.path().join(".iter-reject"), "").unwrap();
        fs::write(tmp.path().join(".iter-revise"), "").unwrap();
        let nested = tmp.path().join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".iter-complete"), "").unwrap();

        clean_sentinels(tmp.path());

        assert!(!tmp.path().join(".iter-complete").exists());
        assert!(!tmp.path().join(".iter-reject").exists());
        assert!(!tmp.path().join(".iter-revise").exists());
        assert!(!nested.join(".iter-complete").exists());
    }

    #[test]
    fn clean_sentinels_noop_when_none() {
        let tmp = TempDir::new().unwrap();
        clean_sentinels(tmp.path());
    }

    #[test]
    fn resolve_complete_advances() {
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        let next = resolve_transition(&iter, &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Advance);
    }

    #[test]
    fn resolve_complete_with_next_override() {
        let iter = make_iter("revise", Mode::Afk, 5, Some("review"), None, None);
        let next = resolve_transition(&iter, &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Named("review".to_string()));
    }

    #[test]
    fn resolve_reject_follows_on_reject() {
        let iter = make_iter("review", Mode::Interactive, 1, None, Some("draft"), None);
        let next = resolve_transition(&iter, &IterOutcome::Reject).unwrap();
        assert_eq!(next, NextIter::Named("draft".to_string()));
    }

    #[test]
    fn resolve_reject_without_transition_errors() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        let err = resolve_transition(&iter, &IterOutcome::Reject).unwrap_err();
        assert!(
            err.to_string()
                .contains("iter 'review' signaled reject but no on_reject transition is defined")
        );
    }

    #[test]
    fn resolve_revise_follows_on_revise() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, Some("revise"));
        let next = resolve_transition(&iter, &IterOutcome::Revise).unwrap();
        assert_eq!(next, NextIter::Named("revise".to_string()));
    }

    #[test]
    fn resolve_revise_without_transition_errors() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        let err = resolve_transition(&iter, &IterOutcome::Revise).unwrap_err();
        assert!(
            err.to_string()
                .contains("iter 'review' signaled revise but no on_revise transition is defined")
        );
    }

    #[test]
    fn resolve_exhausted_stalls() {
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        let next = resolve_transition(&iter, &IterOutcome::Exhausted).unwrap();
        assert_eq!(next, NextIter::Stalled);
    }

    #[test]
    fn resolve_iter_index_advance() {
        let iters = vec![
            make_iter("a", Mode::Afk, 1, None, None, None),
            make_iter("b", Mode::Afk, 1, None, None, None),
            make_iter("c", Mode::Afk, 1, None, None, None),
        ];
        assert_eq!(resolve_iter_index(&iters, 0, &NextIter::Advance), Some(1));
        assert_eq!(resolve_iter_index(&iters, 1, &NextIter::Advance), Some(2));
        assert_eq!(resolve_iter_index(&iters, 2, &NextIter::Advance), None);
    }

    #[test]
    fn resolve_iter_index_named() {
        let iters = vec![
            make_iter("draft", Mode::Afk, 1, None, None, None),
            make_iter("review", Mode::Interactive, 1, None, None, None),
            make_iter("approve", Mode::Interactive, 1, None, None, None),
        ];
        assert_eq!(
            resolve_iter_index(&iters, 1, &NextIter::Named("draft".to_string())),
            Some(0)
        );
        assert_eq!(
            resolve_iter_index(&iters, 0, &NextIter::Named("approve".to_string())),
            Some(2)
        );
        assert_eq!(
            resolve_iter_index(&iters, 0, &NextIter::Named("nonexistent".to_string())),
            None
        );
    }

    #[test]
    fn resolve_iter_index_stalled() {
        let iters = vec![make_iter("a", Mode::Afk, 1, None, None, None)];
        assert_eq!(resolve_iter_index(&iters, 0, &NextIter::Stalled), None);
    }

    #[test]
    fn back_edge_transition() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".iter-reject"), "").unwrap();

        let iters = vec![
            make_iter("draft", Mode::Afk, 10, None, None, None),
            make_iter("review", Mode::Interactive, 1, None, Some("draft"), None),
        ];

        let outcome = detect_outcome(tmp.path(), &iters[1], &iters[1].mode, 2);
        assert_eq!(outcome, IterOutcome::Reject);

        let next = resolve_transition(&iters[1], &outcome).unwrap();
        assert_eq!(next, NextIter::Named("draft".to_string()));

        let idx = resolve_iter_index(&iters, 1, &next);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn exit_code_zero_without_sentinel_is_complete() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 0),
            IterOutcome::Complete
        );
    }

    #[test]
    fn exit_code_nonzero_without_sentinel_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(
            detect_outcome(tmp.path(), &iter, &iter.mode, 2),
            IterOutcome::Exhausted
        );
    }

    #[test]
    fn final_iter_complete_returns_none() {
        let iters = vec![
            make_iter("build", Mode::Afk, 10, None, None, None),
            make_iter("approve", Mode::Interactive, 1, None, None, None),
        ];
        let next = resolve_transition(&iters[1], &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Advance);
        assert_eq!(resolve_iter_index(&iters, 1, &next), None);
    }

    #[test]
    fn outcome_display() {
        assert_eq!(IterOutcome::Complete.to_string(), "complete");
        assert_eq!(IterOutcome::Reject.to_string(), "reject");
        assert_eq!(IterOutcome::Revise.to_string(), "revise");
        assert_eq!(IterOutcome::Exhausted.to_string(), "exhausted");
    }

    fn setup_cursus_project(root: &Path, prompts: &[&str]) {
        fs::create_dir_all(root.join(".sgf/prompts")).unwrap();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();
        fs::create_dir_all(root.join(".sgf/logs")).unwrap();
        for name in prompts {
            fs::write(
                root.join(format!(".sgf/prompts/{name}")),
                format!("Prompt for {name}"),
            )
            .unwrap();
        }
    }

    fn mock_script(root: &Path, name: &str, script: &str) -> String {
        let path = root.join(name);
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().to_string()
    }

    fn make_cursus_def(iters: Vec<IterDefinition>, auto_push: bool) -> CursusDefinition {
        CursusDefinition {
            description: "test cursus".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push,
            iters,
        }
    }

    fn make_iter_full(
        name: &str,
        mode: Mode,
        iterations: u32,
        produces: Option<&str>,
        consumes: Vec<&str>,
        next: Option<&str>,
        on_reject: Option<&str>,
        on_revise: Option<&str>,
    ) -> IterDefinition {
        IterDefinition {
            name: name.to_string(),
            prompt: format!("{name}.md"),
            mode,
            iterations,
            produces: produces.map(|s| s.to_string()),
            consumes: consumes.into_iter().map(|s| s.to_string()).collect(),
            auto_push: None,
            next: next.map(|s| s.to_string()),
            banner: false,
            transitions: Transitions {
                on_reject: on_reject.map(|s| s.to_string()),
                on_revise: on_revise.map(|s| s.to_string()),
            },
        }
    }

    #[test]
    fn run_cursus_single_iter_complete() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["build.md"]);

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![make_iter("build", Mode::Afk, 10, None, None, None)],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "build", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        // Verify metadata was written
        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        assert_eq!(entries.len(), 1);

        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();
        assert_eq!(meta.status, RunStatus::Completed);
        assert_eq!(meta.cursus, "build");
        assert_eq!(meta.iters_completed.len(), 1);
        assert_eq!(meta.iters_completed[0].name, "build");
        assert_eq!(meta.iters_completed[0].outcome, "complete");
    }

    #[test]
    fn run_cursus_stall_on_exhausted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["build.md"]);

        // Mock that does NOT create any sentinel and exits 2 → exhausted for AFK mode
        let ralph = mock_script(root, "mock_ralph.sh", "#!/bin/sh\nexit 2\n");

        let def = make_cursus_def(
            vec![make_iter("build", Mode::Afk, 5, None, None, None)],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "build", &def, &config).unwrap();
        assert_eq!(exit_code, 2);

        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();
        assert_eq!(meta.status, RunStatus::Stalled);
    }

    #[test]
    fn run_cursus_multi_iter_happy_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["draft.md", "review.md", "approve.md"]);

        // Mock that creates .iter-complete for all iters
        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter("draft", Mode::Afk, 10, None, None, None),
                make_iter("review", Mode::Afk, 1, None, None, None),
                make_iter("approve", Mode::Afk, 1, None, None, None),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: Some(Mode::Afk),
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "spec", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();
        assert_eq!(meta.status, RunStatus::Completed);
        assert_eq!(meta.iters_completed.len(), 3);
        assert_eq!(meta.iters_completed[0].name, "draft");
        assert_eq!(meta.iters_completed[1].name, "review");
        assert_eq!(meta.iters_completed[2].name, "approve");
    }

    #[test]
    fn run_cursus_reject_transition() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["draft.md", "review.md"]);

        // First call creates .iter-complete, second creates .iter-reject,
        // third (back to draft) creates .iter-complete, fourth creates .iter-complete
        let counter_file = root.join("call_count");
        fs::write(&counter_file, "0").unwrap();

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                r#"#!/bin/sh
COUNT=$(cat "{counter}")
COUNT=$((COUNT + 1))
echo $COUNT > "{counter}"
if [ $COUNT -eq 2 ]; then
    touch "{root}/.iter-reject"
else
    touch "{root}/.iter-complete"
fi
exit 0
"#,
                counter = counter_file.display(),
                root = root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter("draft", Mode::Afk, 10, None, None, None),
                make_iter("review", Mode::Afk, 1, None, Some("draft"), None),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "spec", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();
        assert_eq!(meta.status, RunStatus::Completed);
        // draft → review (reject) → draft → review → done
        assert_eq!(meta.iters_completed.len(), 4);
        assert_eq!(meta.iters_completed[0].name, "draft");
        assert_eq!(meta.iters_completed[0].outcome, "complete");
        assert_eq!(meta.iters_completed[1].name, "review");
        assert_eq!(meta.iters_completed[1].outcome, "reject");
        assert_eq!(meta.iters_completed[2].name, "draft");
        assert_eq!(meta.iters_completed[2].outcome, "complete");
        assert_eq!(meta.iters_completed[3].name, "review");
        assert_eq!(meta.iters_completed[3].outcome, "complete");
    }

    #[test]
    fn run_cursus_context_passing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["generate.md", "verify.md"]);

        // Mock that captures args and writes a produces file
        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                r#"#!/bin/sh
echo "$@" >> "{root}/ralph_calls.txt"
# Write produces file if SGF_RUN_CONTEXT is set
if [ -n "$SGF_RUN_CONTEXT" ]; then
    echo "Generated output summary." > "$SGF_RUN_CONTEXT/output-summary.md"
fi
touch "{root}/.iter-complete"
exit 0
"#,
                root = root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter_full(
                    "generate",
                    Mode::Afk,
                    5,
                    Some("output-summary"),
                    vec![],
                    None,
                    None,
                    None,
                ),
                make_iter_full(
                    "verify",
                    Mode::Afk,
                    1,
                    None,
                    vec!["output-summary"],
                    None,
                    None,
                    None,
                ),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "pipeline", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        // Verify context file was used: verify iter should have received --prompt-file
        let calls = fs::read_to_string(root.join("ralph_calls.txt")).unwrap();
        let lines: Vec<&str> = calls.lines().collect();
        assert_eq!(lines.len(), 2);
        // Second call (verify) should have --prompt-file pointing to consumed context
        assert!(
            lines[1].contains("--prompt-file"),
            "verify iter should receive --prompt-file, got: {}",
            lines[1]
        );
    }

    #[test]
    fn run_cursus_mode_override() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["build.md"]);

        // Mock that captures args
        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\necho \"$@\" > \"{}/ralph_args.txt\"\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display(),
                root.display()
            ),
        );

        // Iter is interactive by default, but mode_override forces AFK
        let def = make_cursus_def(
            vec![make_iter("build", Mode::Interactive, 1, None, None, None)],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: Some(Mode::Afk),
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "build", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        // Should have invoked ralph (AFK), not cl
        let args = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            args.contains("--loop-id"),
            "should invoke ralph with --loop-id, got: {args}"
        );
    }

    #[test]
    fn run_cursus_spec_passthrough() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["build.md"]);

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\necho \"$@\" > \"{}/ralph_args.txt\"\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display(),
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![make_iter("build", Mode::Afk, 10, None, None, None)],
            false,
        );

        let config = CursusConfig {
            spec: Some("auth".to_string()),
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "build", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        let args = fs::read_to_string(root.join("ralph_args.txt")).unwrap();
        assert!(
            !args.contains("--spec"),
            "should NOT pass --spec to ralph, got: {args}"
        );

        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();
        assert_eq!(meta.spec.as_deref(), Some("auth"));
    }

    #[test]
    fn run_cursus_pid_file_cleaned_up() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["build.md"]);

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![make_iter("build", Mode::Afk, 1, None, None, None)],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        run_cursus(root, "build", &def, &config).unwrap();

        // PID file should be cleaned up
        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let pid_file = state::pid_path(root, &run_id);
        assert!(!pid_file.exists(), "PID file should be removed after run");
    }

    #[test]
    fn run_cursus_empty_iters_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &[]);

        let def = make_cursus_def(vec![], false);

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: None,
            skip_preflight: true,
            monitor_stdin_override: None,
        };

        let err = run_cursus(root, "empty", &def, &config).unwrap_err();
        assert!(err.to_string().contains("no iters defined"));
    }

    #[test]
    fn run_cursus_next_override() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["draft.md", "revise.md", "review.md"]);

        let counter_file = root.join("call_count");
        fs::write(&counter_file, "0").unwrap();

        // draft → complete → revise (via next override) → complete → review → complete
        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter_full(
                    "draft",
                    Mode::Afk,
                    10,
                    None,
                    vec![],
                    Some("revise"),
                    None,
                    None,
                ),
                make_iter_full("revise", Mode::Afk, 5, None, vec![], None, None, None),
                make_iter_full("review", Mode::Afk, 1, None, vec![], None, None, None),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let exit_code = run_cursus(root, "spec", &def, &config).unwrap();
        assert_eq!(exit_code, 0);

        let run_dir = root.join(".sgf/run");
        let entries: Vec<_> = fs::read_dir(&run_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|ft| ft.is_dir()))
            .collect();
        let run_id = entries[0].file_name().to_str().unwrap().to_string();
        let meta = state::read_metadata(root, &run_id).unwrap().unwrap();

        // draft → revise (next override) → review → done
        assert_eq!(meta.iters_completed.len(), 3);
        assert_eq!(meta.iters_completed[0].name, "draft");
        assert_eq!(meta.iters_completed[1].name, "revise");
        assert_eq!(meta.iters_completed[2].name, "review");
    }

    #[test]
    fn resolve_prompt_local() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/prompts")).unwrap();
        fs::write(root.join(".sgf/prompts/build.md"), "prompt content").unwrap();

        let result = resolve_prompt(root, "build.md");
        assert!(result.is_some());
        assert!(result.unwrap().ends_with(".sgf/prompts/build.md"));
    }

    #[test]
    fn resolve_prompt_missing() {
        let tmp = TempDir::new().unwrap();
        let result = resolve_prompt(tmp.path(), "nonexistent.md");
        assert!(result.is_none());
    }

    #[test]
    fn resume_cursus_nonexistent_run_errors() {
        let tmp = TempDir::new().unwrap();
        let err = resume_cursus(tmp.path(), "nonexistent-run").unwrap_err();
        assert!(err.to_string().contains("run not found"));
    }

    #[test]
    fn resume_cursus_completed_run_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260317T140000";
        state::create_run_dir(root, run_id).unwrap();
        state::write_metadata(
            root,
            &RunMetadata {
                run_id: run_id.to_string(),
                cursus: "build".to_string(),
                status: RunStatus::Completed,
                current_iter: "build".to_string(),
                current_iter_index: 0,
                iters_completed: Vec::new(),
                spec: None,
                mode_override: None,
                created_at: "2026-03-17T14:00:00Z".to_string(),
                updated_at: "2026-03-17T14:05:00Z".to_string(),
            },
        )
        .unwrap();

        let err = resume_cursus(root, run_id).unwrap_err();
        assert!(err.to_string().contains("not resumable"));
    }

    #[test]
    fn resume_cursus_missing_cursus_def_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "ghost-20260317T140000";
        state::create_run_dir(root, run_id).unwrap();
        state::write_metadata(
            root,
            &RunMetadata {
                run_id: run_id.to_string(),
                cursus: "ghost".to_string(),
                status: RunStatus::Stalled,
                current_iter: "build".to_string(),
                current_iter_index: 0,
                iters_completed: Vec::new(),
                spec: None,
                mode_override: None,
                created_at: "2026-03-17T14:00:00Z".to_string(),
                updated_at: "2026-03-17T14:05:00Z".to_string(),
            },
        )
        .unwrap();
        fs::create_dir_all(root.join(".sgf/cursus")).unwrap();

        let err = resume_cursus(root, run_id).unwrap_err();
        assert!(err.to_string().contains("cursus definition not found"));
    }

    #[test]
    fn resume_cursus_stalled_iter_not_in_def_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260317T140000";
        state::create_run_dir(root, run_id).unwrap();
        state::write_metadata(
            root,
            &RunMetadata {
                run_id: run_id.to_string(),
                cursus: "build".to_string(),
                status: RunStatus::Stalled,
                current_iter: "deleted-iter".to_string(),
                current_iter_index: 0,
                iters_completed: Vec::new(),
                spec: None,
                mode_override: None,
                created_at: "2026-03-17T14:00:00Z".to_string(),
                updated_at: "2026-03-17T14:05:00Z".to_string(),
            },
        )
        .unwrap();
        fs::create_dir_all(root.join(".sgf/cursus")).unwrap();
        fs::write(
            root.join(".sgf/cursus/build.toml"),
            r#"
description = "Build"

[[iter]]
name = "build"
prompt = "build.md"
"#,
        )
        .unwrap();

        let err = resume_cursus(root, run_id).unwrap_err();
        assert!(err.to_string().contains("iter 'deleted-iter' not found"));
    }

    #[test]
    fn run_cursus_loop_resumes_from_stalled_iter() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["draft.md", "review.md", "approve.md"]);

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter("draft", Mode::Afk, 10, None, None, None),
                make_iter("review", Mode::Afk, 1, None, None, None),
                make_iter("approve", Mode::Afk, 1, None, None, None),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let run_id = "spec-20260317T140000";
        state::create_run_dir(root, run_id).unwrap();
        state::write_pid_file(root, run_id).unwrap();

        let mut metadata = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "spec".to_string(),
            status: RunStatus::Stalled,
            current_iter: "review".to_string(),
            current_iter_index: 1,
            iters_completed: vec![CompletedIter {
                name: "draft".to_string(),
                session_id: "sess-1".to_string(),
                completed_at: "2026-03-17T14:05:00Z".to_string(),
                outcome: "complete".to_string(),
            }],
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:10:00Z".to_string(),
        };

        let exit_code = run_cursus_loop(root, "spec", &def, &config, &mut metadata, 1).unwrap();
        assert_eq!(exit_code, 0);

        assert_eq!(metadata.status, RunStatus::Completed);
        assert_eq!(metadata.iters_completed.len(), 3);
        assert_eq!(metadata.iters_completed[0].name, "draft");
        assert_eq!(metadata.iters_completed[1].name, "review");
        assert_eq!(metadata.iters_completed[2].name, "approve");
    }

    #[test]
    fn run_cursus_loop_skip_resumes_from_next_iter() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        setup_cursus_project(root, &["draft.md", "review.md", "approve.md"]);

        let ralph = mock_script(
            root,
            "mock_ralph.sh",
            &format!(
                "#!/bin/sh\ntouch \"{}/.iter-complete\"\nexit 0\n",
                root.display()
            ),
        );

        let def = make_cursus_def(
            vec![
                make_iter("draft", Mode::Afk, 10, None, None, None),
                make_iter("review", Mode::Afk, 1, None, None, None),
                make_iter("approve", Mode::Afk, 1, None, None, None),
            ],
            false,
        );

        let config = CursusConfig {
            spec: None,
            mode_override: None,
            no_push: true,
            ralph_binary: Some(ralph),
            skip_preflight: true,
            monitor_stdin_override: Some(false),
        };

        let run_id = "spec-20260317T140000";
        state::create_run_dir(root, run_id).unwrap();
        state::write_pid_file(root, run_id).unwrap();

        let mut metadata = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "spec".to_string(),
            status: RunStatus::Stalled,
            current_iter: "draft".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:10:00Z".to_string(),
        };

        // Skip stalled "draft" iter, resume from "review" (index 1)
        let exit_code = run_cursus_loop(root, "spec", &def, &config, &mut metadata, 1).unwrap();
        assert_eq!(exit_code, 0);

        assert_eq!(metadata.status, RunStatus::Completed);
        assert_eq!(metadata.iters_completed.len(), 2);
        assert_eq!(metadata.iters_completed[0].name, "review");
        assert_eq!(metadata.iters_completed[1].name, "approve");
    }
}
