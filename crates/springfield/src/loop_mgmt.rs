use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IterationRecord {
    pub iteration: u32,
    pub session_id: String,
    pub completed_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionMetadata {
    pub loop_id: String,
    pub iterations: Vec<IterationRecord>,
    pub stage: String,
    pub spec: Option<String>,
    pub mode: String,
    pub prompt: String,
    pub iterations_total: u32,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn write_session_metadata(root: &Path, metadata: &SessionMetadata) -> io::Result<()> {
    let run_dir = root.join(".sgf/run");
    fs::create_dir_all(&run_dir)?;
    let target = run_dir.join(format!("{}.json", metadata.loop_id));
    let tmp = run_dir.join(format!("{}.json.tmp", metadata.loop_id));
    let json = serde_json::to_string_pretty(metadata)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &target)?;
    Ok(())
}

pub fn read_session_metadata(root: &Path, loop_id: &str) -> io::Result<Option<SessionMetadata>> {
    let path = root.join(".sgf/run").join(format!("{loop_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)?;
    match serde_json::from_str::<SessionMetadata>(&contents) {
        Ok(m) => Ok(Some(m)),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

pub fn list_session_metadata(root: &Path) -> io::Result<Vec<SessionMetadata>> {
    let run_dir = root.join(".sgf/run");
    let entries = match fs::read_dir(&run_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(meta) = serde_json::from_str::<SessionMetadata>(&contents) {
            sessions.push(meta);
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

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
    fn run_logs_missing_file() {
        let tmp = TempDir::new().unwrap();
        let err = run_logs(tmp.path(), "nonexistent").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("log file not found"));
    }

    fn make_metadata(loop_id: &str, updated_at: &str) -> SessionMetadata {
        SessionMetadata {
            loop_id: loop_id.to_string(),
            iterations: vec![IterationRecord {
                iteration: 1,
                session_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
                completed_at: "2026-03-16T12:02:30Z".to_string(),
            }],
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: "interactive".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "completed".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    #[test]
    fn session_metadata_write_and_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let meta = make_metadata("build-auth-20260316T120000", "2026-03-16T12:05:30Z");

        write_session_metadata(root, &meta).unwrap();

        let json_path = root.join(".sgf/run/build-auth-20260316T120000.json");
        assert!(json_path.exists());

        let read_back = read_session_metadata(root, "build-auth-20260316T120000")
            .unwrap()
            .expect("should return Some");
        assert_eq!(read_back.loop_id, meta.loop_id);
        assert_eq!(read_back.iterations.len(), 1);
        assert_eq!(read_back.iterations[0].iteration, 1);
        assert_eq!(
            read_back.iterations[0].session_id,
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
        assert_eq!(read_back.iterations[0].completed_at, "2026-03-16T12:02:30Z");
        assert_eq!(read_back.stage, meta.stage);
        assert_eq!(read_back.spec, meta.spec);
        assert_eq!(read_back.mode, meta.mode);
        assert_eq!(read_back.prompt, meta.prompt);
        assert_eq!(read_back.iterations_total, meta.iterations_total);
        assert_eq!(read_back.status, meta.status);
        assert_eq!(read_back.created_at, meta.created_at);
        assert_eq!(read_back.updated_at, meta.updated_at);
    }

    #[test]
    fn list_sessions_sorted_by_updated_at_desc() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let older = make_metadata("build-auth-20260316T100000", "2026-03-16T10:00:00Z");
        let newer = make_metadata("spec-20260316T120000", "2026-03-16T12:00:00Z");
        let middle = make_metadata("verify-20260316T110000", "2026-03-16T11:00:00Z");

        write_session_metadata(root, &older).unwrap();
        write_session_metadata(root, &newer).unwrap();
        write_session_metadata(root, &middle).unwrap();

        let sessions = list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].loop_id, "spec-20260316T120000");
        assert_eq!(sessions[1].loop_id, "verify-20260316T110000");
        assert_eq!(sessions[2].loop_id, "build-auth-20260316T100000");
    }

    #[test]
    fn list_sessions_skips_corrupt_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_dir = root.join(".sgf/run");
        fs::create_dir_all(&run_dir).unwrap();

        let valid = make_metadata("build-auth-20260316T120000", "2026-03-16T12:00:00Z");
        write_session_metadata(root, &valid).unwrap();

        fs::write(
            run_dir.join("corrupt-20260316T130000.json"),
            "not valid json{{{",
        )
        .unwrap();

        let sessions = list_session_metadata(root).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].loop_id, "build-auth-20260316T120000");
    }

    #[test]
    fn list_sessions_returns_empty_when_no_json_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        fs::write(root.join(".sgf/run/something.pid"), "12345").unwrap();

        let sessions = list_session_metadata(root).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn list_sessions_returns_empty_when_run_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let sessions = list_session_metadata(tmp.path()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn read_metadata_nonexistent_loop_id() {
        let tmp = TempDir::new().unwrap();
        let result = read_session_metadata(tmp.path(), "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_metadata_atomic_no_tmp_left_behind() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let meta = make_metadata("build-auth-20260316T120000", "2026-03-16T12:00:00Z");

        write_session_metadata(root, &meta).unwrap();

        let run_dir = root.join(".sgf/run");
        let tmp_file = run_dir.join("build-auth-20260316T120000.json.tmp");
        assert!(!tmp_file.exists(), "tmp file should be renamed away");
        assert!(run_dir.join("build-auth-20260316T120000.json").exists());
    }

    #[test]
    fn iterations_len_replaces_iterations_completed() {
        let meta = make_metadata("test-loop", "2026-03-16T12:00:00Z");
        assert_eq!(meta.iterations.len(), 1);

        let mut meta2 = meta;
        meta2.iterations.push(IterationRecord {
            iteration: 2,
            session_id: "bbbbbbbb-cccc-dddd-eeee-ffffffffffff".to_string(),
            completed_at: "2026-03-16T12:05:30Z".to_string(),
        });
        assert_eq!(meta2.iterations.len(), 2);
    }

    #[test]
    fn empty_iterations_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let meta = SessionMetadata {
            loop_id: "empty-iter-loop".to_string(),
            iterations: Vec::new(),
            stage: "build".to_string(),
            spec: None,
            mode: "afk".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 5,
            status: "running".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:00:00Z".to_string(),
        };

        write_session_metadata(root, &meta).unwrap();
        let read_back = read_session_metadata(root, "empty-iter-loop")
            .unwrap()
            .unwrap();
        assert!(read_back.iterations.is_empty());
        assert_eq!(read_back.status, "running");
    }

    #[test]
    fn multiple_iterations_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let meta = SessionMetadata {
            loop_id: "multi-iter-loop".to_string(),
            iterations: vec![
                IterationRecord {
                    iteration: 1,
                    session_id: "aaaa-1111".to_string(),
                    completed_at: "2026-03-16T12:01:00Z".to_string(),
                },
                IterationRecord {
                    iteration: 2,
                    session_id: "bbbb-2222".to_string(),
                    completed_at: "2026-03-16T12:03:00Z".to_string(),
                },
                IterationRecord {
                    iteration: 3,
                    session_id: "cccc-3333".to_string(),
                    completed_at: "2026-03-16T12:05:00Z".to_string(),
                },
            ],
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            mode: "afk".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 5,
            status: "exhausted".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:05:00Z".to_string(),
        };

        write_session_metadata(root, &meta).unwrap();
        let read_back = read_session_metadata(root, "multi-iter-loop")
            .unwrap()
            .unwrap();
        assert_eq!(read_back.iterations.len(), 3);
        assert_eq!(read_back.iterations[0].iteration, 1);
        assert_eq!(read_back.iterations[0].session_id, "aaaa-1111");
        assert_eq!(read_back.iterations[1].iteration, 2);
        assert_eq!(read_back.iterations[1].session_id, "bbbb-2222");
        assert_eq!(read_back.iterations[2].iteration, 3);
        assert_eq!(read_back.iterations[2].session_id, "cccc-3333");
    }

    #[test]
    fn append_iteration_via_write() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let mut meta = SessionMetadata {
            loop_id: "append-test".to_string(),
            iterations: Vec::new(),
            stage: "build".to_string(),
            spec: None,
            mode: "afk".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "running".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:00:00Z".to_string(),
        };
        write_session_metadata(root, &meta).unwrap();

        meta.iterations.push(IterationRecord {
            iteration: 1,
            session_id: "uuid-iter-1".to_string(),
            completed_at: "2026-03-16T12:01:00Z".to_string(),
        });
        meta.updated_at = "2026-03-16T12:01:00Z".to_string();
        write_session_metadata(root, &meta).unwrap();

        let read_back = read_session_metadata(root, "append-test")
            .unwrap()
            .unwrap();
        assert_eq!(read_back.iterations.len(), 1);
        assert_eq!(read_back.iterations[0].session_id, "uuid-iter-1");

        meta.iterations.push(IterationRecord {
            iteration: 2,
            session_id: "uuid-iter-2".to_string(),
            completed_at: "2026-03-16T12:02:00Z".to_string(),
        });
        meta.updated_at = "2026-03-16T12:02:00Z".to_string();
        write_session_metadata(root, &meta).unwrap();

        let read_back = read_session_metadata(root, "append-test")
            .unwrap()
            .unwrap();
        assert_eq!(read_back.iterations.len(), 2);
        assert_eq!(read_back.iterations[1].session_id, "uuid-iter-2");
    }
}
