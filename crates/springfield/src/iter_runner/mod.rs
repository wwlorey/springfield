pub mod banner;
pub mod format;
pub mod pty_tee;

use shutdown::{ShutdownController, ShutdownStatus, kill_process_group};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use crate::style;

pub const SENTINEL: &str = ".iter-complete";
pub const SENTINEL_MAX_DEPTH: usize = 2;
const DING_SENTINEL: &str = ".iter-ding";
pub const MAX_ITERATIONS: u32 = 1000;
const DEFAULT_ITER_DELAY_MS: u64 = 2000;
const DEFAULT_POST_RESULT_TIMEOUT_SECS: u64 = 30;
const STARTUP_ERROR_THRESHOLD: Duration = Duration::from_secs(5);

fn iter_delay() -> Duration {
    let ms = std::env::var("SGF_TEST_ITER_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_ITER_DELAY_MS);
    Duration::from_millis(ms)
}

pub fn default_post_result_timeout() -> Duration {
    let secs = std::env::var("SGF_POST_RESULT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_POST_RESULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

pub struct ProgrammaticResult {
    pub content: String,
    pub session_id: String,
    pub exit_code: i32,
}

pub type IterationCallback = Box<dyn FnMut(u32, &str)>;

/// Callback invoked when a retry attempt is about to be made.
/// Arguments: (attempt_number, reason, next_retry_delay_secs).
pub type RetryCallback = Box<dyn FnMut(u32, &str, u64)>;

pub struct IterRunnerConfig {
    pub afk: bool,
    pub banner: bool,
    pub loop_id: Option<String>,
    pub iterations: u32,
    pub prompt: String,
    pub auto_push: bool,
    /// Override: path to executable replacing agent invocation (for testing).
    pub command: Option<String>,
    /// Additional prompt file paths injected via --append-system-prompt.
    pub prompt_files: Vec<String>,
    pub log_file: Option<PathBuf>,
    pub session_id: Option<String>,
    pub resume: Option<String>,
    /// Extra environment variables set on spawned agent commands.
    pub env_vars: Vec<(String, String)>,
    /// Name shown in banner messages (e.g. "sgf"). Defaults to empty.
    pub runner_name: Option<String>,
    /// Working directory for sentinel detection and spawned commands. Defaults to `.`.
    pub work_dir: Option<PathBuf>,
    /// Max time to wait for the agent process to exit after emitting its result event.
    /// Defaults to 30 seconds.
    pub post_result_timeout: Duration,
    /// Input text to pass as a CLI argument when resuming in programmatic mode.
    pub stdin_input: Option<String>,
    /// Called before each iteration starts with (iteration_number, session_id).
    pub on_iteration_start: Option<IterationCallback>,
    /// Called after each iteration completes with (iteration_number, session_id).
    pub on_iteration_complete: Option<IterationCallback>,
    /// Number of immediate retry attempts (no delay) before switching to backoff.
    pub retry_immediate: u32,
    /// Seconds between backoff retries.
    pub retry_interval_secs: u64,
    /// Maximum total time (seconds) to keep retrying before giving up.
    pub retry_max_duration_secs: u64,
    /// Called when a retry attempt is about to be made.
    pub on_retry: Option<RetryCallback>,
}

pub(crate) struct AgentExitStatus {
    pub(crate) exit_code: Option<i32>,
    pub(crate) killed_by_timeout: bool,
}

/// Exit codes returned by the iteration loop.
pub enum IterExitCode {
    /// Sentinel found — loop completed successfully.
    Complete = 0,
    /// Error (bad args, missing prompt, etc.).
    Error = 1,
    /// Iterations exhausted — may have remaining work.
    Exhausted = 2,
    /// Interrupted (SIGINT/SIGTERM).
    Interrupted = 130,
}

pub struct TeeWriter {
    log_file: Option<Mutex<fs::File>>,
}

impl TeeWriter {
    pub fn new(path: Option<&Path>) -> std::io::Result<Self> {
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

    pub fn writeln(&self, line: &str) {
        println!("{line}");
        if let Some(ref f) = self.log_file
            && let Ok(mut f) = f.lock()
        {
            let _ = writeln!(f, "{}", style::strip_ansi(line));
        }
    }

    pub fn write_ansi_line(&self, line: &str) {
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

pub fn find_sentinel(dir: &Path, max_depth: usize) -> Option<PathBuf> {
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

pub fn remove_sentinel_from(root: &Path) {
    if let Some(path) = find_sentinel(root, SENTINEL_MAX_DEPTH) {
        let _ = fs::remove_file(path);
    }
}

pub fn remove_sentinel() {
    remove_sentinel_from(Path::new("."));
}

pub fn save_terminal_settings() -> Option<libc::termios> {
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) == 0 {
            Some(termios)
        } else {
            None
        }
    }
}

pub fn restore_terminal_settings(termios: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, termios);
    }
}

pub fn check_agent_in_path(agent_cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join(agent_cmd).is_file() {
                return true;
            }
        }
    }
    false
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

fn ding_watcher(stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        if Path::new(DING_SENTINEL).exists() {
            let _ = fs::remove_file(DING_SENTINEL);
            if let Ok(mut child) = Command::new("afplay")
                .arg("/System/Library/Sounds/Blow.aiff")
                .spawn()
            {
                let _ = child.wait();
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_interactive(
    agent_cmd: &str,
    config: &IterRunnerConfig,
    is_file: bool,
    controller: &ShutdownController,
    iteration: u32,
    session_id: &str,
) -> AgentExitStatus {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let watcher = thread::spawn(move || ding_watcher(&stop_clone));

    let asp_args = build_append_system_prompt_args(&config.prompt_files);

    let mut command = Command::new(agent_cmd);
    command.args([
        "--verbose",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);
    command.args(["--session-id", session_id]);
    command.args(&asp_args);
    for (key, val) in &config.env_vars {
        command.env(key, val);
    }
    let resuming = iteration == 1 && config.resume.is_some();
    if resuming {
        command.args(["--resume", config.resume.as_ref().unwrap()]);
    } else {
        let prompt_arg = if is_file {
            format!("@{}", config.prompt)
        } else {
            config.prompt.clone()
        };
        command.arg(&prompt_arg);
    }

    let result =
        pty_tee::run_interactive_with_pty(&mut command, config.log_file.as_deref(), controller);

    stop.store(true, Ordering::Relaxed);
    let _ = watcher.join();

    match result {
        Ok(status) => {
            if let Some(code) = status.exit_code
                && code != 0
            {
                warn!(status = code, "command exited with non-zero status");
            }
            status
        }
        Err(e) => {
            warn!(error = %e, "failed to spawn command via PTY");
            AgentExitStatus {
                exit_code: None,
                killed_by_timeout: false,
            }
        }
    }
}

fn run_afk(
    agent_cmd: &str,
    config: &IterRunnerConfig,
    is_file: bool,
    controller: &ShutdownController,
    tee: &TeeWriter,
    iteration: u32,
    session_id: &str,
) -> AgentExitStatus {
    let skip_setsid = std::env::var("SGF_TEST_NO_SETSID").is_ok();
    let setsid_hook = move || unsafe {
        if !skip_setsid {
            libc::setsid();
        }
        Ok(())
    };

    let asp_args = build_append_system_prompt_args(&config.prompt_files);

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
    cmd.args(["--session-id", session_id]);
    cmd.args(&asp_args);
    for (key, val) in &config.env_vars {
        cmd.env(key, val);
    }
    let resuming = iteration == 1 && config.resume.is_some();
    if resuming {
        cmd.args(["--resume", config.resume.as_ref().unwrap()]);
    } else {
        let prompt_arg = if is_file {
            format!("@{}", config.prompt)
        } else {
            config.prompt.clone()
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
            return AgentExitStatus {
                exit_code: None,
                killed_by_timeout: false,
            };
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            warn!("failed to capture stdout");
            return AgentExitStatus {
                exit_code: None,
                killed_by_timeout: false,
            };
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
    let mut result_received_at: Option<std::time::Instant> = None;
    let post_result_timeout = config.post_result_timeout;
    let mut killed_by_timeout = false;

    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            kill_process_group(child_pid, Duration::from_millis(200));
            let _ = child.wait();
            let _ = reader_handle.join();
            return AgentExitStatus {
                exit_code: None,
                killed_by_timeout: false,
            };
        }

        if let Some(received_at) = result_received_at
            && received_at.elapsed() > post_result_timeout
        {
            warn!(
                elapsed_secs = received_at.elapsed().as_secs(),
                "agent process did not exit after result event, killing"
            );
            killed_by_timeout = true;
            break;
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
                    result_received_at = Some(std::time::Instant::now());
                }
                format::FormattedOutput::Result(text) => {
                    tee.write_ansi_line("");
                    for l in text.split('\n') {
                        tee.write_ansi_line(l);
                    }
                    tee.write_ansi_line("");
                    if result_received_at.is_none() {
                        result_received_at = Some(std::time::Instant::now());
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

    kill_process_group(child_pid, Duration::from_millis(200));
    let exit_code = match child.wait() {
        Ok(status) => status.code(),
        Err(e) => {
            warn!(error = %e, "error waiting for child process");
            None
        }
    };
    let _ = reader_handle.join();
    AgentExitStatus {
        exit_code,
        killed_by_timeout,
    }
}

pub fn run_programmatic(
    agent_cmd: &str,
    config: &IterRunnerConfig,
    is_file: bool,
    _controller: &ShutdownController,
    iteration: u32,
    session_id: &str,
) -> std::io::Result<ProgrammaticResult> {
    let asp_args = build_append_system_prompt_args(&config.prompt_files);

    let mut cmd = Command::new(agent_cmd);
    cmd.args([
        "--print",
        "--output-format",
        "json",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);
    cmd.args(&asp_args);
    for (key, val) in &config.env_vars {
        cmd.env(key, val);
    }

    let resuming = iteration == 1 && config.resume.is_some();
    if resuming {
        cmd.args(["--resume", config.resume.as_ref().unwrap()]);
        if let Some(ref input) = config.stdin_input {
            cmd.arg(input);
        }
    } else {
        cmd.args(["--session-id", session_id]);
        let prompt_arg = if is_file {
            format!("@{}", config.prompt)
        } else {
            config.prompt.clone()
        };
        cmd.arg(&prompt_arg);
    }

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let output = cmd
        .output()
        .map_err(|e| std::io::Error::new(e.kind(), format!("failed to spawn command: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_code = output.status.code().unwrap_or(1);

    if let Some(log_path) = config.log_file.as_deref() {
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            let stripped = style::strip_ansi(&String::from_utf8_lossy(&output.stdout));
            let _ = f.write_all(stripped.as_bytes());
        }
    }

    let (content, result_session_id) =
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let result_obj = match &parsed {
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .find(|v| v["type"].as_str() == Some("result"))
                    .unwrap_or(&parsed),
                _ => &parsed,
            };
            let content = result_obj["result"].as_str().unwrap_or("").to_string();
            let sid = result_obj["session_id"].as_str().unwrap_or("").to_string();
            (content, sid)
        } else {
            warn!("failed to parse agent JSON output");
            (stdout, String::new())
        };

    Ok(ProgrammaticResult {
        content,
        session_id: result_session_id,
        exit_code,
    })
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

fn auto_push_if_changed(config: &IterRunnerConfig, head_before: &Option<String>, tee: &TeeWriter) {
    if !config.auto_push {
        return;
    }

    if let Some(before) = head_before {
        vcs_utils::auto_push_if_changed(before, |msg| tee.writeln(&style::dim(msg)));
    }
}

fn print_startup_banner(
    config: &IterRunnerConfig,
    iterations: u32,
    is_file: bool,
    agent_cmd: &str,
    tee: &TeeWriter,
) {
    let mut body = vec![
        format!(
            "Mode:        {}",
            if config.afk { "AFK" } else { "Interactive" }
        ),
        if is_file {
            format!("Prompt:      {} (file)", config.prompt)
        } else {
            let display = format::truncate(&config.prompt, 60);
            format!("Prompt:      {} (text)", display)
        },
        format!("Iterations:  {}", iterations),
        format!("Agent:       {}", agent_cmd),
    ];
    if let Some(ref id) = config.loop_id {
        body.push(format!("Loop ID:     {}", id));
    }
    if !config.prompt_files.is_empty() {
        body.push("Prompt files:".to_string());
        for f in &config.prompt_files {
            body.push(format!("  - {}", f));
        }
    }
    let title = match &config.runner_name {
        Some(name) => format!("{} Loop Starting", name),
        None => "Iteration Loop Starting".to_string(),
    };
    for line in banner::render_box(&title, &body).split('\n') {
        tee.writeln(line);
    }
    tee.writeln("");
}

/// Run the iteration loop. Returns an `IterExitCode` instead of calling `process::exit`.
///
/// This is the core iteration loop. It handles:
/// - TeeWriter (dual stdout + log file output)
/// - Stdout reader thread (AFK mode NDJSON parsing)
fn is_retryable_process_failure(status: &AgentExitStatus, elapsed: Duration) -> bool {
    if status.killed_by_timeout {
        return false;
    }
    let code = match status.exit_code {
        Some(0) => return false,
        Some(c) => c,
        None => return true, // killed by signal (SIGSEGV, SIGKILL, etc.)
    };
    if code == 130 {
        return false;
    }
    if elapsed < STARTUP_ERROR_THRESHOLD {
        return false;
    }
    true
}

fn estimate_max_attempts(immediate: u32, interval_secs: u64, max_duration_secs: u64) -> u32 {
    immediate
        + if interval_secs > 0 {
            (max_duration_secs / interval_secs) as u32
        } else {
            0
        }
}

fn run_agent_with_retry(
    agent_cmd: &str,
    config: &mut IterRunnerConfig,
    is_file: bool,
    controller: &ShutdownController,
    tee: &Arc<TeeWriter>,
    iteration: u32,
    session_id: &str,
) {
    let start = std::time::Instant::now();
    let status = if config.afk {
        run_afk(
            agent_cmd, config, is_file, controller, tee, iteration, session_id,
        )
    } else {
        run_interactive(
            agent_cmd, config, is_file, controller, iteration, session_id,
        )
    };
    let elapsed = start.elapsed();

    if !is_retryable_process_failure(&status, elapsed) {
        return;
    }

    let first_failure = std::time::Instant::now();
    let max_duration = Duration::from_secs(config.retry_max_duration_secs);
    let max_attempts = estimate_max_attempts(
        config.retry_immediate,
        config.retry_interval_secs,
        config.retry_max_duration_secs,
    );
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;

        if first_failure.elapsed() >= max_duration {
            warn!(
                total_secs = first_failure.elapsed().as_secs(),
                "retry duration exceeded, giving up"
            );
            style::print_error("retry duration exceeded, giving up");
            return;
        }

        if controller.poll() == ShutdownStatus::Shutdown {
            return;
        }

        let in_backoff = attempt > config.retry_immediate;
        let next_retry_secs = if in_backoff {
            config.retry_interval_secs
        } else {
            0
        };

        if let Some(ref mut cb) = config.on_retry {
            cb(attempt, "process_failure", next_retry_secs);
        }

        let msg = if in_backoff {
            format!(
                "retrying in {}m (attempt {}/{})...",
                config.retry_interval_secs / 60,
                attempt,
                max_attempts
            )
        } else {
            format!(
                "retrying immediately (attempt {}/{})...",
                attempt, max_attempts
            )
        };
        style::print_warning(&msg);

        if in_backoff {
            let interval = Duration::from_secs(config.retry_interval_secs);
            let tick = Duration::from_millis(500);
            let mut waited = Duration::ZERO;
            while waited < interval {
                if controller.poll() == ShutdownStatus::Shutdown {
                    return;
                }
                thread::sleep(tick);
                waited += tick;
            }
        }

        config.resume = Some(session_id.to_string());

        let start = std::time::Instant::now();
        let retry_status = if config.afk {
            run_afk(agent_cmd, config, is_file, controller, tee, 1, session_id)
        } else {
            run_interactive(agent_cmd, config, is_file, controller, 1, session_id)
        };
        let retry_elapsed = start.elapsed();

        if !is_retryable_process_failure(&retry_status, retry_elapsed) {
            return;
        }
    }
}

/// - Notification watcher (.iter-ding)
/// - Terminal settings save/restore (tcgetattr/tcsetattr)
/// - Agent-in-PATH check
/// - Iteration clamping (max 1000)
/// - Inter-iteration 2s interruptible sleep
/// - Sentinel search (recursive depth<=2) and stale sentinel cleanup
/// - Main run loop for both AFK and interactive modes
pub fn run_iteration_loop(
    mut config: IterRunnerConfig,
    controller: &ShutdownController,
) -> IterExitCode {
    let tee = match TeeWriter::new(config.log_file.as_deref()) {
        Ok(t) => Arc::new(t),
        Err(e) => {
            tracing::error!(error = %e, "failed to open log file");
            return IterExitCode::Error;
        }
    };

    let agent_cmd = config.command.clone().unwrap_or_else(|| "cl".to_string());

    if config.command.is_none() && !check_agent_in_path(&agent_cmd) {
        tracing::error!("cl not found in PATH");
        return IterExitCode::Error;
    }

    let is_default_prompt = config.prompt == "prompt.md";
    let is_file = Path::new(&config.prompt).exists();

    if config.resume.is_none() && is_default_prompt && !is_file {
        tracing::error!(prompt = %config.prompt, "prompt file not found");
        return IterExitCode::Error;
    }

    let iterations = if config.iterations > MAX_ITERATIONS {
        warn!(
            requested = config.iterations,
            max = MAX_ITERATIONS,
            "clamping iterations to hard limit"
        );
        MAX_ITERATIONS
    } else {
        config.iterations
    };

    if config.banner {
        print_startup_banner(&config, iterations, is_file, &agent_cmd, &tee);
    }

    let root = config
        .work_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let root = root.as_path();

    remove_sentinel_from(root);
    let _ = fs::remove_file(root.join(DING_SENTINEL));

    let saved_termios = save_terminal_settings();

    for i in 1..=iterations {
        remove_sentinel_from(root);

        let iter_session_id = if i == 1 {
            config
                .session_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        } else {
            uuid::Uuid::new_v4().to_string()
        };

        let iter_title = if let Some(ref id) = config.loop_id {
            format!("Iteration {} of {} [{}]", i, iterations, id)
        } else {
            format!("Iteration {} of {}", i, iterations)
        };
        tee.writeln("");
        for line in banner::render_box(&iter_title, &[]).split('\n') {
            tee.writeln(line);
        }
        tee.writeln("");

        if let Some(ref mut cb) = config.on_iteration_start {
            cb(i, &iter_session_id);
        }

        let head_before = vcs_utils::git_head();

        run_agent_with_retry(
            &agent_cmd,
            &mut config,
            is_file,
            controller,
            &tee,
            i,
            &iter_session_id,
        );

        if let Some(ref termios) = saved_termios {
            restore_terminal_settings(termios);
        }

        if let Some(ref mut cb) = config.on_iteration_complete {
            cb(i, &iter_session_id);
        }

        if controller.poll() == ShutdownStatus::Shutdown {
            warn!("interrupted");
            return IterExitCode::Interrupted;
        }

        if let Some(sentinel_path) = find_sentinel(root, SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(sentinel_path);
            let complete_title = match &config.runner_name {
                Some(name) => format!("{} COMPLETE after {} iterations!", name, i),
                None => format!("COMPLETE after {} iterations!", i),
            };
            tee.writeln("");
            for line in
                banner::render_box_styled(&complete_title, &[], |s| style::bold(&style::green(s)))
                    .split('\n')
            {
                tee.writeln(line);
            }
            auto_push_if_changed(&config, &head_before, &tee);
            return IterExitCode::Complete;
        }

        log_resource_usage(i);

        tee.writeln("");
        tee.writeln(&style::dim(&format!(
            "Iteration {} complete, continuing...",
            i
        )));

        let tick = Duration::from_millis(100);
        let mut elapsed = Duration::ZERO;
        let target = iter_delay();
        while elapsed < target {
            if controller.poll() == ShutdownStatus::Shutdown {
                warn!("interrupted");
                return IterExitCode::Interrupted;
            }
            thread::sleep(tick);
            elapsed += tick;
        }

        auto_push_if_changed(&config, &head_before, &tee);
    }

    remove_sentinel_from(root);
    let max_title = match &config.runner_name {
        Some(name) => format!("{} reached max iterations ({})", name, iterations),
        None => format!("Reached max iterations ({})", iterations),
    };
    tee.writeln("");
    for line in
        banner::render_box_styled(&max_title, &[], |s| style::bold(&style::yellow(s))).split('\n')
    {
        tee.writeln(line);
    }
    IterExitCode::Exhausted
}

#[cfg(test)]
mod tests {
    use super::*;
    use shutdown::ShutdownConfig;

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
    fn find_sentinel_too_deep() {
        let dir = tempfile::TempDir::new().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), 2).is_none());
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
    fn check_agent_in_path_finds_existing() {
        assert!(check_agent_in_path("ls"));
    }

    #[test]
    fn check_agent_in_path_missing() {
        assert!(!check_agent_in_path("nonexistent_binary_xyz_12345"));
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
    fn tee_writer_no_log() {
        let tee = TeeWriter::new(None).unwrap();
        tee.writeln("hello");
        tee.write_ansi_line("world");
    }

    #[test]
    fn tee_writer_with_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let tee = TeeWriter::new(Some(&log_path)).unwrap();
        tee.writeln("hello");
        tee.write_ansi_line(&style::bold("styled"));
        let content = fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("styled"));
        assert!(!content.contains("\x1b["));
    }

    #[test]
    fn iter_exit_code_values() {
        assert_eq!(IterExitCode::Complete as i32, 0);
        assert_eq!(IterExitCode::Error as i32, 1);
        assert_eq!(IterExitCode::Exhausted as i32, 2);
        assert_eq!(IterExitCode::Interrupted as i32, 130);
    }

    #[test]
    fn iteration_clamping() {
        assert_eq!(MAX_ITERATIONS, 1000);
    }

    #[test]
    fn default_iter_delay_is_2s() {
        assert_eq!(DEFAULT_ITER_DELAY_MS, 2000);
    }

    #[test]
    fn remove_sentinel_no_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = SetCurrentDir::new(dir.path());
        remove_sentinel();
    }

    struct SetCurrentDir {
        prev: PathBuf,
    }

    impl SetCurrentDir {
        fn new(path: &Path) -> Self {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            SetCurrentDir { prev }
        }
    }

    impl Drop for SetCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }

    fn mock_script(dir: &Path, name: &str, script: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().to_string()
    }

    fn make_config(dir: &Path, command: String) -> IterRunnerConfig {
        IterRunnerConfig {
            afk: true,
            banner: false,
            loop_id: None,
            iterations: 1,
            prompt: "test".to_string(),
            auto_push: false,
            command: Some(command),
            prompt_files: vec![],
            log_file: None,
            session_id: None,
            resume: None,
            env_vars: vec![],
            runner_name: None,
            work_dir: Some(dir.to_path_buf()),
            post_result_timeout: Duration::from_secs(30),
            stdin_input: None,
            on_iteration_start: None,
            on_iteration_complete: None,
            retry_immediate: 3,
            retry_interval_secs: 300,
            retry_max_duration_secs: 43200,
            on_retry: None,
        }
    }

    #[test]
    fn post_result_timeout_kills_hung_process() {
        let dir = tempfile::tempdir().unwrap();
        let result_json = r#"{"type":"result","result":"Done.","session_id":"s1","usage":{"input_tokens":100,"output_tokens":200}}"#;
        let script = mock_script(
            dir.path(),
            "hang_after_result.sh",
            &format!("#!/bin/sh\necho '{}'\nsleep 300\n", result_json),
        );

        let mut config = make_config(dir.path(), script);
        config.post_result_timeout = Duration::from_secs(2);

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let start = std::time::Instant::now();
        let exit_code = run_iteration_loop(config, &controller);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "should have killed hung process within timeout, took {:?}",
            elapsed
        );
        assert!(
            elapsed >= Duration::from_secs(2),
            "should have waited for the timeout period, only took {:?}",
            elapsed
        );
        assert!(matches!(exit_code, IterExitCode::Exhausted));
    }

    #[test]
    fn clean_exit_not_affected_by_post_result_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let result_json = r#"{"type":"result","result":"Done.","session_id":"s1","usage":{"input_tokens":100,"output_tokens":200}}"#;
        let script = mock_script(
            dir.path(),
            "clean_exit.sh",
            &format!("#!/bin/sh\necho '{}'\nexit 0\n", result_json),
        );

        let mut config = make_config(dir.path(), script);
        config.post_result_timeout = Duration::from_secs(30);

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let start = std::time::Instant::now();
        let _exit_code = run_iteration_loop(config, &controller);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "clean exit should not wait for post-result timeout, took {:?}",
            elapsed
        );
    }

    #[test]
    fn result_without_usage_also_triggers_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let result_json = r#"{"type":"result","result":"Done."}"#;
        let script = mock_script(
            dir.path(),
            "hang_after_result_no_usage.sh",
            &format!("#!/bin/sh\necho '{}'\nsleep 300\n", result_json),
        );

        let mut config = make_config(dir.path(), script);
        config.post_result_timeout = Duration::from_secs(2);

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let start = std::time::Instant::now();
        let _exit_code = run_iteration_loop(config, &controller);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(10),
            "should have killed hung process within timeout, took {:?}",
            elapsed
        );
        assert!(
            elapsed >= Duration::from_secs(2),
            "should have waited for the timeout period, only took {:?}",
            elapsed
        );
    }

    #[test]
    fn on_iteration_complete_callback_invoked() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = dir.path().join(SENTINEL);
        let script = mock_script(
            dir.path(),
            "callback_test.sh",
            &format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", sentinel.display()),
        );

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = recorded.clone();

        let mut config = make_config(dir.path(), script);
        config.session_id = Some("test-uuid-1234".to_string());
        config.on_iteration_complete = Some(Box::new(move |iter, sid| {
            recorded_clone.lock().unwrap().push((iter, sid.to_string()));
        }));

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let exit_code = run_iteration_loop(config, &controller);
        assert!(matches!(exit_code, IterExitCode::Complete));

        let calls = recorded.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 1);
        assert_eq!(calls[0].1, "test-uuid-1234");
    }

    #[test]
    fn on_iteration_complete_called_per_iteration_with_unique_session_ids() {
        let dir = tempfile::tempdir().unwrap();
        let script = mock_script(dir.path(), "multi_iter_cb.sh", "#!/bin/sh\nexit 0\n");

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = recorded.clone();

        let mut config = make_config(dir.path(), script);
        config.iterations = 3;
        config.session_id = Some("initial-uuid".to_string());
        config.on_iteration_complete = Some(Box::new(move |iter, sid| {
            recorded_clone.lock().unwrap().push((iter, sid.to_string()));
        }));

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let exit_code = run_iteration_loop(config, &controller);
        assert!(matches!(exit_code, IterExitCode::Exhausted));

        let calls = recorded.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, 1);
        assert_eq!(calls[1].0, 2);
        assert_eq!(calls[2].0, 3);
        assert_eq!(calls[0].1, "initial-uuid");
        assert_ne!(calls[1].1, calls[0].1, "iteration 2 should have fresh UUID");
        assert_ne!(calls[2].1, calls[1].1, "iteration 3 should have fresh UUID");
    }

    #[test]
    fn run_programmatic_parses_json_response() {
        let dir = tempfile::tempdir().unwrap();
        let result_json = r#"{"result":"All done.","session_id":"sess-abc123"}"#;
        let script = mock_script(
            dir.path(),
            "prog_json.sh",
            &format!("#!/bin/sh\necho '{}'\n", result_json),
        );

        let config = make_config(dir.path(), script.clone());
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.content, "All done.");
        assert_eq!(result.session_id, "sess-abc123");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_programmatic_handles_non_json_output() {
        let dir = tempfile::tempdir().unwrap();
        let script = mock_script(dir.path(), "prog_plain.sh", "#!/bin/sh\necho 'not json'\n");

        let config = make_config(dir.path(), script.clone());
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.content, "not json\n");
        assert!(result.session_id.is_empty());
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_programmatic_captures_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let result_json = r#"{"result":"error","session_id":"s1"}"#;
        let script = mock_script(
            dir.path(),
            "prog_fail.sh",
            &format!("#!/bin/sh\necho '{}'\nexit 1\n", result_json),
        );

        let config = make_config(dir.path(), script.clone());
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.exit_code, 1);
        assert_eq!(result.content, "error");
    }

    #[test]
    fn run_programmatic_with_resume_passes_input_as_arg() {
        let dir = tempfile::tempdir().unwrap();
        // Script checks that --resume and the input are present in args
        let script = mock_script(
            dir.path(),
            "prog_resume.sh",
            r#"#!/bin/sh
has_resume=0
has_input=0
for arg in "$@"; do
    case "$arg" in
        --resume) has_resume=1 ;;
        "use bcrypt") has_input=1 ;;
    esac
done
if [ "$has_resume" = "1" ] && [ "$has_input" = "1" ]; then
    echo '{"result":"resumed","session_id":"s2"}'
else
    echo '{"result":"missing_args","session_id":"s2"}'
fi
"#,
        );

        let mut config = make_config(dir.path(), script.clone());
        config.resume = Some("prev-session-id".to_string());
        config.stdin_input = Some("use bcrypt".to_string());

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.content, "resumed");
        assert_eq!(result.session_id, "s2");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_programmatic_missing_fields_defaults_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let script = mock_script(
            dir.path(),
            "prog_minimal.sh",
            "#!/bin/sh\necho '{\"other\":\"field\"}'\n",
        );

        let config = make_config(dir.path(), script.clone());
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert!(result.content.is_empty());
        assert!(result.session_id.is_empty());
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_programmatic_parses_verbose_json_array() {
        let dir = tempfile::tempdir().unwrap();
        let verbose_json = r#"[{"type":"system","subtype":"init"},{"type":"assistant"},{"type":"result","subtype":"success","result":"Done.","session_id":"sess-verbose"}]"#;
        let script = mock_script(
            dir.path(),
            "prog_verbose.sh",
            &format!("#!/bin/sh\necho '{}'\n", verbose_json),
        );

        let config = make_config(dir.path(), script.clone());
        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.content, "Done.");
        assert_eq!(result.session_id, "sess-verbose");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_programmatic_env_vars_propagated() {
        let dir = tempfile::tempdir().unwrap();
        let script = mock_script(
            dir.path(),
            "prog_env.sh",
            "#!/bin/sh\necho \"{\\\"result\\\":\\\"$MY_VAR\\\",\\\"session_id\\\":\\\"s1\\\"}\"\n",
        );

        let mut config = make_config(dir.path(), script.clone());
        config.env_vars = vec![("MY_VAR".to_string(), "hello_world".to_string())];

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_programmatic(&script, &config, false, &controller, 1, "test-sid").unwrap();

        assert_eq!(result.content, "hello_world");
    }

    fn exit(code: Option<i32>) -> AgentExitStatus {
        AgentExitStatus {
            exit_code: code,
            killed_by_timeout: false,
        }
    }

    #[test]
    fn retryable_nonzero_after_threshold() {
        assert!(is_retryable_process_failure(
            &exit(Some(1)),
            Duration::from_secs(10)
        ));
        assert!(is_retryable_process_failure(
            &exit(Some(2)),
            Duration::from_secs(60)
        ));
        assert!(is_retryable_process_failure(
            &exit(Some(137)),
            Duration::from_secs(10)
        ));
    }

    #[test]
    fn not_retryable_success() {
        assert!(!is_retryable_process_failure(
            &exit(Some(0)),
            Duration::from_secs(0)
        ));
        assert!(!is_retryable_process_failure(
            &exit(Some(0)),
            Duration::from_secs(100)
        ));
    }

    #[test]
    fn not_retryable_interrupted() {
        assert!(!is_retryable_process_failure(
            &exit(Some(130)),
            Duration::from_secs(0)
        ));
        assert!(!is_retryable_process_failure(
            &exit(Some(130)),
            Duration::from_secs(100)
        ));
    }

    #[test]
    fn not_retryable_fast_exit() {
        assert!(!is_retryable_process_failure(
            &exit(Some(1)),
            Duration::from_secs(0)
        ));
        assert!(!is_retryable_process_failure(
            &exit(Some(1)),
            Duration::from_secs(1)
        ));
        assert!(!is_retryable_process_failure(
            &exit(Some(2)),
            Duration::from_secs(4)
        ));
        assert!(!is_retryable_process_failure(
            &exit(Some(137)),
            Duration::from_secs(3)
        ));
    }

    #[test]
    fn retryable_after_threshold() {
        assert!(is_retryable_process_failure(
            &exit(Some(1)),
            Duration::from_secs(5)
        ));
        assert!(is_retryable_process_failure(
            &exit(Some(1)),
            Duration::from_secs(6)
        ));
        assert!(is_retryable_process_failure(
            &exit(Some(2)),
            Duration::from_secs(10)
        ));
    }

    #[test]
    fn retryable_signal_kill() {
        assert!(is_retryable_process_failure(
            &exit(None),
            Duration::from_millis(0)
        ));
        assert!(is_retryable_process_failure(
            &exit(None),
            Duration::from_secs(10)
        ));
    }

    #[test]
    fn not_retryable_timeout_kill() {
        let status = AgentExitStatus {
            exit_code: None,
            killed_by_timeout: true,
        };
        assert!(!is_retryable_process_failure(
            &status,
            Duration::from_secs(10)
        ));
    }

    #[test]
    fn estimate_max_attempts_default_config() {
        // 3 immediate + 43200/300 = 3 + 144 = 147
        assert_eq!(estimate_max_attempts(3, 300, 43200), 147);
    }

    #[test]
    fn estimate_max_attempts_custom_config() {
        // 5 immediate + 86400/600 = 5 + 144 = 149
        assert_eq!(estimate_max_attempts(5, 600, 86400), 149);
    }

    #[test]
    fn estimate_max_attempts_zero_interval() {
        assert_eq!(estimate_max_attempts(3, 0, 43200), 3);
    }

    #[test]
    fn estimate_max_attempts_zero_immediate() {
        assert_eq!(estimate_max_attempts(0, 300, 43200), 144);
    }

    #[test]
    fn estimate_max_attempts_zero_duration() {
        assert_eq!(estimate_max_attempts(3, 300, 0), 3);
    }

    #[test]
    fn retry_callback_receives_attempt_count() {
        let dir = tempfile::tempdir().unwrap();
        // Script sleeps 6s (past STARTUP_ERROR_THRESHOLD) then exits 1 (retryable)
        // Second run exits 0 (success)
        let state_file = dir.path().join("state");
        let script = mock_script(
            dir.path(),
            "retry_cb.sh",
            &format!(
                "#!/bin/sh\nif [ -f \"{}\" ]; then exit 0; fi\ntouch \"{}\"\nsleep 6\nexit 1\n",
                state_file.display(),
                state_file.display()
            ),
        );

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = recorded.clone();

        let mut config = make_config(dir.path(), script);
        config.retry_immediate = 3;
        config.retry_interval_secs = 300;
        config.retry_max_duration_secs = 43200;
        config.on_retry = Some(Box::new(move |attempt, reason, next_retry_secs| {
            recorded_clone
                .lock()
                .unwrap()
                .push((attempt, reason.to_string(), next_retry_secs));
        }));

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let _exit_code = run_iteration_loop(config, &controller);

        let calls = recorded.lock().unwrap();
        assert_eq!(calls.len(), 1, "should have retried once");
        assert_eq!(calls[0].0, 1, "first retry attempt");
        assert_eq!(calls[0].1, "process_failure");
        assert_eq!(calls[0].2, 0, "immediate retry has 0 delay");
    }

    #[test]
    fn on_iteration_start_called_before_complete() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = dir.path().join(SENTINEL);
        let script = mock_script(
            dir.path(),
            "start_cb_test.sh",
            &format!("#!/bin/sh\ntouch \"{}\"\nexit 0\n", sentinel.display()),
        );

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_start = events.clone();
        let events_complete = events.clone();

        let mut config = make_config(dir.path(), script);
        config.session_id = Some("start-test-uuid".to_string());
        config.on_iteration_start = Some(Box::new(move |iter, sid| {
            events_start
                .lock()
                .unwrap()
                .push(("start".to_string(), iter, sid.to_string()));
        }));
        config.on_iteration_complete = Some(Box::new(move |iter, sid| {
            events_complete
                .lock()
                .unwrap()
                .push(("complete".to_string(), iter, sid.to_string()));
        }));

        let controller = ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let exit_code = run_iteration_loop(config, &controller);
        assert!(matches!(exit_code, IterExitCode::Complete));

        let ev = events.lock().unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].0, "start", "start should be called first");
        assert_eq!(ev[0].1, 1);
        assert_eq!(ev[0].2, "start-test-uuid");
        assert_eq!(ev[1].0, "complete", "complete should be called second");
        assert_eq!(ev[1].1, 1);
        assert_eq!(ev[1].2, "start-test-uuid");
    }
}
