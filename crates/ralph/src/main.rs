pub(crate) mod format;

use clap::Parser;
use docker_ctx::docker_command;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
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
            let _ = writeln!(f, "{line}");
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
            let _ = writeln!(f, "{line}");
        }
    }
}

const SENTINEL: &str = ".ralph-complete";
const SENTINEL_MAX_DEPTH: usize = 2;
const DING_SENTINEL: &str = ".ralph-ding";

fn stop_sandbox() {
    info!("stopping docker sandbox");
    let _ = docker_command()
        .args(["sandbox", "stop", "claude"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn ensure_sandbox(template: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let workspace = cwd.to_string_lossy();
    info!(template, %workspace, "ensuring docker sandbox exists");
    let status = docker_command()
        .args([
            "sandbox",
            "create",
            "--template",
            template,
            "claude",
            &workspace,
        ])
        .stdin(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => info!("docker sandbox ready"),
        Ok(s) => {
            warn!(
                status = s.code().unwrap_or(-1),
                "docker sandbox create exited with non-zero status"
            );
        }
        Err(e) => warn!(error = %e, "failed to run docker sandbox create"),
    }
}

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

/// Iterative Claude Code runner via Docker sandbox.
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

    /// Docker sandbox template image
    #[arg(long, env = "RALPH_TEMPLATE", default_value = "ralph-sandbox:latest")]
    template: String,

    /// Safety limit for iterations
    #[arg(long, env = "RALPH_MAX_ITERATIONS", default_value_t = 100)]
    max_iterations: u32,

    /// Auto-push after new commits
    #[arg(long, env = "RALPH_AUTO_PUSH", default_value = "true", value_parser = parse_bool, num_args = 1)]
    auto_push: bool,

    /// Override: path to executable replacing docker invocation (for testing)
    #[arg(long, env = "RALPH_COMMAND")]
    command: Option<String>,

    /// Spec stem — appends ./specs/<stem>.md as a system prompt file
    #[arg(long, env = "SGF_SPEC")]
    spec: Option<String>,

    /// Additional system prompt file path (repeatable)
    #[arg(long = "system-file")]
    system_file: Vec<String>,

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

fn collect_system_files(cli: &Cli) -> Vec<String> {
    let mut files = resolve_prompt_files();

    if let Some(ref stem) = cli.spec {
        let spec_path = format!("./specs/{stem}.md");
        if !Path::new(&spec_path).exists() {
            error!("spec file not found: specs/{stem}.md");
            std::process::exit(1);
        }
        files.push(spec_path);
    }

    for path in &cli.system_file {
        if !Path::new(path).exists() {
            error!(path, "system file not found");
            std::process::exit(1);
        }
        files.push(path.clone());
    }

    files
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let sigint_count = Arc::new(AtomicUsize::new(0));
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let sigint_count = sigint_count.clone();
        unsafe {
            signal_hook::low_level::register(SIGINT, move || {
                sigint_count.fetch_add(1, Ordering::SeqCst);
            })
        }
        .expect("Failed to register SIGINT handler");
    }
    flag::register(SIGTERM, interrupted.clone()).expect("Failed to register SIGTERM handler");

    let is_default_prompt = cli.prompt == "prompt.md";
    let is_file = Path::new(&cli.prompt).exists();

    if is_default_prompt && !is_file {
        error!(prompt = %cli.prompt, "prompt file not found");
        std::process::exit(1);
    }

    let system_files = collect_system_files(&cli);

    let tee = TeeWriter::new(cli.log_file.as_deref()).unwrap_or_else(|e| {
        error!(error = %e, "failed to open log file");
        std::process::exit(1);
    });

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

    print_banner(&cli, iterations, is_file, &tee);

    remove_sentinel();
    let _ = fs::remove_file(DING_SENTINEL);

    if cli.command.is_none() {
        ensure_sandbox(&cli.template);
    }

    for i in 1..=iterations {
        remove_sentinel();

        tee.writeln("");
        tee.writeln("========================================");
        if let Some(ref id) = cli.loop_id {
            tee.writeln(&format!("Iteration {} of {} [{}]", i, iterations, id));
        } else {
            tee.writeln(&format!("Iteration {} of {}", i, iterations));
        }
        tee.writeln("========================================");
        tee.writeln("");

        let head_before = git_head();

        if cli.afk {
            run_afk(
                &cli,
                is_file,
                &system_files,
                &sigint_count,
                &interrupted,
                &tee,
            );
        } else {
            run_interactive(&cli, is_file, &system_files);
        }

        if sigint_count.load(Ordering::Relaxed) >= 1 || interrupted.load(Ordering::Relaxed) {
            warn!("interrupted");
            stop_sandbox();
            std::process::exit(130);
        }

        if let Some(sentinel_path) = find_sentinel(Path::new("."), SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(sentinel_path);
            tee.writeln("");
            tee.writeln("========================================");
            tee.writeln(&format!("Ralph COMPLETE after {} iterations!", i));
            tee.writeln("========================================");
            auto_push_if_changed(&cli, &head_before);
            std::process::exit(0);
        }

        tee.writeln("");
        tee.writeln(&format!("Iteration {} complete, continuing...", i));

        for _ in 0..20 {
            if sigint_count.load(Ordering::Relaxed) >= 1 || interrupted.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }

        if sigint_count.load(Ordering::Relaxed) >= 1 || interrupted.load(Ordering::Relaxed) {
            warn!("interrupted");
            stop_sandbox();
            std::process::exit(130);
        }

        auto_push_if_changed(&cli, &head_before);
    }

    remove_sentinel();
    tee.writeln("");
    tee.writeln("========================================");
    tee.writeln(&format!("Ralph reached max iterations ({})", iterations));
    tee.writeln("========================================");
    std::process::exit(2);
}

fn print_banner(cli: &Cli, iterations: u32, is_file: bool, tee: &TeeWriter) {
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
    tee.writeln("========================================");
    tee.writeln("Ralph Loop Starting");
    tee.writeln("========================================");
    tee.writeln(&format!(
        "Mode:        {}",
        if cli.afk { "AFK" } else { "Interactive" }
    ));
    if is_file {
        tee.writeln(&format!("Prompt:      {} (file)", cli.prompt));
    } else {
        let display = format::truncate(&cli.prompt, 60);
        tee.writeln(&format!("Prompt:      {} (text)", display));
    }
    tee.writeln(&format!("Iterations:  {}", iterations));
    tee.writeln(&format!("Sandbox:     {}", cli.template));
    if let Some(ref id) = cli.loop_id {
        tee.writeln(&format!("Loop ID:     {}", id));
    }
    tee.writeln("========================================");
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

fn run_interactive(cli: &Cli, is_file: bool, system_files: &[String]) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let watcher = thread::spawn(move || ding_watcher(&stop_clone));

    let prompt_arg = if is_file {
        format!("@{}", cli.prompt)
    } else {
        cli.prompt.clone()
    };

    let result = if let Some(ref cmd) = cli.command {
        let mut command = Command::new(cmd);
        for f in system_files {
            command.args(["--append-system-prompt-file", f]);
        }
        command
            .arg(&prompt_arg)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    } else {
        let mut cmd = docker_command();
        cmd.args([
            "sandbox",
            "run",
            "claude",
            "--",
            "--verbose",
            "--dangerously-skip-permissions",
        ]);
        for f in system_files {
            cmd.args(["--append-system-prompt-file", f]);
        }
        cmd.arg(&prompt_arg)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    };

    stop.store(true, Ordering::Relaxed);
    let _ = watcher.join();

    match result {
        Ok(status) if !status.success() => {
            warn!(
                status = status.code().unwrap_or(-1),
                "command exited with non-zero status"
            );
        }
        Err(e) => {
            warn!(error = %e, "failed to spawn command");
        }
        _ => {}
    }
}

fn run_afk(
    cli: &Cli,
    is_file: bool,
    system_files: &[String],
    sigint_count: &Arc<AtomicUsize>,
    interrupted: &Arc<AtomicBool>,
    tee: &TeeWriter,
) {
    // Two defenses keep Ctrl+C working in AFK mode:
    //
    // 1. PTY for docker's stdin: docker puts its stdin terminal into raw mode,
    //    which disables Ctrl+C signal generation. By giving docker its own PTY,
    //    raw mode only affects the PTY — ralph's terminal stays in cooked mode
    //    and Ctrl+C generates SIGINT normally. Docker requires isatty(0) == true,
    //    so we can't use Stdio::null().
    //
    // 2. setsid() in pre_exec: detaches docker from ralph's session so docker
    //    cannot call tcsetpgrp() on ralph's terminal (via the inherited stderr fd)
    //    to steal the foreground process group.
    let setsid_hook = || unsafe {
        libc::setsid();
        Ok(())
    };

    // Keeps the master end of the PTY alive until run_afk returns.
    // Dropping it early causes EIO on docker's stdin.
    let mut _pty_master: Option<OwnedFd> = None;

    let prompt_arg = if is_file {
        format!("@{}", cli.prompt)
    } else {
        cli.prompt.clone()
    };

    let child = if let Some(ref cmd) = cli.command {
        let mut command = Command::new(cmd);
        for f in system_files {
            command.args(["--append-system-prompt-file", f]);
        }
        unsafe {
            command
                .arg(&prompt_arg)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .pre_exec(setsid_hook)
                .spawn()
        }
    } else {
        let (master, slave_stdio) = create_pty_stdin();
        _pty_master = Some(master);
        let mut cmd = docker_command();
        cmd.args([
            "sandbox",
            "run",
            "claude",
            "--",
            "--verbose",
            "--print",
            "--output-format",
            "stream-json",
            "--dangerously-skip-permissions",
        ]);
        for f in system_files {
            cmd.args(["--append-system-prompt-file", f]);
        }
        unsafe {
            cmd.arg(&prompt_arg)
                .stdin(slave_stdio)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .pre_exec(setsid_hook)
                .spawn()
        }
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

    // Read stdout on a separate thread so the main thread can poll for
    // interrupts between lines. Without this, reader.lines() blocks
    // indefinitely and prevents Ctrl+C from taking effect in AFK mode.
    let reader = BufReader::new(stdout);
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for line in reader.lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut first_sigint_at: Option<Instant> = None;

    loop {
        if interrupted.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            stop_sandbox();
            return;
        }

        let count = sigint_count.load(Ordering::Relaxed);
        if count >= 2 {
            let _ = child.kill();
            let _ = child.wait();
            stop_sandbox();
            return;
        }
        if count == 1 {
            if let Some(first_at) = first_sigint_at {
                if first_at.elapsed() >= Duration::from_secs(2) {
                    sigint_count.store(0, Ordering::Relaxed);
                    first_sigint_at = None;
                }
            } else {
                tee.writeln("\nPress Ctrl+C again to stop\n");
                first_sigint_at = Some(Instant::now());
            }
        }

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                if let Some(output) = format::format_line(&line) {
                    for line in output.split('\n') {
                        tee.write_ansi_line(line);
                    }
                }
            }
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

fn create_pty_stdin() -> (OwnedFd, Stdio) {
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    unsafe {
        let ret = libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if ret != 0 {
            panic!("openpty failed: {}", std::io::Error::last_os_error());
        }
        (
            OwnedFd::from_raw_fd(master),
            Stdio::from(OwnedFd::from_raw_fd(slave)),
        )
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

fn auto_push_if_changed(cli: &Cli, head_before: &Option<String>) {
    if !cli.auto_push {
        return;
    }

    let head_after = git_head();

    if let (Some(before), Some(after)) = (head_before, &head_after)
        && before != after
    {
        println!("New commits detected, pushing...");
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
