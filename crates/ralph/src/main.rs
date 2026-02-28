pub(crate) mod format;

use clap::Parser;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

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

    /// Number of iterations to run
    #[arg(default_value_t = 1)]
    iterations: u32,

    /// Prompt file path or inline text string
    #[arg(default_value = "prompt.md")]
    prompt: String,

    /// Docker sandbox template image
    #[arg(long, env = "RALPH_TEMPLATE", default_value = "tauri-sandbox:latest")]
    template: String,

    /// Safety limit for iterations
    #[arg(long, env = "RALPH_MAX_ITERATIONS", default_value_t = 100)]
    max_iterations: u32,

    /// Auto-push after new commits
    #[arg(long, env = "RALPH_AUTO_PUSH", default_value = "true", value_parser = parse_bool)]
    auto_push: bool,

    /// Override: path to executable replacing docker invocation (for testing)
    #[arg(long, env = "RALPH_COMMAND")]
    command: Option<String>,
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

fn main() {
    let cli = Cli::parse();

    let interrupted = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, interrupted.clone()).expect("Failed to register SIGINT handler");
    flag::register(SIGTERM, interrupted.clone()).expect("Failed to register SIGTERM handler");

    let is_default_prompt = cli.prompt == "prompt.md";
    let is_file = Path::new(&cli.prompt).exists();

    if is_default_prompt && !is_file {
        eprintln!("Error: Prompt file '{}' not found", cli.prompt);
        std::process::exit(1);
    }

    let iterations = if cli.iterations > cli.max_iterations {
        eprintln!(
            "Warning: Reducing iterations from {} to max allowed ({})",
            cli.iterations, cli.max_iterations
        );
        cli.max_iterations
    } else {
        cli.iterations
    };

    print_banner(&cli, iterations, is_file);

    remove_sentinel();
    let _ = fs::remove_file(DING_SENTINEL);

    for i in 1..=iterations {
        println!();
        println!("========================================");
        println!("Iteration {} of {}", i, iterations);
        println!("========================================");
        println!();

        let head_before = git_head();

        if cli.afk {
            run_afk(&cli, is_file, &interrupted);
        } else {
            run_interactive(&cli, is_file);
        }

        if interrupted.load(Ordering::Relaxed) {
            eprintln!("\nInterrupted.");
            std::process::exit(130);
        }

        if let Some(sentinel_path) = find_sentinel(Path::new("."), SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(sentinel_path);
            println!();
            println!("========================================");
            println!("Ralph COMPLETE after {} iterations!", i);
            println!("========================================");
            auto_push_if_changed(&cli, &head_before);
            std::process::exit(0);
        }

        println!();
        println!("Iteration {} complete, continuing...", i);

        for _ in 0..20 {
            if interrupted.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }

        if interrupted.load(Ordering::Relaxed) {
            eprintln!("\nInterrupted.");
            std::process::exit(130);
        }

        auto_push_if_changed(&cli, &head_before);
    }

    remove_sentinel();
    println!();
    println!("========================================");
    println!("Ralph reached max iterations ({})", iterations);
    println!("========================================");
    std::process::exit(2);
}

fn print_banner(cli: &Cli, iterations: u32, is_file: bool) {
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣤⡴⣶⠖⡲⠒⡶⠒⣖⢲⡤⣄⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⢴⡾⣻⠟⢉⡞⢁⡞⠁⢠⠇⠀⠸⡄⠳⡈⢫⡙⢦⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⢀⡴⢚⡵⢋⡜⠁⢠⡎⠀⡞⠀⠀⢸⠀⠀⠀⡇⠀⢹⡀⠹⡌⢳⡙⣦⡀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠐⠋⠀⡞⠀⣸⠔⠒⠲⣄⠀⠀⠀⢀⡔⠋⠉⠙⠲⡀⠀⢷⠀⢹⡀⢱⡘⣟⣆⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⢰⠃⢸⠁⠀⣤⠄⠈⡇⠀⠀⢸⠀⠀⠾⠆⠀⡇⠀⠈⠀⠀⣇⠀⢧⢸⡘⡆⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠘⢆⡀⠀⣠⠴⢧⠀⠀⠈⠳⣄⣀⣠⠜⠁⠀⠀⠀⠀⠀⠀⠸⠄⡇⡇⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⢀⡼⠀⠀⠀⠉⠉⣇⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣤⠖⠲⣇⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⣏⠀⠀⠀⠀⠀⠀⠀⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⣀⣤⡄⠀⠀⠀⠀⢿⠓⣹⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠈⠓⠦⠤⠭⢿⡒⠒⠒⠒⠒⠒⠒⠒⠒⠊⠉⠉⠁⠀⠁⠀⠀⠀⠦⡤⠖⠃⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣾⡁⠀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⢴⡶⡇⠀⠀⠀⠀⠀⠀⣀⣀⣀⡤⠤⠤⠖⠚⠉⠁⠀⢧⠀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⢾⠏⠈⠳⣇⠀⠀⠀⠀⣠⠞⠁⠲⣄⠀⠀⠀⠀⠀⠀⢀⣠⡾⢤⡀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠀⣠⡾⠃⢸⡴⠚⠁⠈⠳⢤⡠⠞⠁⠀⠀⠀⠈⢦⢀⣀⡤⠖⠛⢩⠋⠀⠀⠈⢣⡀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⢀⣾⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠉⠀⠀⠀⠀⡞⠀⠀⠀⠀⠀⠹⡄⠀⠀");
    println!("⠀⠀⠀⠀⢠⢏⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀⠀⠀⠹⡀⠀");
    println!("⠀⠀⠀⣰⠃⡼⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣷⠀⠀⠀⠀⠀⠀⠀⢳⠀");
    println!("⠀⠀⢠⠇⢰⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀⠘⡆");
    println!("⠀⠀⡏⠀⢸⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀⠀⣇");
    println!("⠀⢰⢡⠀⢸⠀⠀⠀⠀⠀⢀⣀⣤⣤⣤⣤⣤⣤⣀⣠⣤⣤⣄⣀⣀⣀⣀⣀⣀⡀⠈⡇⠀⠀⠀⠀⠀⠀⠀⣿");
    println!("⠀⢸⢀⣀⣸⠞⠋⠉⠉⢉⣹⣿⣿⣿⣿⣿⣿⣿⣀⣀⣀⣀⣀⣀⣀⣀⣀⡀⠉⠉⠉⡗⠒⠒⠢⠤⣄⡀⠀⡿");
    println!("⠀⠘⢿⠁⢸⡴⠖⠛⠉⠉⠙⠛⠛⠛⠋⠉⠉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠉⣽⠟⠁⠀⠀⠀⠀⠀⠙⡖⠃");
    println!("⠀⠀⠘⣆⢣⣳⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠧⣤⠴⡄⠀⢀⠀⠀⢠⠃⠀");
    println!("⠀⠀⠀⠈⠢⣝⣻⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡼⠃⢀⡞⢠⠆⡞⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⠈⣯⠳⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠳⢶⣏⡴⠯⠞⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⢸⣿⠀⠀⠙⠶⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⢸⡏⠀⠀⠀⠀⠀⠉⠉⠛⠒⢲⠖⠚⠋⢹⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⣼⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⠀⠀⠀⢸⡆⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⡆⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⠀⡏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠸⡆⠀⠀⢸⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⡇⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⠀⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⢸⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢷⠀⠀⠀⠀⠀⠀");
    println!("⠀⠀⣠⡴⠒⠛⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠙⠛⣻⠖⠚⠉⠉⠉⠉⠉⠉⠉⠉⠉⠛⠛⠛⠛⢦⡀⠀⠀⠀⠀");
    println!("⢠⡾⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡞⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⠀⠀⠀⠀");
    println!("========================================");
    println!("Ralph Loop Starting");
    println!("========================================");
    println!(
        "Mode:        {}",
        if cli.afk { "AFK" } else { "Interactive" }
    );
    if is_file {
        println!("Prompt:      {} (file)", cli.prompt);
    } else {
        let display = format::truncate(&cli.prompt, 60);
        println!("Prompt:      {} (text)", display);
    }
    println!("Iterations:  {}", iterations);
    println!("Sandbox:     {}", cli.template);
    println!("========================================");
    println!();
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

fn run_interactive(cli: &Cli, is_file: bool) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let watcher = thread::spawn(move || ding_watcher(&stop_clone));

    let prompt_arg = if is_file {
        format!("@{}", cli.prompt)
    } else {
        cli.prompt.clone()
    };

    let result = if let Some(ref cmd) = cli.command {
        Command::new(cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    } else {
        Command::new("docker")
            .args([
                "sandbox",
                "run",
                "--credentials",
                "host",
                "--template",
                &cli.template,
                "claude",
                "--verbose",
                "--dangerously-skip-permissions",
                &prompt_arg,
            ])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    };

    stop.store(true, Ordering::Relaxed);
    let _ = watcher.join();

    match result {
        Ok(status) if !status.success() => {
            eprintln!(
                "Warning: command exited with status {}",
                status.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            eprintln!("Warning: failed to spawn command: {e}");
        }
        _ => {}
    }
}

fn run_afk(cli: &Cli, is_file: bool, interrupted: &Arc<AtomicBool>) {
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
        unsafe {
            Command::new(cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .pre_exec(setsid_hook)
                .spawn()
        }
    } else {
        let (master, slave_stdio) = create_pty_stdin();
        _pty_master = Some(master);
        unsafe {
            Command::new("docker")
                .args([
                    "sandbox",
                    "run",
                    "--credentials",
                    "host",
                    "--template",
                    &cli.template,
                    "claude",
                    "--verbose",
                    "--print",
                    "--output-format",
                    "stream-json",
                    &prompt_arg,
                ])
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
            eprintln!("Warning: failed to spawn command: {e}");
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            eprintln!("Warning: failed to capture stdout");
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

    loop {
        if interrupted.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                if let Some(output) = format::format_line(&line) {
                    // Docker sandbox writes spinner/progress output directly to /dev/tty,
                    // bypassing stdout/stderr redirection. These writes move the terminal
                    // cursor to unpredictable columns. Without correction, ralph's output
                    // appears at random horizontal offsets instead of left-aligned.
                    //
                    // Fix: prefix EVERY line with \r (carriage return to column 0) +
                    // \x1b[2K (ANSI clear entire line). This must apply to each line
                    // individually because text content from Claude contains embedded
                    // newlines (markdown lists, paragraphs, etc.) — a single prefix
                    // would only fix the first line of a multi-line block.
                    let stdout = std::io::stdout();
                    let mut lock = stdout.lock();
                    for line in output.split('\n') {
                        let _ = write!(lock, "\r\x1b[2K{line}\n");
                    }
                    let _ = lock.flush();
                }
            }
            Ok(Err(e)) => {
                eprintln!("Warning: error reading stdout: {e}");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Err(e) = child.wait() {
        eprintln!("Warning: error waiting for child process: {e}");
    }
}

fn create_pty_stdin() -> (OwnedFd, Stdio) {
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    let ret = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        ret,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );
    unsafe {
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
                eprintln!("Warning: git push failed, continuing...");
            }
            Err(e) => {
                eprintln!("Warning: git push failed: {e}");
            }
            _ => {}
        }
    }
}
