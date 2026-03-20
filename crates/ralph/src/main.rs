pub(crate) mod banner;
pub(crate) mod format;
pub(crate) mod style;

use clap::Parser;
use shutdown::{ShutdownConfig, ShutdownController, ShutdownStatus, kill_process_group};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};

struct TeeWriter {
    log_file: Option<Mutex<fs::File>>,
}

impl TeeWriter {
    fn new(path: Option<&Path>) -> std::io::Result<Self> {
        let log_file = match path {
            Some(p) => {
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent)?;
                }
                Some(Mutex::new(
                    fs::OpenOptions::new().create(true).append(true).open(p)?,
                ))
            }
            None => None,
        };
        Ok(TeeWriter { log_file })
    }

    fn writeln(&self, line: &str) {
        println!("{line}");
        if let Some(ref f) = self.log_file
            && let Ok(mut f) = f.lock()
        {
            let _ = writeln!(f, "{}", style::strip_ansi(line));
        }
    }

    fn write_ansi_line(&self, line: &str) {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let _ = write!(lock, "\r\x1b[2K{line}\n");
        let _ = lock.flush();
        if let Some(ref f) = self.log_file
            && let Ok(mut f) = f.lock()
        {
            let _ = writeln!(f, "{}", style::strip_ansi(line));
        }
    }
}

const SENTINEL: &str = ".ralph-complete";
const SENTINEL_MAX_DEPTH: usize = 2;
const DING_SENTINEL: &str = ".ralph-ding";

fn find_sentinel(dir: &Path, max_depth: usize) -> Option<PathBuf> {
    let candidate = dir.join(SENTINEL);
    if candidate.exists() {
        return Some(candidate);
    }
    if max_depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().ok().is_some_and(|ft| ft.is_dir())
            && let Some(found) = find_sentinel(&entry.path(), max_depth - 1)
        {
            return Some(found);
        }
    }
    None
}

fn remove_sentinel() {
    if let Some(path) = find_sentinel(Path::new("."), SENTINEL_MAX_DEPTH) {
        let _ = fs::remove_file(path);
    }
}

/// Iterative Claude Code runner via direct `cl` invocation.
///
/// Runs Claude Code repeatedly against a prompt file, formatting NDJSON
/// stream output for readable AFK execution.
#[derive(Parser)]
#[command(name = "ralph")]
struct Cli {
    /// Run in AFK mode (non-interactive, formatted NDJSON stream)
    #[arg(short = 'a', long)]
    afk: bool,

    /// Display ASCII art startup banner
    #[arg(long)]
    banner: bool,

    /// Loop identifier (sgf-generated, included in banner output)
    #[arg(long)]
    loop_id: Option<String>,

    /// Number of iterations to run
    #[arg(default_value_t = 1)]
    iterations: u32,

    /// Prompt file path or inline text string
    #[arg(default_value = "prompt.md")]
    prompt: String,

    /// Auto-push after new commits
    #[arg(long, env = "RALPH_AUTO_PUSH", default_value = "true", value_parser = parse_bool, num_args = 1)]
    auto_push: bool,

    /// Override: path to executable replacing agent invocation (for testing)
    #[arg(long, env = "RALPH_COMMAND")]
    command: Option<String>,

    /// Additional prompt file path (repeatable)
    #[arg(long = "prompt-file")]
    prompt_file: Vec<String>,

    /// Path to log file — ralph tees its output here
    #[arg(long)]
    log_file: Option<PathBuf>,

    /// Pre-assigned Claude session ID (UUID), passed through to cl as --session-id
    #[arg(long, conflicts_with = "resume")]
    session_id: Option<String>,

    /// Resume a previous Claude session by session ID, passed through to cl as --resume
    #[arg(long, conflicts_with = "session_id")]
    resume: Option<String>,
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(format!(
            "invalid boolean value '{other}': expected true/false, 1/0, or yes/no"
        )),
    }
}

fn resolve_agent_cmd(cli: &Cli) -> String {
    if let Some(ref cmd) = cli.command {
        return cmd.clone();
    }
    "cl".to_string()
}

fn check_agent_in_path(agent_cmd: &str) {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(agent_cmd).is_file() {
                return;
            }
        }
    }
    error!("cl not found in PATH");
    std::process::exit(1);
}

fn build_append_system_prompt_args(prompt_files: &[String]) -> Vec<String> {
    let mut parts = Vec::new();

    for f in prompt_files {
        if let Ok(content) = std::fs::read_to_string(f) {
            parts.push(content);
        } else {
            warn!(path = %f, "failed to read prompt file for system prompt injection");
        }
    }

    if parts.is_empty() {
        return Vec::new();
    }

    vec!["--append-system-prompt".to_string(), parts.join("\n")]
}

fn collect_prompt_files(cli: &Cli) -> Vec<String> {
    let mut files = Vec::new();

    for path in &cli.prompt_file {
        if !Path::new(path).exists() {
            error!(path, "prompt file not found");
            std::process::exit(1);
        }
        files.push(path.clone());
    }

    files
}

fn save_terminal_settings() -> Option<libc::termios> {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
            Some(termios)
        } else {
            None
        }
    }
}

fn restore_terminal_settings(termios: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, termios);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let tee = match TeeWriter::new(cli.log_file.as_deref()) {
        Ok(t) => Arc::new(t),
        Err(e) => {
            error!(error = %e, "failed to open log file");
            std::process::exit(1);
        }
    };

    let agent_cmd = resolve_agent_cmd(&cli);

    if cli.command.is_none() {
        check_agent_in_path(&agent_cmd);
    }

    let config = ShutdownConfig {
        monitor_stdin: std::env::var("SGF_MANAGED").is_err(),
        ..Default::default()
    };
    let controller = ShutdownController::new(config).expect("Failed to create ShutdownController");

    let is_default_prompt = cli.prompt == "prompt.md";
    let is_file = Path::new(&cli.prompt).exists();

    if cli.resume.is_none() && is_default_prompt && !is_file {
        error!(prompt = %cli.prompt, "prompt file not found");
        std::process::exit(1);
    }

    let prompt_files = collect_prompt_files(&cli);

    const MAX_ITERATIONS: u32 = 1000;
    let iterations = if cli.iterations > MAX_ITERATIONS {
        warn!(
            requested = cli.iterations,
            max = MAX_ITERATIONS,
            "clamping iterations to hard limit"
        );
        MAX_ITERATIONS
    } else {
        cli.iterations
    };

    if cli.banner {
        print_banner(&cli, iterations, is_file, &prompt_files, &agent_cmd, &tee);
    }

    remove_sentinel();
    let _ = fs::remove_file(DING_SENTINEL);

    let saved_termios = save_terminal_settings();

    for i in 1..=iterations {
        remove_sentinel();

        let iter_title = if let Some(ref id) = cli.loop_id {
            format!("Iteration {} of {} [{}]", i, iterations, id)
        } else {
            format!("Iteration {} of {}", i, iterations)
        };
        tee.writeln("");
        for line in banner::render_box(&iter_title, &[]).split('\n') {
            tee.writeln(line);
        }
        tee.writeln("");

        let head_before = vcs_utils::git_head();

        if cli.afk {
            run_afk(
                &agent_cmd,
                &cli,
                is_file,
                &prompt_files,
                &controller,
                &tee,
                i,
            );
        } else {
            run_interactive(&agent_cmd, &cli, is_file, &prompt_files, &controller, i);
        }

        if let Some(ref termios) = saved_termios {
            restore_terminal_settings(termios);
        }

        if controller.poll() == ShutdownStatus::Shutdown {
            warn!("interrupted");
            std::process::exit(130);
        }

        if let Some(sentinel_path) = find_sentinel(Path::new("."), SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(sentinel_path);
            let complete_title = format!("Ralph COMPLETE after {} iterations!", i);
            tee.writeln("");
            for line in
                banner::render_box_styled(&complete_title, &[], |s| style::bold(&style::green(s)))
                    .split('\n')
            {
                tee.writeln(line);
            }
            auto_push_if_changed(&cli, &head_before, &tee);
            std::process::exit(0);
        }

        log_resource_usage(i);

        tee.writeln("");
        tee.writeln(&style::dim(&format!(
            "Iteration {} complete, continuing...",
            i
        )));

        for _ in 0..20 {
            if controller.poll() == ShutdownStatus::Shutdown {
                warn!("interrupted");
                std::process::exit(130);
            }
            thread::sleep(Duration::from_millis(100));
        }

        auto_push_if_changed(&cli, &head_before, &tee);
    }

    remove_sentinel();
    let max_title = format!("Ralph reached max iterations ({})", iterations);
    tee.writeln("");
    for line in
        banner::render_box_styled(&max_title, &[], |s| style::bold(&style::yellow(s))).split('\n')
    {
        tee.writeln(line);
    }
    std::process::exit(2);
}

fn print_banner(
    cli: &Cli,
    iterations: u32,
    is_file: bool,
    prompt_files: &[String],
    agent_cmd: &str,
    tee: &TeeWriter,
) {
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣤⡴⣶⠖⡲⠒⡶⠒⣖⢲⡤⣄⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⢴⡾⣻⠟⢉⡞⢁⡞⠁⢠⠇⠀⠸⡄⠳⡈⢫⡙⢦⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⢀⡴⢚⡵⢋⡜⠁⢠⡎⠀⡞⠀⠀⢸⠀⠀⠀⡇⠀⢹⡀⠹⡌⢳⡙⣦⡀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠐⠋⠀⡞⠀⣸⠔⠒⠲⣄⠀⠀⠀⢀⡔⠋⠉⠙⠲⡀⠀⢷⠀⢹⡀⢱⡘⣟⣆⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⢰⠃⢸⠁⠀⣤⠄⠈⡇⠀⠀⢸⠀⠀⠾⠆⠀⡇⠀⠈⠀⠀⣇⠀⢧⢸⡘⡆⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠘⢆⡀⠀⣠⠴⢧⠀⠀⠈⠳⣄⣀⣠⠜⠁⠀⠀⠀⠀⠀⠀⠸⠄⡇⡇⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⢀⡼⠀⠀⠀⠉⠉⣇⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣤⠖⠲⣇⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⣏⠀⠀⠀⠀⠀⠀⠀⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⣀⣤⡄⠀⠀⠀⠀⢿⠓⣹⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠈⠓⠦⠤⠭⢿⡒⠒⠒⠒⠒⠒⠒⠒⠒⠊⠉⠉⠁⠀⠁⠀⠀⠀⠦⡤⠖⠃⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣾⡁⠀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⢴⡶⡇⠀⠀⠀⠀⠀⠀⣀⣀⣀⡤⠤⠤⠖⠚⠉⠁⠀⢧⠀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⢾⠏⠈⠳⣇⠀⠀⠀⠀⣠⠞⠁⠲⣄⠀⠀⠀⠀⠀⠀⢀⣠⡾⢤⡀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠀⣠⡾⠃⢸⡴⠚⠁⠈⠳⢤⡠⠞⠁⠀⠀⠀⠈⢦⢀⣀⡤⠖⠛⢩⠋⠀⠀⠈⢣⡀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⢀⣾⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠉⠀⠀⠀⠀⡞⠀⠀⠀⠀⠀⠹⡄⠀⠀");
    tee.writeln("⠀⠀⠀⠀⢠⢏⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠹⡀⠀");
    tee.writeln("⠀⠀⠀⣰⠃⡼⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣷⠀⠀⠀⠀⠀⠀⠀⢳⠀");
    tee.writeln("⠀⠀⢠⠇⢰⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀⠘⡆");
    tee.writeln("⠀⠀⡏⠀⢸⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀⠀⣇");
    tee.writeln("⠀⢰⢡⠀⢸⠀⠀⠀⠀⠀⢀⣀⣤⣤⣤⣤⣤⣤⣀⣠⣤⣤⣄⣀⣀⣀⣀⣀⣀⡀⠈⡇⠀⠀⠀⠀⠀⠀⠀⣿");
    tee.writeln("⠀⢸⢀⣀⣸⠞⠋⠉⠉⢉⣹⣿⣿⣿⣿⣿⣿⣿⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⡀⠉⠉⡗⠒⠒⠢⠤⣄⡀⠀⡿");
    tee.writeln("⠀⠘⢿⠁⢸⡴⠖⠛⠉⠉⠙⠛⠛⠛⠋⠉⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⣽⠟⠁⠀⠀⠀⠀⠀⠙⡖⠃");
    tee.writeln("⠀⠀⠘⣆⢣⣳⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠧⣤⠴⡄⠀⢀⠀⠀⢠⠃⠀");
    tee.writeln("⠀⠀⠀⠈⠢⣝⣻⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡼⠃⢀⡞⢠⠆⡞⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⠈⣯⠳⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠳⢶⣏⡴⠯⠞⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⢸⣿⠀⠀⠙⠶⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⢸⡏⠀⠀⠀⠀⠀⠉⠉⠛⠒⢲⠖⠚⠋⢹⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⣼⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⢸⡆⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⡆⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⠀⡏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠸⡆⠀⠀⢸⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⡇⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⠀⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢷⠀⠀⠀⠀⠀⠀");
    tee.writeln("⠀⠀⣠⡴⠒⠛⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠙⠛⣻⠖⠚⠉⠉⠉⠉⠉⠉⠉⠉⠉⠛⠛⠛⠛⢦⡀⠀⠀⠀⠀");
    tee.writeln("⢠⡾⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡞⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀");
    let mut body = vec![
        format!(
            "Mode:        {}",
            if cli.afk { "AFK" } else { "Interactive" }
        ),
        if is_file {
            format!("Prompt:      {} (file)", cli.prompt)
        } else {
            let display = format::truncate(&cli.prompt, 60);
            format!("Prompt:      {} (text)", display)
        },
        format!("Iterations:  {}", iterations),
        format!("Agent:       {}", agent_cmd),
    ];
    if let Some(ref id) = cli.loop_id {
        body.push(format!("Loop ID:     {}", id));
    }
    if !prompt_files.is_empty() {
        body.push("Prompt files:".to_string());
        for f in prompt_files {
            body.push(format!("  - {}", f));
        }
    }
    for line in banner::render_box("Ralph Loop Starting", &body).split('\n') {
        tee.writeln(line);
    }
    tee.writeln("");
}

fn ding_watcher(stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        if Path::new(DING_SENTINEL).exists() {
            let _ = fs::remove_file(DING_SENTINEL);
            let _ = Command::new("afplay")
                .arg("/System/Library/Sounds/Blow.aiff")
                .spawn();
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_interactive(
    agent_cmd: &str,
    cli: &Cli,
    is_file: bool,
    prompt_files: &[String],
    controller: &ShutdownController,
    iteration: u32,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let watcher = thread::spawn(move || ding_watcher(&stop_clone));

    let asp_args = build_append_system_prompt_args(prompt_files);

    let mut command = Command::new(agent_cmd);
    command.args([
        "--verbose",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);
    if iteration == 1 {
        if let Some(ref sid) = cli.session_id {
            command.args(["--session-id", sid]);
        }
    } else {
        let fresh_id = uuid::Uuid::new_v4().to_string();
        command.args(["--session-id", &fresh_id]);
    }
    command.args(&asp_args);
    let resuming = iteration == 1 && cli.resume.is_some();
    if resuming {
        command.args(["--resume", cli.resume.as_ref().unwrap()]);
    } else {
        let prompt_arg = if is_file {
            format!("@{}", cli.prompt)
        } else {
            cli.prompt.clone()
        };
        command.arg(&prompt_arg);
    }
    let mut child = match command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to spawn command");
            stop.store(true, Ordering::Relaxed);
            let _ = watcher.join();
            return;
        }
    };

    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            kill_process_group(child.id(), Duration::from_millis(200));
            let _ = child.wait();
            break;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    warn!(
                        status = status.code().unwrap_or(-1),
                        "command exited with non-zero status"
                    );
                }
                break;
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                warn!(error = %e, "error waiting for child process");
                break;
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = watcher.join();
}

fn run_afk(
    agent_cmd: &str,
    cli: &Cli,
    is_file: bool,
    prompt_files: &[String],
    controller: &ShutdownController,
    tee: &TeeWriter,
    iteration: u32,
) {
    let skip_setsid = std::env::var("SGF_TEST_NO_SETSID").is_ok();
    let setsid_hook = move || unsafe {
        if !skip_setsid {
            libc::setsid();
        }
        Ok(())
    };

    let asp_args = build_append_system_prompt_args(prompt_files);

    let mut cmd = Command::new(agent_cmd);
    cmd.args([
        "--verbose",
        "--print",
        "--output-format",
        "stream-json",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);
    if iteration == 1 {
        if let Some(ref sid) = cli.session_id {
            cmd.args(["--session-id", sid]);
        }
    } else {
        let fresh_id = uuid::Uuid::new_v4().to_string();
        cmd.args(["--session-id", &fresh_id]);
    }
    cmd.args(&asp_args);
    let resuming = iteration == 1 && cli.resume.is_some();
    if resuming {
        cmd.args(["--resume", cli.resume.as_ref().unwrap()]);
    } else {
        let prompt_arg = if is_file {
            format!("@{}", cli.prompt)
        } else {
            cli.prompt.clone()
        };
        cmd.arg(&prompt_arg);
    }
    let child = unsafe {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .pre_exec(setsid_hook)
            .spawn()
    };

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to spawn command");
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            warn!("failed to capture stdout");
            return;
        }
    };

    let reader = BufReader::new(stdout);
    let (tx, rx) = mpsc::channel();

    let reader_handle = thread::spawn(move || {
        for line in reader.lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let child_pid = child.id();

    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            kill_process_group(child_pid, Duration::from_millis(200));
            let _ = child.wait();
            let _ = reader_handle.join();
            return;
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => match format::format_line(&line) {
                format::FormattedOutput::Text(text) => {
                    tee.write_ansi_line("");
                    for l in text.split('\n') {
                        tee.write_ansi_line(&style::white(&style::bold(l)));
                    }
                    tee.write_ansi_line("");
                }
                format::FormattedOutput::ToolCalls(calls) => {
                    for call in &calls {
                        tee.write_ansi_line(&format!(
                            "  {} {}  {}",
                            style::dim("─"),
                            style::tool_name_style(&call.name),
                            style::white(&call.detail),
                        ));
                    }
                }
                format::FormattedOutput::ToolResults(_) => {}
                format::FormattedOutput::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    tee.write_ansi_line(&style::dim(&format!(
                        "  Input: {input_tokens} tokens · Output: {output_tokens} tokens"
                    )));
                }
                format::FormattedOutput::Result(text) => {
                    tee.write_ansi_line("");
                    for l in text.split('\n') {
                        tee.write_ansi_line(l);
                    }
                    tee.write_ansi_line("");
                }
                format::FormattedOutput::Skip => {}
            },
            Ok(Err(e)) => {
                warn!(error = %e, "error reading stdout");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    kill_process_group(child_pid, Duration::from_millis(200));
    if let Err(e) = child.wait() {
        warn!(error = %e, "error waiting for child process");
    }
    let _ = reader_handle.join();
}

fn log_resource_usage(iteration: u32) {
    let pid = std::process::id();
    let open_fds = fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0);

    let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
    let fd_limit = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) } == 0 {
        rlim.rlim_cur
    } else {
        0
    };

    info!(
        iteration,
        pid, open_fds, fd_limit, "resource usage after iteration"
    );
}

fn auto_push_if_changed(cli: &Cli, head_before: &Option<String>, tee: &TeeWriter) {
    if !cli.auto_push {
        return;
    }

    if let Some(before) = head_before {
        vcs_utils::auto_push_if_changed(before, |msg| tee.writeln(&style::dim(msg)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_sentinel_at_root() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), 2).is_some());
    }

    #[test]
    fn find_sentinel_nested() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), 2).is_some());
    }

    #[test]
    fn save_terminal_settings_returns_some_on_tty() {
        let result = save_terminal_settings();
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
            assert!(result.is_some());
        } else {
            assert!(result.is_none());
        }
    }

    #[test]
    fn restore_terminal_settings_is_idempotent() {
        if let Some(termios) = save_terminal_settings() {
            restore_terminal_settings(&termios);
            restore_terminal_settings(&termios);
            let after = save_terminal_settings();
            assert!(after.is_some());
        }
    }

    #[test]
    fn find_sentinel_too_deep() {
        let dir = tempfile::TempDir::new().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), 2).is_none());
    }

    #[test]
    fn build_append_system_prompt_args_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("NOTES.md");
        std::fs::write(&file, "These are notes.").unwrap();
        let files = vec![file.to_string_lossy().to_string()];
        let args = build_append_system_prompt_args(&files);
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "--append-system-prompt");
        assert_eq!(args[1], "These are notes.");
    }

    #[test]
    fn build_append_system_prompt_args_empty() {
        let args = build_append_system_prompt_args(&[]);
        assert!(args.is_empty());
    }

    #[test]
    fn cli_session_id_flag_parses() {
        let cli = Cli::parse_from([
            "ralph",
            "--session-id",
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "1",
            "prompt.md",
        ]);
        assert_eq!(
            cli.session_id.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert!(cli.resume.is_none());
    }

    #[test]
    fn cli_resume_flag_parses() {
        let cli = Cli::parse_from([
            "ralph",
            "--resume",
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "1",
        ]);
        assert_eq!(
            cli.resume.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert!(cli.session_id.is_none());
    }

    #[test]
    fn cli_session_id_and_resume_conflict() {
        let result = Cli::try_parse_from(["ralph", "--session-id", "id1", "--resume", "id2", "1"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_banner_flag_defaults_false() {
        let cli = Cli::parse_from(["ralph", "1", "prompt.md"]);
        assert!(!cli.banner);
    }

    #[test]
    fn cli_banner_flag_parses() {
        let cli = Cli::parse_from(["ralph", "--banner", "1", "prompt.md"]);
        assert!(cli.banner);
    }
}
