use std::ffi::OsString;
use std::path::Path;

use clap::{Parser, Subcommand};
use shutdown::{ShutdownConfig, ShutdownController};

use springfield::cursus;
use springfield::iter_runner::IterRunnerConfig;

/// CLI entry point for Springfield — scaffolding, prompt delivery, loop orchestration.
#[derive(Parser)]
#[command(name = "sgf")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new project
    Init {
        #[arg(long)]
        force: bool,
        /// Skip frontend scaffolding (pnpm create vite)
        #[arg(long)]
        no_fe: bool,
    },

    /// Show available commands with descriptions
    List,

    /// List and resume a previous session
    Resume {
        /// Run ID to resume directly
        run_id: Option<String>,
    },

    /// Kill a running cursus and mark it resumable
    Kill {
        /// Run ID to kill
        run_id: String,
    },

    /// Tail a running loop's output
    Logs {
        /// Loop ID to tail
        loop_id: String,
    },

    #[command(external_subcommand)]
    Dynamic(Vec<OsString>),
}

#[derive(Debug)]
struct DynamicArgs {
    command: String,
    spec: Option<String>,
    afk: bool,
    interactive: bool,
    iterations: Option<u32>,
    no_push: bool,
    skip_preflight: bool,
    resume: Option<String>,
    output_format: Option<String>,
}

fn parse_dynamic_args(args: Vec<OsString>) -> Result<DynamicArgs, String> {
    if args.is_empty() {
        return Err("no command specified".to_string());
    }

    let strs: Vec<String> = args
        .into_iter()
        .map(|a| {
            a.into_string()
                .map_err(|_| "invalid UTF-8 in argument".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let command = strs[0].clone();
    let rest = &strs[1..];

    let mut spec = None;
    let mut afk = false;
    let mut interactive = false;
    let mut iterations = None;
    let mut no_push = false;
    let mut skip_preflight = false;
    let mut resume = None;
    let mut output_format = None;

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-a" | "--afk" => afk = true,
            "-i" | "--interactive" => interactive = true,
            "--no-push" => no_push = true,
            "--skip-preflight" => skip_preflight = true,
            "--resume" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--resume requires a value".to_string());
                }
                resume = Some(rest[i].clone());
            }
            "--output-format" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--output-format requires a value".to_string());
                }
                let val = rest[i].clone();
                if val != "json" {
                    return Err(format!("unsupported output format: {val}"));
                }
                output_format = Some(val);
            }
            "-n" | "--iterations" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--iterations requires a value".to_string());
                }
                let mut n = rest[i]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid iteration count: {}", rest[i]))?;
                if n > springfield::iter_runner::MAX_ITERATIONS {
                    tracing::warn!(
                        requested = n,
                        max = springfield::iter_runner::MAX_ITERATIONS,
                        "clamping -n to hard limit"
                    );
                    n = springfield::iter_runner::MAX_ITERATIONS;
                }
                iterations = Some(n);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag: {arg}"));
            }
            positional => {
                if spec.is_some() {
                    return Err(format!("unexpected argument: {positional}"));
                }
                spec = Some(positional.to_string());
            }
        }
        i += 1;
    }

    if afk && interactive {
        return Err("-a/--afk and -i/--interactive are mutually exclusive".to_string());
    }
    if resume.is_some() && afk {
        return Err("--resume and -a/--afk are mutually exclusive".to_string());
    }
    if resume.is_some() && interactive {
        return Err("--resume and -i/--interactive are mutually exclusive".to_string());
    }

    Ok(DynamicArgs {
        command,
        spec,
        afk,
        interactive,
        iterations,
        no_push,
        skip_preflight,
        resume,
        output_format,
    })
}

fn run_pre_launch(root: &Path, skip_preflight: bool) {
    if skip_preflight {
        return;
    }

    if let Err(e) = springfield::recovery::pre_launch_recovery(root) {
        springfield::style::print_warning(&format!("pre-launch recovery: {e}"));
    }

    if std::env::var("SGF_SKIP_PREFLIGHT").is_err()
        && let Err(e) = springfield::recovery::ensure_daemons(root)
    {
        springfield::style::print_warning(&format!("daemon startup: {e}"));
    }

    springfield::recovery::export_pensa();
    springfield::recovery::export_forma();
}

fn resolve_command(root: &Path, name: &str) -> Result<cursus::ResolvedCursus, String> {
    if let Some(resolved) = cursus::resolve_cursus(root, name) {
        return Ok(resolved);
    }

    if let Some(resolved) = cursus::resolve_alias(root, name) {
        return Ok(resolved);
    }

    Err(format!("unknown command: {name}"))
}

fn run_simple_prompt(root: &Path, args: &DynamicArgs, prompt_path: &Path) -> ! {
    use chrono::Utc;
    use springfield::loop_mgmt::{self, IterationRecord, SessionMetadata};

    run_pre_launch(root, args.skip_preflight);

    let afk = args.afk;
    let iterations = args.iterations.unwrap_or(1);
    let auto_push = !args.no_push;

    let loop_id = loop_mgmt::generate_loop_id("simple", None);

    let log_file = loop_mgmt::create_log_file(root, &loop_id).ok();

    let agent_command = std::env::var("SGF_AGENT_COMMAND").ok();

    let mode = if afk { "afk" } else { "interactive" };
    let now = Utc::now().to_rfc3339();
    let prompt_str = prompt_path.to_string_lossy().to_string();

    let metadata = SessionMetadata {
        loop_id: loop_id.clone(),
        iterations: Vec::new(),
        stage: "simple".to_string(),
        spec: None,
        cursus: None,
        mode: mode.to_string(),
        prompt: prompt_str.clone(),
        iterations_total: iterations,
        status: "running".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    if let Err(e) = loop_mgmt::write_session_metadata(root, &metadata) {
        tracing::warn!(error = %e, "failed to write initial session metadata");
    }

    let root_for_start = root.to_path_buf();
    let loop_id_for_start = loop_id.clone();
    let on_iteration_start: springfield::iter_runner::IterationCallback =
        Box::new(move |iteration: u32, session_id: &str| {
            match loop_mgmt::read_session_metadata(&root_for_start, &loop_id_for_start) {
                Ok(Some(mut meta)) => {
                    meta.iterations.push(IterationRecord {
                        iteration,
                        session_id: session_id.to_string(),
                        completed_at: String::new(),
                    });
                    meta.updated_at = Utc::now().to_rfc3339();
                    if let Err(e) = loop_mgmt::write_session_metadata(&root_for_start, &meta) {
                        tracing::warn!(error = %e, "failed to write session_id before spawn");
                    }
                }
                Ok(None) => {
                    tracing::warn!("session metadata missing during iteration start callback");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read session metadata");
                }
            }
        });

    let root_for_cb = root.to_path_buf();
    let loop_id_for_cb = loop_id.clone();
    let on_iteration_complete: springfield::iter_runner::IterationCallback = Box::new(
        move |_iteration: u32, _session_id: &str| match loop_mgmt::read_session_metadata(
            &root_for_cb,
            &loop_id_for_cb,
        ) {
            Ok(Some(mut meta)) => {
                if let Some(last) = meta.iterations.last_mut() {
                    last.completed_at = Utc::now().to_rfc3339();
                }
                meta.updated_at = Utc::now().to_rfc3339();
                if let Err(e) = loop_mgmt::write_session_metadata(&root_for_cb, &meta) {
                    tracing::warn!(error = %e, "failed to update session metadata");
                }
            }
            Ok(None) => {
                tracing::warn!("session metadata missing during iteration callback");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to read session metadata");
            }
        },
    );

    let config = IterRunnerConfig {
        afk,
        banner: true,
        loop_id: Some(loop_id.clone()),
        iterations,
        prompt: prompt_str,
        auto_push,
        command: agent_command,
        prompt_files: vec![],
        log_file,
        session_id: Some(uuid::Uuid::new_v4().to_string()),
        resume: None,
        env_vars: vec![],
        runner_name: Some("sgf".to_string()),
        work_dir: Some(root.to_path_buf()),
        post_result_timeout: springfield::iter_runner::default_post_result_timeout(),
        inactivity_timeout: springfield::iter_runner::default_inactivity_timeout(),
        stdin_input: None,
        on_iteration_start: Some(on_iteration_start),
        on_iteration_complete: Some(on_iteration_complete),
        retry_immediate: 3,
        retry_interval_secs: 300,
        retry_max_duration_secs: 43200,
        on_retry: None,
    };

    let is_tty = args.output_format.is_none()
        && std::env::var("SGF_FORCE_TERMINAL")
            .map(|v| v == "1")
            .unwrap_or_else(|_| std::io::IsTerminal::is_terminal(&std::io::stdin()));
    let monitor_stdin = afk && is_tty;
    tracing::debug!(monitor_stdin, afk, is_tty, "simple prompt shutdown config");
    let controller = match ShutdownController::new(ShutdownConfig {
        monitor_stdin,
        ..Default::default()
    }) {
        Ok(c) => c,
        Err(e) => {
            springfield::style::print_error(&format!("shutdown init: {e}"));
            std::process::exit(1);
        }
    };

    if afk {
        springfield::style::print_action_detail(
            &format!("launching iteration runner [{loop_id}]"),
            &format!("iterations: {iterations} · mode: afk"),
        );
    } else {
        springfield::style::print_action_detail(
            "launching interactive session",
            "mode: interactive",
        );
    }

    let exit_code = springfield::iter_runner::run_iteration_loop(config, &controller);

    let status = match exit_code {
        springfield::iter_runner::IterExitCode::Complete => {
            springfield::style::print_success(&format!("loop complete [{loop_id}]"));
            "completed"
        }
        springfield::iter_runner::IterExitCode::Exhausted => {
            springfield::style::print_warning(&format!("iterations exhausted [{loop_id}]"));
            "exhausted"
        }
        springfield::iter_runner::IterExitCode::Interrupted => {
            springfield::style::print_warning(&format!("interrupted [{loop_id}]"));
            "interrupted"
        }
        springfield::iter_runner::IterExitCode::Error => {
            springfield::style::print_error(&format!("agent exited with error [{loop_id}]"));
            "interrupted"
        }
    };
    if let Ok(Some(mut meta)) = loop_mgmt::read_session_metadata(root, &loop_id) {
        meta.status = status.to_string();
        meta.updated_at = Utc::now().to_rfc3339();
        if let Err(e) = loop_mgmt::write_session_metadata(root, &meta) {
            tracing::warn!(error = %e, "failed to update session metadata on exit");
        }
    }

    eprintln!("To resume: sgf {} --resume {}", args.command, loop_id);

    std::process::exit(exit_code as i32);
}

fn resume_dispatch(root: &Path, run_id: &str) -> std::io::Result<i32> {
    if let Ok(Some(_)) = cursus::state::read_metadata(root, run_id) {
        return cursus::runner::resume_cursus(root, run_id);
    }

    match springfield::loop_mgmt::read_session_metadata(root, run_id) {
        Ok(Some(_)) => springfield::orchestrate::run_resume(root, run_id),
        Ok(None) => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("run not found: {run_id}"),
        )),
        Err(e) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Session metadata not found or corrupt for {run_id}: {e}"),
        )),
    }
}

fn run_dynamic(args: DynamicArgs) -> ! {
    let root = std::env::current_dir().expect("failed to get current directory");

    if let Some(ref run_id) = args.resume {
        match resume_dispatch(&root, run_id) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                springfield::style::print_error(&format!("resume: {e}"));
                std::process::exit(1);
            }
        }
    }

    let candidate = Path::new(&args.command);
    if candidate.exists() && candidate.is_file() {
        let prompt_path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        run_simple_prompt(&root, &args, &prompt_path);
    }

    let resolved = match resolve_command(&root, &args.command) {
        Ok(r) => r,
        Err(e) => {
            springfield::style::print_error(&e);
            std::process::exit(1);
        }
    };

    run_cursus_dispatch(&root, &args, resolved);
}

fn run_cursus_dispatch(root: &Path, args: &DynamicArgs, resolved: cursus::ResolvedCursus) -> ! {
    run_pre_launch(root, args.skip_preflight);

    let mut def = resolved.definition.clone();

    if let Err(e) = cursus::toml::validate(&def) {
        springfield::style::print_error(&format!("{}: {e}", resolved.name));
        std::process::exit(1);
    }
    if let Err(e) = cursus::toml::validate_prompts(root, &def) {
        springfield::style::print_error(&format!("{}: {e}", resolved.name));
        std::process::exit(1);
    }

    let all_defs = cursus::load_all_definitions(root);
    if let Err(e) = cursus::toml::validate_aliases(&all_defs) {
        springfield::style::print_error(&format!("alias validation: {e}"));
        std::process::exit(1);
    }

    if let Some(n) = args.iterations {
        for iter in &mut def.iters {
            iter.iterations = n;
        }
    }

    cursus::toml::clamp_iterations(&mut def);

    if args.no_push {
        def.auto_push = false;
        for iter in &mut def.iters {
            iter.auto_push = Some(false);
        }
    }

    let mode_override = if args.afk {
        Some(cursus::toml::Mode::Afk)
    } else if args.interactive {
        Some(cursus::toml::Mode::Interactive)
    } else {
        None
    };

    if let Some(ref stem) = args.spec {
        let spec_path = root.join(format!("specs/{stem}.md"));
        if !spec_path.exists() {
            springfield::style::print_error(&format!("spec not found: specs/{stem}.md"));
            std::process::exit(1);
        }
    }

    let is_tty = std::env::var("SGF_FORCE_TERMINAL")
        .map(|v| v == "1")
        .unwrap_or_else(|_| std::io::IsTerminal::is_terminal(&std::io::stdin()));
    let programmatic = args.output_format.as_deref() == Some("json") || !is_tty;

    let initial_input = if programmatic {
        let mut buf = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
        Some(buf)
    } else {
        None
    };

    let config = cursus::runner::CursusConfig {
        spec: args.spec.clone(),
        mode_override,
        no_push: args.no_push,
        agent_command: None,
        skip_preflight: args.skip_preflight,
        monitor_stdin_override: None,
        programmatic,
        initial_input,
    };

    match cursus::runner::run_cursus(root, &resolved.name, &def, &config) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            springfield::style::print_error(&format!("{}: {e}", resolved.name));
            std::process::exit(1);
        }
    }
}

struct ResumableEntry {
    run_id: String,
    label: String,
    status: String,
    updated_at: String,
}

fn collect_resumable(root: &Path) -> Vec<ResumableEntry> {
    let mut entries = Vec::new();

    if let Ok(cursus_runs) = cursus::state::find_resumable_runs(root) {
        for r in cursus_runs {
            entries.push(ResumableEntry {
                run_id: r.run_id.clone(),
                label: format!("cursus:{}", r.cursus),
                status: r.status.to_string(),
                updated_at: r.updated_at.clone(),
            });
        }
    }

    if let Ok(legacy) = springfield::loop_mgmt::find_resumable_sessions(root) {
        for m in legacy {
            entries.push(ResumableEntry {
                run_id: m.loop_id.clone(),
                label: m.stage.clone(),
                status: m.status.clone(),
                updated_at: m.updated_at.clone(),
            });
        }
    }

    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    entries.truncate(20);
    entries
}

fn run_resume_command(root: &Path, run_id: Option<&str>) -> ! {
    if let Some(id) = run_id {
        match resume_dispatch(root, id) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                springfield::style::print_error(&format!("resume: {e}"));
                std::process::exit(1);
            }
        }
    }

    let entries = collect_resumable(root);
    if entries.is_empty() {
        eprintln!("No resumable sessions");
        std::process::exit(0);
    }

    eprintln!("Resumable sessions:\n");
    for (i, e) in entries.iter().enumerate() {
        let relative = springfield::orchestrate::humanize_relative_time(&e.updated_at);
        eprintln!(
            "  {:>2}. {:<40} {:<16} {:<14} {}",
            i + 1,
            e.run_id,
            e.label,
            e.status,
            relative,
        );
    }
    eprintln!();

    eprint!("Select session (1-{}): ", entries.len());

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
        std::process::exit(0);
    }

    let choice: usize = match input.trim().parse() {
        Ok(n) if n >= 1 && n <= entries.len() => n,
        _ => {
            springfield::style::print_error("Invalid selection");
            std::process::exit(1);
        }
    };

    let selected = &entries[choice - 1];
    match resume_dispatch(root, &selected.run_id) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            springfield::style::print_error(&format!("resume: {e}"));
            std::process::exit(1);
        }
    }
}

fn run_kill(root: &Path, run_id: &str) -> ! {
    let meta = match cursus::state::read_metadata(root, run_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            springfield::style::print_error(&format!("run not found: {run_id}"));
            std::process::exit(1);
        }
        Err(e) => {
            springfield::style::print_error(&format!("failed to read metadata: {e}"));
            std::process::exit(1);
        }
    };

    if meta.status != cursus::state::RunStatus::Running {
        eprintln!("Run {run_id} is already {}, nothing to kill", meta.status);
        std::process::exit(0);
    }

    let killed = if let Some(pid) = cursus::state::read_pid(root, run_id) {
        if cursus::state::is_pid_alive(pid) {
            let ok = shutdown::kill_process_group(pid, std::time::Duration::from_secs(2));
            if ok {
                eprintln!("Killed process group (pid {pid})");
            } else {
                eprintln!("Process {pid} already exited");
            }
            true
        } else {
            eprintln!("Process {pid} already exited");
            true
        }
    } else {
        eprintln!("No PID file found, updating status only");
        false
    };

    let mut meta = if killed {
        match cursus::state::read_metadata(root, run_id) {
            Ok(Some(m)) => m,
            _ => meta,
        }
    } else {
        meta
    };

    meta.status = cursus::state::RunStatus::WaitingForInput;
    meta.touch();
    if let Err(e) = cursus::state::write_metadata(root, &meta) {
        springfield::style::print_error(&format!("failed to update metadata: {e}"));
        std::process::exit(1);
    }
    cursus::state::remove_pid_file(root, run_id);

    springfield::style::print_success(&format!("Marked {run_id} as waiting_for_input"));
    eprintln!("To resume: sgf {} --resume {run_id}", meta.cursus);
    std::process::exit(0);
}

fn run_list(root: &Path) {
    let commands = cursus::list_all(root);

    let builtins = [
        ("init", "Scaffold a new project"),
        ("kill", "Kill a running cursus and mark it resumable"),
        ("list", "Show available commands"),
        ("logs", "Tail a running loop's output"),
        ("resume", "List and resume a previous session"),
    ];

    let max_name = commands
        .iter()
        .map(|(n, _)| n.len())
        .chain(builtins.iter().map(|(n, _)| n.len()))
        .max()
        .unwrap_or(0);

    if !commands.is_empty() {
        println!("Available commands:\n");
        for (name, desc) in &commands {
            println!("  {:<width$}  {}", name, desc, width = max_name);
        }
        println!();
    }

    println!("Built-ins:\n");
    for (name, desc) in &builtins {
        println!("  {:<width$}  {}", name, desc, width = max_name);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { force, no_fe } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::init::run(&root, force, no_fe) {
                springfield::style::print_error(&format!("init: {e}"));
                std::process::exit(1);
            }
        }
        Commands::List => {
            let root = std::env::current_dir().expect("failed to get current directory");
            run_list(&root);
        }
        Commands::Kill { run_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            run_kill(&root, &run_id);
        }
        Commands::Resume { run_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            run_resume_command(&root, run_id.as_deref());
        }
        Commands::Logs { loop_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::loop_mgmt::run_logs(&root, &loop_id) {
                springfield::style::print_error(&format!("logs: {e}"));
                std::process::exit(1);
            }
        }
        Commands::Dynamic(args) => {
            let parsed = match parse_dynamic_args(args) {
                Ok(a) => a,
                Err(e) => {
                    springfield::style::print_error(&e);
                    std::process::exit(1);
                }
            };
            run_dynamic(parsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn parse_command_only() {
        let args = vec![os("build")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert!(parsed.spec.is_none());
        assert!(!parsed.afk);
        assert!(!parsed.interactive);
        assert!(parsed.iterations.is_none());
        assert!(!parsed.no_push);
        assert!(!parsed.skip_preflight);
        assert!(parsed.resume.is_none());
        assert!(parsed.output_format.is_none());
    }

    #[test]
    fn parse_command_with_spec() {
        let args = vec![os("build"), os("auth")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert_eq!(parsed.spec.as_deref(), Some("auth"));
    }

    #[test]
    fn parse_afk_flag_short() {
        let args = vec![os("build"), os("-a")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.afk);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parse_afk_flag_long() {
        let args = vec![os("build"), os("--afk")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.afk);
    }

    #[test]
    fn parse_interactive_flag_short() {
        let args = vec![os("build"), os("-i")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.interactive);
        assert!(!parsed.afk);
    }

    #[test]
    fn parse_interactive_flag_long() {
        let args = vec![os("build"), os("--interactive")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.interactive);
    }

    #[test]
    fn parse_iterations_short() {
        let args = vec![os("build"), os("-n"), os("10")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(10));
    }

    #[test]
    fn parse_iterations_long() {
        let args = vec![os("build"), os("--iterations"), os("50")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(50));
    }

    #[test]
    fn parse_no_push() {
        let args = vec![os("build"), os("--no-push")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.no_push);
    }

    #[test]
    fn parse_skip_preflight() {
        let args = vec![os("build"), os("--skip-preflight")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.skip_preflight);
    }

    #[test]
    fn parse_all_flags_with_spec() {
        let args = vec![
            os("build"),
            os("auth"),
            os("-a"),
            os("-n"),
            os("30"),
            os("--no-push"),
        ];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert_eq!(parsed.spec.as_deref(), Some("auth"));
        assert!(parsed.afk);
        assert_eq!(parsed.iterations, Some(30));
        assert!(parsed.no_push);
    }

    #[test]
    fn parse_mutual_exclusion_error() {
        let args = vec![os("build"), os("-a"), os("-i")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn parse_empty_args_error() {
        let err = parse_dynamic_args(vec![]).unwrap_err();
        assert!(err.contains("no command"));
    }

    #[test]
    fn parse_unknown_flag_error() {
        let args = vec![os("build"), os("--turbo")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("unknown flag: --turbo"));
    }

    #[test]
    fn parse_iterations_missing_value() {
        let args = vec![os("build"), os("-n")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("requires a value"));
    }

    #[test]
    fn parse_iterations_invalid_value() {
        let args = vec![os("build"), os("-n"), os("abc")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("invalid iteration count"));
    }

    #[test]
    fn parse_iterations_clamped_to_max() {
        let args = vec![os("build"), os("-n"), os("1500")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(1000));
    }

    #[test]
    fn parse_iterations_at_max_unchanged() {
        let args = vec![os("build"), os("-n"), os("1000")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(1000));
    }

    #[test]
    fn parse_iterations_below_max_unchanged() {
        let args = vec![os("build"), os("-n"), os("500")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(500));
    }

    #[test]
    fn parse_duplicate_spec_error() {
        let args = vec![os("build"), os("auth"), os("extra")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("unexpected argument"));
    }

    #[test]
    fn parse_resume_with_value() {
        let args = vec![os("build"), os("--resume"), os("build-20260422T150000")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.resume.as_deref(), Some("build-20260422T150000"));
        assert_eq!(parsed.command, "build");
        assert!(!parsed.afk);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parse_resume_missing_value() {
        let args = vec![os("build"), os("--resume")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("--resume requires a value"));
    }

    #[test]
    fn parse_resume_with_afk_error() {
        let args = vec![os("build"), os("--resume"), os("run-123"), os("-a")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("--resume and -a/--afk are mutually exclusive"));
    }

    #[test]
    fn parse_resume_with_interactive_error() {
        let args = vec![os("build"), os("--resume"), os("run-123"), os("-i")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("--resume and -i/--interactive are mutually exclusive"));
    }

    #[test]
    fn parse_output_format_json() {
        let args = vec![os("build"), os("--output-format"), os("json")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.output_format.as_deref(), Some("json"));
    }

    #[test]
    fn parse_output_format_missing_value() {
        let args = vec![os("build"), os("--output-format")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("--output-format requires a value"));
    }

    #[test]
    fn parse_output_format_unsupported_value() {
        let args = vec![os("build"), os("--output-format"), os("xml")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("unsupported output format: xml"));
    }

    #[test]
    fn parse_output_format_default_none() {
        let args = vec![os("build")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.output_format.is_none());
    }

    #[test]
    fn parse_output_format_with_other_flags() {
        let args = vec![
            os("build"),
            os("-a"),
            os("-n"),
            os("10"),
            os("--output-format"),
            os("json"),
        ];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.output_format.as_deref(), Some("json"));
        assert!(parsed.afk);
        assert_eq!(parsed.iterations, Some(10));
    }

    const SIMPLE_CURSUS: &str = r#"
description = "Build loop"
alias = "b"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
"#;

    #[test]
    fn resolve_cursus_toml_direct() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_cursus_toml_via_alias() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();

        let resolved = resolve_command(tmp.path(), "b").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_bare_prompt_without_cursus_is_unknown() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        fs::write(tmp.path().join(".sgf/prompts/deploy.md"), "prompt").unwrap();

        let err = resolve_command(tmp.path(), "deploy").unwrap_err();
        assert!(err.contains("unknown command: deploy"));
    }

    #[test]
    fn resolve_unknown_command_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

        let err = resolve_command(tmp.path(), "nonexistent").unwrap_err();
        assert!(err.contains("unknown command: nonexistent"));
    }

    #[test]
    fn resume_dispatch_not_found_exits_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/run")).unwrap();

        let result = resume_dispatch(tmp.path(), "nonexistent-run-id");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            err.to_string()
                .contains("run not found: nonexistent-run-id"),
            "error should mention run id, got: {err}"
        );
    }

    #[test]
    fn resume_dispatch_cursus_metadata_delegates_to_cursus() {
        let tmp = TempDir::new().unwrap();
        let run_id = "build-20260422T150000";
        let run_dir = tmp.path().join(".sgf/run").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        // Use "completed" status so resume_cursus rejects it with a known error
        // that proves the cursus path was entered
        let meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "build",
            "status": "completed",
            "current_iter": "build",
            "current_iter_index": 0,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "created_at": "2026-04-22T15:00:00Z",
            "updated_at": "2026-04-22T15:00:00Z"
        });
        fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let result = resume_dispatch(tmp.path(), run_id);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not resumable"),
            "should enter cursus path and reject non-resumable status, got: {err}"
        );
    }

    #[test]
    fn resume_dispatch_legacy_metadata_delegates_to_legacy() {
        let tmp = TempDir::new().unwrap();
        let run_id = "spec-20260422T130000";
        let run_dir = tmp.path().join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();
        // Empty iterations so run_resume rejects with a known error
        // that proves the legacy path was entered
        let meta = serde_json::json!({
            "loop_id": run_id,
            "iterations": [],
            "stage": "spec",
            "spec": null,
            "cursus": null,
            "mode": "interactive",
            "prompt": ".sgf/prompts/spec.md",
            "iterations_total": 1,
            "status": "completed",
            "created_at": "2026-04-22T13:00:00Z",
            "updated_at": "2026-04-22T13:02:30Z"
        });
        fs::write(
            run_dir.join(format!("{run_id}.json")),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let result = resume_dispatch(tmp.path(), run_id);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("No iterations found"),
            "should enter legacy path and find no iterations, got: {err}"
        );
    }

    #[test]
    fn resume_dispatch_cursus_takes_priority_over_legacy() {
        let tmp = TempDir::new().unwrap();
        let run_id = "build-20260422T150000";

        // Create BOTH cursus and legacy metadata for the same run_id
        let cursus_dir = tmp.path().join(".sgf/run").join(run_id);
        fs::create_dir_all(&cursus_dir).unwrap();
        // Use "completed" status so cursus path fails with known error
        let cursus_meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "build",
            "status": "completed",
            "current_iter": "build",
            "current_iter_index": 0,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "created_at": "2026-04-22T15:00:00Z",
            "updated_at": "2026-04-22T15:00:00Z"
        });
        fs::write(
            cursus_dir.join("meta.json"),
            serde_json::to_string_pretty(&cursus_meta).unwrap(),
        )
        .unwrap();

        let legacy_dir = tmp.path().join(".sgf/run");
        let legacy_meta = serde_json::json!({
            "loop_id": run_id,
            "iterations": [],
            "stage": "build",
            "spec": null,
            "cursus": null,
            "mode": "afk",
            "prompt": ".sgf/prompts/build.md",
            "iterations_total": 1,
            "status": "completed",
            "created_at": "2026-04-22T15:00:00Z",
            "updated_at": "2026-04-22T15:02:30Z"
        });
        fs::write(
            legacy_dir.join(format!("{run_id}.json")),
            serde_json::to_string_pretty(&legacy_meta).unwrap(),
        )
        .unwrap();

        // Should attempt cursus resume (not legacy)
        let result = resume_dispatch(tmp.path(), run_id);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Cursus path error (not resumable), NOT legacy path error (No iterations)
        assert!(
            err.to_string().contains("not resumable"),
            "should prefer cursus path when both exist, got: {err}"
        );
    }

    #[test]
    fn resume_dispatch_corrupt_legacy_metadata_returns_error() {
        let tmp = TempDir::new().unwrap();
        let run_id = "broken-20260422T150000";
        let run_dir = tmp.path().join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(run_dir.join(format!("{run_id}.json")), "not valid json").unwrap();

        let result = resume_dispatch(tmp.path(), run_id);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string()
                .contains("Session metadata not found or corrupt"),
            "should report corruption, got: {err}"
        );
    }

    #[test]
    fn resolve_prefers_cursus_direct_over_cursus_alias() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();
        fs::write(
            tmp.path().join(".sgf/cursus/test.toml"),
            r#"
description = "Test"
alias = "build"

[[iter]]
name = "test"
prompt = "test.md"
"#,
        )
        .unwrap();

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn collect_resumable_empty_when_no_sessions() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/run")).unwrap();
        let entries = collect_resumable(tmp.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_resumable_includes_legacy_interrupted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        let meta = serde_json::json!({
            "loop_id": "simple-20260430T120000",
            "iterations": [{"iteration": 1, "session_id": "sid-1", "completed_at": "2026-04-30T12:01:00Z"}],
            "stage": "build",
            "spec": null,
            "cursus": null,
            "mode": "interactive",
            "prompt": ".sgf/prompts/build.md",
            "iterations_total": 1,
            "status": "interrupted",
            "created_at": "2026-04-30T12:00:00Z",
            "updated_at": "2026-04-30T12:01:00Z"
        });
        fs::write(
            run_dir.join("simple-20260430T120000.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let entries = collect_resumable(root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run_id, "simple-20260430T120000");
        assert_eq!(entries[0].label, "build");
        assert_eq!(entries[0].status, "interrupted");
    }

    #[test]
    fn collect_resumable_includes_cursus_stalled() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "change-20260430T130000";
        let run_dir = root.join(".sgf/run").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();

        let meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "change",
            "status": "stalled",
            "current_iter": "build",
            "current_iter_index": 1,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "created_at": "2026-04-30T13:00:00Z",
            "updated_at": "2026-04-30T13:05:00Z"
        });
        fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let entries = collect_resumable(root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].run_id, run_id);
        assert_eq!(entries[0].label, "cursus:change");
        assert_eq!(entries[0].status, "stalled");
    }

    #[test]
    fn collect_resumable_merges_and_sorts_by_updated_at() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        // Legacy session (older)
        let legacy = serde_json::json!({
            "loop_id": "simple-20260430T100000",
            "iterations": [{"iteration": 1, "session_id": "sid-1", "completed_at": "2026-04-30T10:01:00Z"}],
            "stage": "build",
            "spec": null,
            "cursus": null,
            "mode": "afk",
            "prompt": ".sgf/prompts/build.md",
            "iterations_total": 1,
            "status": "exhausted",
            "created_at": "2026-04-30T10:00:00Z",
            "updated_at": "2026-04-30T10:01:00Z"
        });
        fs::write(
            run_dir.join("simple-20260430T100000.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        // Cursus run (newer)
        let cursus_id = "spec-20260430T140000";
        let cursus_dir = run_dir.join(cursus_id);
        fs::create_dir_all(&cursus_dir).unwrap();
        let cursus_meta = serde_json::json!({
            "run_id": cursus_id,
            "cursus": "spec",
            "status": "interrupted",
            "current_iter": "draft",
            "current_iter_index": 0,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "created_at": "2026-04-30T14:00:00Z",
            "updated_at": "2026-04-30T14:05:00Z"
        });
        fs::write(
            cursus_dir.join("meta.json"),
            serde_json::to_string_pretty(&cursus_meta).unwrap(),
        )
        .unwrap();

        let entries = collect_resumable(root);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].run_id, cursus_id, "newer should be first");
        assert_eq!(entries[1].run_id, "simple-20260430T100000");
    }

    #[test]
    fn collect_resumable_excludes_completed_includes_crashed() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        let completed = serde_json::json!({
            "loop_id": "done-loop",
            "iterations": [{"iteration": 1, "session_id": "s1", "completed_at": "2026-04-30T12:00:00Z"}],
            "stage": "build",
            "spec": null,
            "cursus": null,
            "mode": "interactive",
            "prompt": ".sgf/prompts/build.md",
            "iterations_total": 1,
            "status": "completed",
            "created_at": "2026-04-30T12:00:00Z",
            "updated_at": "2026-04-30T12:00:00Z"
        });
        fs::write(
            run_dir.join("done-loop.json"),
            serde_json::to_string_pretty(&completed).unwrap(),
        )
        .unwrap();

        let running = serde_json::json!({
            "loop_id": "active-loop",
            "iterations": [{"iteration": 1, "session_id": "s-crash", "completed_at": ""}],
            "stage": "build",
            "spec": null,
            "cursus": null,
            "mode": "afk",
            "prompt": ".sgf/prompts/build.md",
            "iterations_total": 5,
            "status": "running",
            "created_at": "2026-04-30T12:00:00Z",
            "updated_at": "2026-04-30T12:00:00Z"
        });
        fs::write(
            run_dir.join("active-loop.json"),
            serde_json::to_string_pretty(&running).unwrap(),
        )
        .unwrap();

        let entries = collect_resumable(root);
        assert_eq!(
            entries.len(),
            1,
            "crashed running session should be resumable"
        );
        assert_eq!(entries[0].run_id, "active-loop");
        assert_eq!(entries[0].status, "crashed");
    }

    #[test]
    fn collect_resumable_truncates_to_20() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        for i in 0..25u32 {
            let meta = serde_json::json!({
                "loop_id": format!("loop-{i:02}"),
                "iterations": [{"iteration": 1, "session_id": format!("sid-{i}"), "completed_at": "2026-04-30T12:00:00Z"}],
                "stage": "build",
                "spec": null,
                "cursus": null,
                "mode": "afk",
                "prompt": ".sgf/prompts/build.md",
                "iterations_total": 1,
                "status": "interrupted",
                "created_at": "2026-04-30T12:00:00Z",
                "updated_at": format!("2026-04-{:02}T12:00:00Z", (i % 28) + 1)
            });
            fs::write(
                run_dir.join(format!("loop-{i:02}.json")),
                serde_json::to_string_pretty(&meta).unwrap(),
            )
            .unwrap();
        }

        let entries = collect_resumable(root);
        assert_eq!(entries.len(), 20);
    }

    fn write_cursus_meta(root: &Path, run_id: &str, status: &str) {
        let run_dir = root.join(".sgf/run").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        let meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "build",
            "status": status,
            "current_iter": "build",
            "current_iter_index": 0,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "created_at": "2026-05-07T10:00:00Z",
            "updated_at": "2026-05-07T10:00:00Z"
        });
        fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
    }

    fn write_pid(root: &Path, run_id: &str, pid: u32) {
        let pid_path = root
            .join(".sgf/run")
            .join(run_id)
            .join(format!("{run_id}.pid"));
        fs::write(pid_path, pid.to_string()).unwrap();
    }

    #[test]
    fn kill_not_found_exits_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/run")).unwrap();

        let result = std::panic::catch_unwind(|| {
            // run_kill calls process::exit, so we test the underlying logic directly
        });
        let _ = result;

        // Test the metadata lookup path directly
        let meta = cursus::state::read_metadata(tmp.path(), "nonexistent").unwrap();
        assert!(meta.is_none());
    }

    #[test]
    fn kill_already_completed_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_cursus_meta(root, "build-20260507T100000", "completed");

        let meta = cursus::state::read_metadata(root, "build-20260507T100000")
            .unwrap()
            .unwrap();
        assert_ne!(meta.status, cursus::state::RunStatus::Running);
    }

    #[test]
    fn kill_already_waiting_for_input_is_noop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_cursus_meta(root, "build-20260507T100000", "waiting_for_input");

        let meta = cursus::state::read_metadata(root, "build-20260507T100000")
            .unwrap()
            .unwrap();
        assert_ne!(meta.status, cursus::state::RunStatus::Running);
    }

    #[test]
    fn kill_running_with_dead_pid_updates_status() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260507T100000";
        write_cursus_meta(root, run_id, "running");
        write_pid(root, run_id, 4000000); // very unlikely to be alive

        let meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(meta.status, cursus::state::RunStatus::Running);

        // Simulate what run_kill does for a dead process
        let pid = cursus::state::read_pid(root, run_id).unwrap();
        assert!(!cursus::state::is_pid_alive(pid));

        let mut meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        meta.status = cursus::state::RunStatus::WaitingForInput;
        meta.touch();
        cursus::state::write_metadata(root, &meta).unwrap();
        cursus::state::remove_pid_file(root, run_id);

        let updated = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(updated.status, cursus::state::RunStatus::WaitingForInput);
        assert!(cursus::state::read_pid(root, run_id).is_none());
    }

    #[test]
    fn kill_running_no_pid_file_updates_status() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260507T100000";
        write_cursus_meta(root, run_id, "running");
        // No PID file written

        assert!(cursus::state::read_pid(root, run_id).is_none());

        let mut meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        meta.status = cursus::state::RunStatus::WaitingForInput;
        meta.touch();
        cursus::state::write_metadata(root, &meta).unwrap();

        let updated = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(updated.status, cursus::state::RunStatus::WaitingForInput);
    }

    #[test]
    fn kill_running_with_alive_pid_kills_and_updates() {
        use std::os::unix::process::CommandExt;

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260507T100000";
        write_cursus_meta(root, run_id, "running");

        // Spawn a child in its own process group so kill_process_group works
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("60");
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap();
        let pid = child.id();
        write_pid(root, run_id, pid);

        assert!(cursus::state::is_pid_alive(pid));

        let killed = shutdown::kill_process_group(pid, std::time::Duration::from_secs(2));
        assert!(killed);

        let mut meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        meta.status = cursus::state::RunStatus::WaitingForInput;
        meta.touch();
        cursus::state::write_metadata(root, &meta).unwrap();
        cursus::state::remove_pid_file(root, run_id);

        let updated = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(updated.status, cursus::state::RunStatus::WaitingForInput);
        assert!(cursus::state::read_pid(root, run_id).is_none());
    }

    #[test]
    fn kill_preserves_existing_session_id() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260507T100000";
        let run_dir = root.join(".sgf/run").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();

        let meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "build",
            "status": "running",
            "current_iter": "build",
            "current_iter_index": 0,
            "iters_completed": [],
            "spec": null,
            "mode_override": null,
            "context_producers": {},
            "current_session_id": "sess-abc123",
            "created_at": "2026-05-07T10:00:00Z",
            "updated_at": "2026-05-07T10:00:00Z"
        });
        fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let mut meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        meta.status = cursus::state::RunStatus::WaitingForInput;
        meta.touch();
        cursus::state::write_metadata(root, &meta).unwrap();

        let updated = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(updated.status, cursus::state::RunStatus::WaitingForInput);
        assert_eq!(updated.current_session_id.as_deref(), Some("sess-abc123"));
    }

    #[test]
    fn kill_preserves_iter_position() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "build-20260507T100000";
        let run_dir = root.join(".sgf/run").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();

        let meta = serde_json::json!({
            "run_id": run_id,
            "cursus": "build",
            "status": "running",
            "current_iter": "test",
            "current_iter_index": 2,
            "iters_completed": [
                {"name": "plan", "session_id": "s1", "completed_at": "2026-05-07T10:01:00Z", "outcome": "complete"},
                {"name": "build", "session_id": "s2", "completed_at": "2026-05-07T10:05:00Z", "outcome": "complete"}
            ],
            "spec": "auth",
            "mode_override": null,
            "context_producers": {"plan-summary": "plan"},
            "created_at": "2026-05-07T10:00:00Z",
            "updated_at": "2026-05-07T10:05:00Z"
        });
        fs::write(
            run_dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let mut meta = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        meta.status = cursus::state::RunStatus::WaitingForInput;
        meta.touch();
        cursus::state::write_metadata(root, &meta).unwrap();

        let updated = cursus::state::read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(updated.status, cursus::state::RunStatus::WaitingForInput);
        assert_eq!(updated.current_iter, "test");
        assert_eq!(updated.current_iter_index, 2);
        assert_eq!(updated.iters_completed.len(), 2);
        assert_eq!(updated.spec.as_deref(), Some("auth"));
        assert_eq!(updated.context_producers.len(), 1);
    }
}
