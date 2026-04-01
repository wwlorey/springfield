pub mod banner;
pub mod format;
pub mod style;

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

pub const SENTINEL: &str = ".iter-complete";
pub const SENTINEL_MAX_DEPTH: usize = 2;
const DING_SENTINEL: &str = ".iter-ding";
const MAX_ITERATIONS: u32 = 1000;
const DEFAULT_ITER_DELAY_MS: u64 = 2000;
const DEFAULT_POST_RESULT_TIMEOUT_SECS: u64 = 30;

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

pub type IterationCallback = Box<dyn FnMut(u32, &str)>;

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
    /// Called after each iteration completes with (iteration_number, session_id).
    pub on_iteration_complete: Option<IterationCallback>,
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
) {
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
    config: &IterRunnerConfig,
    is_file: bool,
    controller: &ShutdownController,
    tee: &TeeWriter,
    iteration: u32,
    session_id: &str,
) {
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
    let mut result_received_at: Option<std::time::Instant> = None;
    let post_result_timeout = config.post_result_timeout;

    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            kill_process_group(child_pid, Duration::from_millis(200));
            let _ = child.wait();
            let _ = reader_handle.join();
            return;
        }

        if let Some(received_at) = result_received_at
            && received_at.elapsed() > post_result_timeout
        {
            warn!(
                elapsed_secs = received_at.elapsed().as_secs(),
                "agent process did not exit after result event, killing"
            );
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

    let root = config.work_dir.as_deref().unwrap_or_else(|| Path::new("."));

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

        let head_before = vcs_utils::git_head();

        if config.afk {
            run_afk(
                &agent_cmd,
                &config,
                is_file,
                controller,
                &tee,
                i,
                &iter_session_id,
            );
        } else {
            run_interactive(
                &agent_cmd,
                &config,
                is_file,
                controller,
                i,
                &iter_session_id,
            );
        }

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
            on_iteration_complete: None,
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
}
