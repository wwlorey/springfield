use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::Local;

pub fn generate_loop_id(stage: &str, spec: Option<&str>) -> String {
    let ts = Local::now().format("%Y%m%dT%H%M%S");
    match spec {
        Some(s) => format!("{stage}-{s}-{ts}"),
        None => format!("{stage}-{ts}"),
    }
}

pub fn write_pid_file(root: &Path, loop_id: &str) -> io::Result<PathBuf> {
    let pid_path = root.join(".sgf/run").join(format!("{loop_id}.pid"));
    fs::create_dir_all(pid_path.parent().unwrap())?;
    fs::write(&pid_path, std::process::id().to_string())?;
    Ok(pid_path)
}

pub fn remove_pid_file(root: &Path, loop_id: &str) {
    let pid_path = root.join(".sgf/run").join(format!("{loop_id}.pid"));
    let _ = fs::remove_file(pid_path);
}

pub fn list_pid_files(root: &Path) -> Vec<(String, u32)> {
    let run_dir = root.join(".sgf/run");
    let entries = match fs::read_dir(&run_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("pid")
            && let Ok(contents) = fs::read_to_string(&path)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            let loop_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            results.push((loop_id, pid));
        }
    }
    results
}

pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn create_log_file(root: &Path, loop_id: &str) -> io::Result<PathBuf> {
    let log_path = root.join(".sgf/logs").join(format!("{loop_id}.log"));
    fs::create_dir_all(log_path.parent().unwrap())?;
    fs::File::create(&log_path)?;
    Ok(log_path)
}

pub fn tee_output<R: io::Read>(reader: R, log_path: &Path) -> io::Result<()> {
    let mut log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let buf_reader = io::BufReader::new(reader);
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in buf_reader.lines() {
        let line = line?;
        writeln!(stdout_lock, "{line}")?;
        writeln!(log_file, "{line}")?;
    }
    Ok(())
}

pub fn run_logs(root: &Path, loop_id: &str) -> io::Result<()> {
    let log_path = root.join(".sgf/logs").join(format!("{loop_id}.log"));
    if !log_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("log file not found: {}", log_path.display()),
        ));
    }

    let status = std::process::Command::new("tail")
        .args(["-f", &log_path.to_string_lossy()])
        .status()?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "tail exited with status: {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn loop_id_with_spec() {
        let id = generate_loop_id("build", Some("auth"));
        assert!(id.starts_with("build-auth-"));
        let ts_part = id.strip_prefix("build-auth-").unwrap();
        assert_eq!(ts_part.len(), 15); // YYYYMMDDTHHmmss
        assert!(ts_part.contains('T'));
    }

    #[test]
    fn loop_id_without_spec() {
        let id = generate_loop_id("verify", None);
        assert!(id.starts_with("verify-"));
        let ts_part = id.strip_prefix("verify-").unwrap();
        assert_eq!(ts_part.len(), 15);
    }

    #[test]
    fn loop_id_compound_stage() {
        let id = generate_loop_id("issues-plan", None);
        assert!(id.starts_with("issues-plan-"));
        let ts_part = id.strip_prefix("issues-plan-").unwrap();
        assert_eq!(ts_part.len(), 15);
    }

    #[test]
    fn pid_file_write_and_read() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let pid_path = write_pid_file(root, "test-loop").unwrap();
        assert!(pid_path.exists());

        let contents = fs::read_to_string(&pid_path).unwrap();
        let pid: u32 = contents.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn pid_file_remove() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let pid_path = write_pid_file(root, "test-loop").unwrap();
        assert!(pid_path.exists());

        remove_pid_file(root, "test-loop");
        assert!(!pid_path.exists());
    }

    #[test]
    fn remove_nonexistent_pid_file_is_noop() {
        let tmp = TempDir::new().unwrap();
        remove_pid_file(tmp.path(), "nonexistent");
    }

    #[test]
    fn list_pid_files_empty() {
        let tmp = TempDir::new().unwrap();
        let results = list_pid_files(tmp.path());
        assert!(results.is_empty());
    }

    #[test]
    fn list_pid_files_finds_entries() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(run_dir.join("build-auth-20260226T143000.pid"), "12345").unwrap();
        fs::write(run_dir.join("verify-20260226T150000.pid"), "67890").unwrap();

        let mut results = list_pid_files(root);
        results.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "build-auth-20260226T143000");
        assert_eq!(results[0].1, 12345);
        assert_eq!(results[1].0, "verify-20260226T150000");
        assert_eq!(results[1].1, 67890);
    }

    #[test]
    fn list_pid_files_skips_non_pid() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(run_dir.join("build-auth.pid"), "12345").unwrap();
        fs::write(run_dir.join("not-a-pid.txt"), "67890").unwrap();
        fs::write(run_dir.join("bad.pid"), "not-a-number").unwrap();

        let results = list_pid_files(root);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "build-auth");
        assert_eq!(results[0].1, 12345);
    }

    #[test]
    fn is_own_pid_alive() {
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn is_dead_pid_not_alive() {
        // PID 4_000_000 is extremely unlikely to exist
        assert!(!is_pid_alive(4_000_000));
    }

    #[test]
    fn create_log_file_creates_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let log_path = create_log_file(root, "build-auth-20260226T143000").unwrap();
        assert!(log_path.exists());
        assert_eq!(
            log_path,
            root.join(".sgf/logs/build-auth-20260226T143000.log")
        );
    }

    #[test]
    fn tee_output_writes_to_both() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");

        let input = b"line one\nline two\n";
        tee_output(&input[..], &log_path).unwrap();

        let log_contents = fs::read_to_string(&log_path).unwrap();
        assert!(log_contents.contains("line one"));
        assert!(log_contents.contains("line two"));
    }

    #[test]
    fn run_logs_missing_file() {
        let tmp = TempDir::new().unwrap();
        let err = run_logs(tmp.path(), "nonexistent").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("log file not found"));
    }
}
