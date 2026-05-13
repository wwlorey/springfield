use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use shutdown::{ShutdownController, ShutdownStatus, kill_process_group};
use tracing::warn;

use super::AgentExitStatus;

fn open_pty() -> io::Result<(OwnedFd, OwnedFd)> {
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::grantpt(master) != 0 {
            let e = io::Error::last_os_error();
            libc::close(master);
            return Err(e);
        }
        if libc::unlockpt(master) != 0 {
            let e = io::Error::last_os_error();
            libc::close(master);
            return Err(e);
        }
        let slave_name = libc::ptsname(master);
        if slave_name.is_null() {
            let e = io::Error::last_os_error();
            libc::close(master);
            return Err(e);
        }
        let slave = libc::open(slave_name, libc::O_RDWR | libc::O_NOCTTY);
        if slave < 0 {
            let e = io::Error::last_os_error();
            libc::close(master);
            return Err(e);
        }
        Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)))
    }
}

fn dup_fd(fd: &OwnedFd) -> io::Result<OwnedFd> {
    let new_fd = unsafe { libc::dup(fd.as_raw_fd()) };
    if new_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(new_fd) })
}

fn copy_winsize(from: RawFd, to: RawFd) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(from, libc::TIOCGWINSZ, &mut ws) == 0 {
            libc::ioctl(to, libc::TIOCSWINSZ, &ws);
        }
    }
}

fn enter_raw_mode(fd: RawFd) -> io::Result<libc::termios> {
    unsafe {
        let mut orig: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut orig) != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = orig;
        libc::cfmakeraw(&mut raw);
        if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(orig)
    }
}

fn restore_termios(fd: RawFd, termios: &libc::termios) {
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, termios);
    }
}

struct ScreenScraper {
    parser: vt100::Parser,
    prev_lines: Vec<String>,
}

impl ScreenScraper {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
            prev_lines: Vec::new(),
        }
    }

    fn process(&mut self, data: &[u8]) -> String {
        self.parser.process(data);
        let (_, cols) = self.parser.screen().size();
        let curr_lines: Vec<String> = self
            .parser
            .screen()
            .rows(0, cols)
            .map(|l| l.trim_end().to_string())
            .collect();

        if curr_lines == self.prev_lines {
            return String::new();
        }

        let result = extract_new_content(&self.prev_lines, &curr_lines);
        self.prev_lines = curr_lines;
        result
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        // The app will repaint at the new size, invalidating prev_lines.
        // Clear so the next frame diffs against nothing rather than stale
        // old-layout lines. This may re-log some content — acceptable since
        // resizes are rare.
        self.prev_lines.clear();
    }
}

/// Dedup uses exact string comparison after trim_end(). Lines whose only
/// difference is trailing whitespace are always deduped. Internal whitespace
/// changes (e.g. column realignment after resize) are treated as new content
/// and logged, which is the correct behavior for layout changes.
fn extract_new_content(prev: &[String], curr: &[String]) -> String {
    if prev.is_empty() {
        return curr
            .iter()
            .filter(|l| !l.is_empty() && !is_chrome(l))
            .map(|l| format!("{l}\n"))
            .collect();
    }

    if let Some(scroll) = detect_scroll(prev, curr) {
        return curr[curr.len().saturating_sub(scroll)..]
            .iter()
            .filter(|l| !l.is_empty() && !is_chrome(l))
            .map(|l| format!("{l}\n"))
            .collect();
    }

    let prev_set: HashSet<&str> = prev
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect();

    curr.iter()
        .filter(|l| !l.is_empty() && !is_chrome(l) && !prev_set.contains(l.as_str()))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn detect_scroll(prev: &[String], curr: &[String]) -> Option<usize> {
    let n = prev.len().min(curr.len());
    if n < 2 {
        return None;
    }
    for scroll in 1..=5 {
        if scroll >= n {
            break;
        }
        if prev[scroll..n] == curr[..n - scroll] {
            return Some(scroll);
        }
    }
    None
}

fn is_chrome(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    if t.chars().all(|c| "─═│╭╮╰╯┌┐└┘├┤┬┴┼━┃╋ ".contains(c)) {
        return true;
    }
    if t.chars().count() <= 2 && t.chars().all(|c| !c.is_ascii_alphanumeric()) {
        return true;
    }
    false
}

fn drain_master(
    master_fd: RawFd,
    log_file: &Option<Mutex<fs::File>>,
    scraper: &mut Option<ScreenScraper>,
) {
    let mut buf = [0u8; 4096];
    loop {
        let mut fds = [libc::pollfd {
            fd: master_fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 50) };
        if ret <= 0 {
            break;
        }
        if fds[0].revents & libc::POLLIN == 0 {
            break;
        }
        let n = unsafe { libc::read(master_fd, buf.as_mut_ptr() as _, buf.len()) };
        if n <= 0 {
            break;
        }
        let data = &buf[..n as usize];
        let _ = io::stdout().write_all(data);
        let _ = io::stdout().flush();
        if let Some(s) = scraper.as_mut() {
            let output = s.process(data);
            if !output.is_empty()
                && let Some(lf) = log_file
                && let Ok(mut f) = lf.lock()
            {
                let _ = f.write_all(output.as_bytes());
            }
        }
    }
}

pub(crate) fn run_interactive_with_pty(
    command: &mut Command,
    log_path: Option<&Path>,
    controller: &ShutdownController,
) -> io::Result<AgentExitStatus> {
    let (master, slave) = open_pty()?;
    let master_fd = master.as_raw_fd();

    copy_winsize(libc::STDIN_FILENO, master_fd);

    let slave_in = dup_fd(&slave)?;
    let slave_out = dup_fd(&slave)?;
    let slave_err = dup_fd(&slave)?;
    drop(slave);

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    command
        .stdin(Stdio::from(slave_in))
        .stdout(Stdio::from(slave_out))
        .stderr(Stdio::from(slave_err));

    let log_file: Option<Mutex<fs::File>> = match log_path {
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

    let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } == 1;
    let original_termios = if is_tty {
        Some(enter_raw_mode(libc::STDIN_FILENO)?)
    } else {
        None
    };

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            if let Some(ref t) = original_termios {
                restore_termios(libc::STDIN_FILENO, t);
            }
            return Err(e);
        }
    };

    let stdin_fd = libc::STDIN_FILENO;
    let mut buf = [0u8; 4096];
    let mut last_ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(stdin_fd, libc::TIOCGWINSZ, &mut last_ws) };

    let mut scraper = if log_file.is_some() {
        let rows = if last_ws.ws_row > 0 {
            last_ws.ws_row
        } else {
            24
        };
        let cols = if last_ws.ws_col > 0 {
            last_ws.ws_col
        } else {
            80
        };
        Some(ScreenScraper::new(rows, cols))
    } else {
        None
    };

    let mut ctrl_c_forwarded = false;
    let exit_code;
    loop {
        if controller.poll() == ShutdownStatus::Shutdown {
            kill_process_group(child.id(), Duration::from_millis(200));
            let _ = child.wait();
            exit_code = None;
            break;
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                drain_master(master_fd, &log_file, &mut scraper);
                exit_code = status.code();
                break;
            }
            Ok(None) => {}
            Err(e) => {
                warn!(error = %e, "error waiting for child process");
                exit_code = None;
                break;
            }
        }

        let mut fds = [
            libc::pollfd {
                fd: stdin_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };

        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            warn!(error = %err, "poll error");
            exit_code = None;
            break;
        }

        if is_tty {
            let mut current_ws: libc::winsize = unsafe { std::mem::zeroed() };
            if unsafe { libc::ioctl(stdin_fd, libc::TIOCGWINSZ, &mut current_ws) } == 0
                && (current_ws.ws_row != last_ws.ws_row || current_ws.ws_col != last_ws.ws_col)
            {
                unsafe { libc::ioctl(master_fd, libc::TIOCSWINSZ, &current_ws) };
                if let Some(s) = &mut scraper {
                    s.resize(current_ws.ws_row, current_ws.ws_col);
                }
                last_ws = current_ws;
            }
        }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as _, buf.len()) };
            if n > 0 {
                if buf[..n as usize].contains(&0x03) {
                    ctrl_c_forwarded = true;
                }
                let _ = unsafe { libc::write(master_fd, buf.as_ptr() as _, n as usize) };
            }
        }

        if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let n = unsafe { libc::read(master_fd, buf.as_mut_ptr() as _, buf.len()) };
            if n > 0 {
                let data = &buf[..n as usize];
                let _ = io::stdout().write_all(data);
                let _ = io::stdout().flush();
                if let Some(s) = &mut scraper {
                    let output = s.process(data);
                    if !output.is_empty()
                        && let Some(lf) = &log_file
                        && let Ok(mut f) = lf.lock()
                    {
                        let _ = f.write_all(output.as_bytes());
                    }
                }
            } else if n == 0 || (n < 0 && fds[1].revents & libc::POLLHUP != 0) {
                drain_master(master_fd, &log_file, &mut scraper);
                match child.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = status.code();
                        break;
                    }
                    _ => {
                        let status = child.wait();
                        exit_code = status.ok().and_then(|s| s.code());
                        break;
                    }
                }
            }
        }

        if fds[1].revents & libc::POLLERR != 0 && fds[1].revents & libc::POLLIN == 0 {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_code = status.code();
                    break;
                }
                _ => {
                    let status = child.wait();
                    exit_code = status.ok().and_then(|s| s.code());
                    break;
                }
            }
        }
    }

    if let Some(ref t) = original_termios {
        restore_termios(libc::STDIN_FILENO, t);
    }

    Ok(AgentExitStatus {
        exit_code,
        killed_by_timeout: false,
        ctrl_c_forwarded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_pty_returns_valid_fds() {
        let result = open_pty();
        assert!(
            result.is_ok(),
            "open_pty should succeed: {:?}",
            result.err()
        );
        let (master, slave) = result.unwrap();
        assert!(master.as_raw_fd() >= 0);
        assert!(slave.as_raw_fd() >= 0);
        assert_ne!(master.as_raw_fd(), slave.as_raw_fd());
    }

    #[test]
    fn dup_fd_returns_different_fd() {
        let (master, _slave) = open_pty().unwrap();
        let duped = dup_fd(&master).unwrap();
        assert_ne!(master.as_raw_fd(), duped.as_raw_fd());
    }

    #[test]
    fn copy_winsize_does_not_panic() {
        let (master, slave) = open_pty().unwrap();
        copy_winsize(slave.as_raw_fd(), master.as_raw_fd());
    }

    #[test]
    fn pty_tee_captures_output_to_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");

        let mut cmd = Command::new("echo");
        cmd.arg("hello from pty");

        let controller = ShutdownController::new(shutdown::ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_interactive_with_pty(&mut cmd, Some(&log_path), &controller);
        assert!(result.is_ok(), "pty tee should succeed: {:?}", result.err());

        let status = result.unwrap();
        assert_eq!(status.exit_code, Some(0));

        let log_content = fs::read_to_string(&log_path).unwrap();
        assert!(
            log_content.contains("hello from pty"),
            "log should contain output, got: {log_content:?}"
        );
    }

    #[test]
    fn pty_tee_strips_ansi_from_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");

        let mut cmd = Command::new("sh");
        cmd.args(["-c", r#"printf '\x1b[1;32mgreen bold\x1b[0m plain\n'"#]);

        let controller = ShutdownController::new(shutdown::ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_interactive_with_pty(&mut cmd, Some(&log_path), &controller).unwrap();
        assert_eq!(result.exit_code, Some(0));

        let log_content = fs::read_to_string(&log_path).unwrap();
        assert!(
            log_content.contains("green bold"),
            "log should contain text, got: {log_content:?}"
        );
        assert!(
            !log_content.contains("\x1b["),
            "log should not contain ANSI escapes, got: {log_content:?}"
        );
    }

    #[test]
    fn pty_tee_captures_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fail.sh");
        fs::write(&script, "#!/bin/sh\nexit 42\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let mut cmd = Command::new(script.to_str().unwrap());
        let controller = ShutdownController::new(shutdown::ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_interactive_with_pty(&mut cmd, None, &controller).unwrap();
        assert_eq!(result.exit_code, Some(42));
    }

    #[test]
    fn pty_tee_no_log_file_still_works() {
        let mut cmd = Command::new("true");
        let controller = ShutdownController::new(shutdown::ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_interactive_with_pty(&mut cmd, None, &controller).unwrap();
        assert_eq!(result.exit_code, Some(0));
    }

    #[test]
    fn pty_child_sees_tty() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("tty.log");

        let mut cmd = Command::new("sh");
        cmd.args(["-c", "test -t 0 && echo 'is_tty=yes' || echo 'is_tty=no'"]);

        let controller = ShutdownController::new(shutdown::ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap();

        let result = run_interactive_with_pty(&mut cmd, Some(&log_path), &controller).unwrap();
        assert_eq!(result.exit_code, Some(0));

        let log_content = fs::read_to_string(&log_path).unwrap();
        assert!(
            log_content.contains("is_tty=yes"),
            "child should see a TTY, got: {log_content:?}"
        );
    }

    #[test]
    fn scraper_simple_text() {
        let mut s = ScreenScraper::new(24, 80);
        let out = s.process(b"hello world\r\n");
        assert!(out.contains("hello world"), "got: {out:?}");
    }

    #[test]
    fn scraper_dedup_identical_frames() {
        let mut s = ScreenScraper::new(24, 80);
        let _ = s.process(b"hello\r\n");
        let out = s.process(b"\x1b[H\x1b[2Jhello\r\n");
        assert!(
            !out.contains("hello"),
            "duplicate frame should produce no output, got: {out:?}"
        );
    }

    #[test]
    fn scraper_cursor_positioned_overwrite() {
        let mut s = ScreenScraper::new(24, 80);
        s.process(b"old text\r\n");
        let out = s.process(b"\x1b[1;1Hnew text");
        assert!(out.contains("new text"), "got: {out:?}");
        assert!(
            !out.contains("old text"),
            "old text should not reappear, got: {out:?}"
        );
    }

    #[test]
    fn scraper_filters_separator_lines() {
        let mut s = ScreenScraper::new(24, 80);
        let out = s.process("─────────────────\r\n".as_bytes());
        assert!(
            out.is_empty(),
            "separator line should be filtered, got: {out:?}"
        );
    }

    #[test]
    fn scraper_filters_short_nonalpha() {
        let mut s = ScreenScraper::new(24, 80);
        // Single spinner character
        let out = s.process("✽\r\n".as_bytes());
        assert!(
            out.is_empty(),
            "spinner char should be filtered, got: {out:?}"
        );
    }

    #[test]
    fn scraper_preserves_real_content_with_ansi() {
        let mut s = ScreenScraper::new(24, 80);
        let out = s.process(b"\x1b[1;32mgreen bold\x1b[0m plain\r\n");
        assert!(out.contains("green bold"), "got: {out:?}");
        assert!(out.contains("plain"), "got: {out:?}");
        assert!(
            !out.contains("\x1b["),
            "ANSI codes should be stripped, got: {out:?}"
        );
    }

    #[test]
    fn scraper_scroll_logs_new_line_only() {
        let mut s = ScreenScraper::new(4, 40);
        s.process(b"line1\r\nline2\r\nline3\r\nline4\r\n");
        // Now add a new line, pushing line1 off the top
        let out = s.process(b"\x1b[Hline2\r\nline3\r\nline4\r\nline5\r\n");
        assert!(
            out.contains("line5"),
            "new line should appear, got: {out:?}"
        );
        assert!(
            !out.contains("line2"),
            "old lines should not repeat, got: {out:?}"
        );
        assert!(
            !out.contains("line3"),
            "old lines should not repeat, got: {out:?}"
        );
    }

    #[test]
    fn scraper_major_redraw_logs_new_content() {
        let mut s = ScreenScraper::new(24, 80);
        s.process(b"first screen content\r\n");
        let out = s.process(b"\x1b[H\x1b[2Jcompletely different\r\n");
        assert!(out.contains("completely different"), "got: {out:?}");
        assert!(
            !out.contains("first screen"),
            "old content should not repeat, got: {out:?}"
        );
    }

    #[test]
    fn scraper_resize_does_not_panic() {
        let mut s = ScreenScraper::new(24, 80);
        s.process(b"hello\r\n");
        s.resize(40, 120);
        let out = s.process(b"after resize\r\n");
        assert!(out.contains("after resize"), "got: {out:?}");
    }

    #[test]
    fn is_chrome_rejects_separators() {
        assert!(is_chrome("─────────────"));
        assert!(is_chrome("  ─══━━━  "));
        assert!(is_chrome("╭──────╮"));
    }

    #[test]
    fn is_chrome_preserves_real_text() {
        assert!(!is_chrome("hello world"));
        assert!(!is_chrome("cargo build -p foo"));
        assert!(!is_chrome("│ sgf │ launching"));
        assert!(!is_chrome("error[E0308]: mismatched types"));
    }

    #[test]
    fn is_chrome_rejects_short_special() {
        assert!(is_chrome("✽"));
        assert!(is_chrome("◐"));
        assert!(is_chrome("⠋"));
    }

    #[test]
    fn detect_scroll_single_line() {
        let prev = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let curr = vec!["B".into(), "C".into(), "D".into(), "E".into()];
        assert_eq!(detect_scroll(&prev, &curr), Some(1));
    }

    #[test]
    fn detect_scroll_no_scroll() {
        let prev = vec!["A".into(), "B".into()];
        let curr = vec!["X".into(), "Y".into()];
        assert_eq!(detect_scroll(&prev, &curr), None);
    }

    #[test]
    fn detect_scroll_multi_line() {
        let prev = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        let curr = vec!["C".into(), "D".into(), "E".into(), "F".into()];
        assert_eq!(detect_scroll(&prev, &curr), Some(2));
    }
}
