pub(crate) mod banner;
pub(crate) mod format;
pub(crate) mod style;

use clap::Parser;
use shutdown::{ShutdownConfig, ShutdownController, ShutdownStatus};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use tracing::{error, warn};

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

/// Iterative Claude Code runner via direct $AGENT_CMD invocation.
///
/// Runs Claude Code repeatedly against a prompt file, formatting NDJSON
/// stream output for readable AFK execution.
#[derive(Parser)]
#[command(name = "ralph")]
struct Cli {
    /// Run in AFK mode (non-interactive, formatted NDJSON stream)
    #[arg(short = 'a', long)]
    afk: bool,

    /// Loop identifier (sgf-generated, included in banner output)
    #[arg(long)]
    loop_id: Option<String>,

    /// Number of iterations to run
    #[arg(default_value_t = 1)]
    iterations: u32,

    /// Prompt file path or inline text string
    #[arg(default_value = "prompt.md")]
    prompt: String,

    /// Safety limit for iterations
    #[arg(long, env = "RALPH_MAX_ITERATIONS", default_value_t = 100)]
    max_iterations: u32,

    /// Auto-push after new commits
    #[arg(long, env = "RALPH_AUTO_PUSH", default_value = "true", value_parser = parse_bool, num_args = 1)]
    auto_push: bool,

    /// Override: path to executable replacing agent invocation (for testing)
    #[arg(long, env = "RALPH_COMMAND")]
    command: Option<String>,

    /// Spec stem — appends ./specs/<stem>.md as a system prompt file
    #[arg(long, env = "SGF_SPEC")]
    spec: Option<String>,

    /// Additional prompt file path (repeatable)
    #[arg(long = "prompt-file")]
    prompt_file: Vec<String>,

    /// Path to log file — ralph tees its output here
    #[arg(long)]
    log_file: Option<PathBuf>,
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
    match std::env::var("AGENT_CMD") {
        Ok(val) if !val.is_empty() => val,
        _ => {
            error!(
                "AGENT_CMD not set. Set AGENT_CMD to the path of the agent binary (e.g., AGENT_CMD=claude)."
            );
            std::process::exit(1);
        }
    }
}

fn resolve_prompt_files() -> Vec<String> {
    let default = "$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md";
    let raw = match std::env::var("PROMPT_FILES") {
        Ok(val) => val,
        Err(_) => {
            warn!("PROMPT_FILES not set, using default: {default}");
            default.to_string()
        }
    };

    let home = std::env::var("HOME").unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut files = Vec::new();
    for entry in raw.split(':') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        let expanded = entry.replace("$HOME", &home).replace('~', &home);

        let path = if expanded.starts_with("./") || expanded.starts_with("../") {
            cwd.join(&expanded)
        } else {
            PathBuf::from(&expanded)
        };

        if path.exists() {
            files.push(expanded);
        } else {
            warn!(path = %path.display(), "PROMPT_FILES entry not found, skipping");
        }
    }

    files
}

fn build_study_args(prompt_files: &[String]) -> Vec<String> {
    if prompt_files.is_empty() {
        return Vec::new();
    }
    let instruction = prompt_files
        .iter()
        .map(|f| format!("study @{f}"))
        .collect::<Vec<_>>()
        .join(";");
    vec!["--append-system-prompt".to_string(), instruction]
}

fn collect_prompt_files(cli: &Cli) -> Vec<String> {
    let mut files = resolve_prompt_files();

    if let Some(ref stem) = cli.spec {
        let spec_path = format!("./specs/{stem}.md");
        if !Path::new(&spec_path).exists() {
            error!("spec file not found: specs/{stem}.md");
            std::process::exit(1);
        }
        files.push(spec_path);
    }

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

    let config = ShutdownConfig {
        monitor_stdin: std::env::var("SGF_MANAGED").is_err(),
        ..Default::default()
    };
    let controller = ShutdownController::new(config).expect("Failed to create ShutdownController");

    let is_default_prompt = cli.prompt == "prompt.md";
    let is_file = Path::new(&cli.prompt).exists();

    if is_default_prompt && !is_file {
        error!(prompt = %cli.prompt, "prompt file not found");
        std::process::exit(1);
    }

    let prompt_files = collect_prompt_files(&cli);

    let iterations = if cli.iterations > cli.max_iterations {
        warn!(
            requested = cli.iterations,
            max = cli.max_iterations,
            "reducing iterations to max allowed"
        );
        cli.max_iterations
    } else {
        cli.iterations
    };

    print_banner(&cli, iterations, is_file, &prompt_files, &agent_cmd, &tee);

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

        let head_before = git_head();

        if cli.afk {
            run_afk(&agent_cmd, &cli, is_file, &prompt_files, &controller, &tee);
        } else {
            run_interactive(&agent_cmd, &cli, is_file, &prompt_files, &controller);
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
) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let watcher = thread::spawn(move || ding_watcher(&stop_clone));

    let prompt_arg = if is_file {
        format!("@{}", cli.prompt)
    } else {
        cli.prompt.clone()
    };

    let study_args = build_study_args(prompt_files);

    let mut command = Command::new(agent_cmd);
    command.args([
        "--verbose",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);
    command.args(&study_args);
    let mut child = match command
        .arg(&prompt_arg)
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
            let _ = child.kill();
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
) {
    let setsid_hook = || unsafe {
        libc::setsid();
        Ok(())
    };

    let prompt_arg = if is_file {
        format!("@{}", cli.prompt)
    } else {
        cli.prompt.clone()
    };

    let study_args = build_study_args(prompt_files);

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
    cmd.args(&study_args);
    let child = unsafe {
        cmd.arg(&prompt_arg)
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

    thread::spawn(move || {
        for line in reader.lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => match format::format_line(&line) {
                format::FormattedOutput::Text(text) => {
                    for l in text.split('\n') {
                        tee.write_ansi_line(&style::bold(l));
                    }
                }
                format::FormattedOutput::ToolCalls(calls) => {
                    for call in &calls {
                        tee.write_ansi_line(&format!(
                            "  {} {}  {}",
                            style::dim("─"),
                            style::tool_name_style(&call.name),
                            style::cyan(&call.detail),
                        ));
                    }
                }
                format::FormattedOutput::ToolResults(results) => {
                    for result in &results {
                        for l in &result.lines {
                            if result.is_error {
                                tee.write_ansi_line(&format!("     {}", style::red(l)));
                            } else if l.ends_with("... ok") {
                                tee.write_ansi_line(&format!(
                                    "     {}",
                                    style::dim(&style::green(l))
                                ));
                            } else if l.ends_with("... FAILED") {
                                tee.write_ansi_line(&format!("     {}", style::red(l)));
                            } else {
                                tee.write_ansi_line(&format!("     {}", style::dim(l)));
                            }
                        }
                        if result.truncated_count > 0 {
                            tee.write_ansi_line(&format!(
                                "     {}",
                                style::dim(&format!("... ({} more lines)", result.truncated_count))
                            ));
                        }
                    }
                }
                format::FormattedOutput::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    tee.write_ansi_line(&style::dim(&format!(
                        "  Input: {input_tokens} tokens · Output: {output_tokens} tokens"
                    )));
                }
                format::FormattedOutput::Result(text) => {
                    for l in text.split('\n') {
                        tee.write_ansi_line(l);
                    }
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

    if let Err(e) = child.wait() {
        warn!(error = %e, "error waiting for child process");
    }
}

fn git_head() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn auto_push_if_changed(cli: &Cli, head_before: &Option<String>, tee: &TeeWriter) {
    if !cli.auto_push {
        return;
    }

    let head_after = git_head();

    if let (Some(before), Some(after)) = (head_before, &head_after)
        && before != after
    {
        tee.writeln(&style::dim("New commits detected, pushing..."));
        match Command::new("git").arg("push").status() {
            Ok(status) if !status.success() => {
                warn!("git push failed, continuing");
            }
            Err(e) => {
                warn!(error = %e, "git push failed");
            }
            _ => {}
        }
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
}
