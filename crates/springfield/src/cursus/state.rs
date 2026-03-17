use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Completed,
    Stalled,
    Interrupted,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Stalled => write!(f, "stalled"),
            Self::Interrupted => write!(f, "interrupted"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedIter {
    pub name: String,
    pub session_id: String,
    pub completed_at: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    pub cursus: String,
    pub status: RunStatus,
    pub current_iter: String,
    pub current_iter_index: u32,
    pub iters_completed: Vec<CompletedIter>,
    pub spec: Option<String>,
    pub mode_override: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl RunMetadata {
    pub fn new(
        cursus_name: &str,
        first_iter: &str,
        spec: Option<&str>,
        mode_override: Option<&str>,
    ) -> Self {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let run_id = generate_run_id(cursus_name);
        Self {
            run_id,
            cursus: cursus_name.to_string(),
            status: RunStatus::Running,
            current_iter: first_iter.to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: spec.map(|s| s.to_string()),
            mode_override: mode_override.map(|m| m.to_string()),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
}

pub fn generate_run_id(cursus_name: &str) -> String {
    let ts = Local::now().format("%Y%m%dT%H%M%S");
    format!("{cursus_name}-{ts}")
}

pub fn run_dir(root: &Path, run_id: &str) -> PathBuf {
    root.join(".sgf/run").join(run_id)
}

pub fn context_dir(root: &Path, run_id: &str) -> PathBuf {
    run_dir(root, run_id).join("context")
}

pub fn meta_path(root: &Path, run_id: &str) -> PathBuf {
    run_dir(root, run_id).join("meta.json")
}

pub fn pid_path(root: &Path, run_id: &str) -> PathBuf {
    run_dir(root, run_id).join(format!("{run_id}.pid"))
}

pub fn create_run_dir(root: &Path, run_id: &str) -> io::Result<PathBuf> {
    let dir = run_dir(root, run_id);
    let ctx = context_dir(root, run_id);
    fs::create_dir_all(&ctx)?;
    Ok(dir)
}

pub fn write_metadata(root: &Path, metadata: &RunMetadata) -> io::Result<()> {
    let dir = run_dir(root, &metadata.run_id);
    fs::create_dir_all(&dir)?;
    let target = meta_path(root, &metadata.run_id);
    let tmp = dir.join("meta.json.tmp");
    let json = serde_json::to_string_pretty(metadata)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &target)?;
    Ok(())
}

pub fn read_metadata(root: &Path, run_id: &str) -> io::Result<Option<RunMetadata>> {
    let path = meta_path(root, run_id);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path)?;
    let meta = serde_json::from_str::<RunMetadata>(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(meta))
}

pub fn write_pid_file(root: &Path, run_id: &str) -> io::Result<PathBuf> {
    let path = pid_path(root, run_id);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, std::process::id().to_string())?;
    Ok(path)
}

pub fn remove_pid_file(root: &Path, run_id: &str) {
    let _ = fs::remove_file(pid_path(root, run_id));
}

pub fn read_pid(root: &Path, run_id: &str) -> Option<u32> {
    let path = pid_path(root, run_id);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn is_stale_run(root: &Path, run_id: &str) -> io::Result<bool> {
    let meta = match read_metadata(root, run_id)? {
        Some(m) => m,
        None => return Ok(false),
    };
    if meta.status != RunStatus::Running {
        return Ok(false);
    }
    match read_pid(root, run_id) {
        Some(pid) => Ok(!is_pid_alive(pid)),
        None => Ok(true),
    }
}

pub fn mark_stale_runs_interrupted(root: &Path) -> io::Result<Vec<String>> {
    let run_base = root.join(".sgf/run");
    let entries = match fs::read_dir(&run_base) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut marked = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let run_id = entry
            .file_name()
            .to_str()
            .unwrap_or("")
            .to_string();
        if run_id.is_empty() {
            continue;
        }
        if is_stale_run(root, &run_id)? {
            if let Some(mut meta) = read_metadata(root, &run_id)? {
                meta.status = RunStatus::Interrupted;
                meta.touch();
                write_metadata(root, &meta)?;
                remove_pid_file(root, &run_id);
                marked.push(run_id);
            }
        }
    }
    Ok(marked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn run_id_format() {
        let id = generate_run_id("spec");
        assert!(id.starts_with("spec-"));
        let ts = id.strip_prefix("spec-").unwrap();
        assert_eq!(ts.len(), 15);
        assert!(ts.contains('T'));
    }

    #[test]
    fn new_metadata_defaults() {
        let meta = RunMetadata::new("build", "compile", Some("auth"), None);
        assert!(meta.run_id.starts_with("build-"));
        assert_eq!(meta.cursus, "build");
        assert_eq!(meta.status, RunStatus::Running);
        assert_eq!(meta.current_iter, "compile");
        assert_eq!(meta.current_iter_index, 0);
        assert!(meta.iters_completed.is_empty());
        assert_eq!(meta.spec.as_deref(), Some("auth"));
        assert!(meta.mode_override.is_none());
        assert_eq!(meta.created_at, meta.updated_at);
    }

    #[test]
    fn metadata_serialization_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let meta = RunMetadata {
            run_id: "spec-20260317T140000".to_string(),
            cursus: "spec".to_string(),
            status: RunStatus::Running,
            current_iter: "draft".to_string(),
            current_iter_index: 1,
            iters_completed: vec![CompletedIter {
                name: "discuss".to_string(),
                session_id: "a1b2c3d4".to_string(),
                completed_at: "2026-03-17T14:05:00Z".to_string(),
                outcome: "complete".to_string(),
            }],
            spec: Some("auth".to_string()),
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:10:00Z".to_string(),
        };

        create_run_dir(root, &meta.run_id).unwrap();
        write_metadata(root, &meta).unwrap();

        let read_back = read_metadata(root, &meta.run_id).unwrap().unwrap();
        assert_eq!(read_back.run_id, meta.run_id);
        assert_eq!(read_back.cursus, meta.cursus);
        assert_eq!(read_back.status, RunStatus::Running);
        assert_eq!(read_back.current_iter, "draft");
        assert_eq!(read_back.current_iter_index, 1);
        assert_eq!(read_back.iters_completed.len(), 1);
        assert_eq!(read_back.iters_completed[0].name, "discuss");
        assert_eq!(read_back.iters_completed[0].session_id, "a1b2c3d4");
        assert_eq!(read_back.iters_completed[0].outcome, "complete");
        assert_eq!(read_back.spec.as_deref(), Some("auth"));
        assert!(read_back.mode_override.is_none());
    }

    #[test]
    fn status_transitions() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        let mut meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };

        create_run_dir(root, run_id).unwrap();

        // running -> completed
        meta.status = RunStatus::Completed;
        meta.touch();
        write_metadata(root, &meta).unwrap();
        let read_back = read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(read_back.status, RunStatus::Completed);

        // running -> stalled
        meta.status = RunStatus::Stalled;
        meta.touch();
        write_metadata(root, &meta).unwrap();
        let read_back = read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(read_back.status, RunStatus::Stalled);

        // running -> interrupted
        meta.status = RunStatus::Interrupted;
        meta.touch();
        write_metadata(root, &meta).unwrap();
        let read_back = read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(read_back.status, RunStatus::Interrupted);
    }

    #[test]
    fn create_run_dir_creates_context_subdir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "spec-20260317T140000";

        let dir = create_run_dir(root, run_id).unwrap();
        assert!(dir.exists());
        assert!(context_dir(root, run_id).exists());
    }

    #[test]
    fn pid_file_write_read_remove() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let path = write_pid_file(root, run_id).unwrap();
        assert!(path.exists());

        let pid = read_pid(root, run_id).unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_file(root, run_id);
        assert!(!path.exists());
        assert!(read_pid(root, run_id).is_none());
    }

    #[test]
    fn stale_run_detection_alive_process() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();
        write_pid_file(root, run_id).unwrap();

        assert!(!is_stale_run(root, run_id).unwrap());
    }

    #[test]
    fn stale_run_detection_dead_process() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();
        // Write a PID that is extremely unlikely to be alive
        fs::write(pid_path(root, run_id), "4000000").unwrap();

        assert!(is_stale_run(root, run_id).unwrap());
    }

    #[test]
    fn stale_run_detection_no_pid_file() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();

        assert!(is_stale_run(root, run_id).unwrap());
    }

    #[test]
    fn completed_run_not_stale() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Completed,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();

        assert!(!is_stale_run(root, run_id).unwrap());
    }

    #[test]
    fn nonexistent_run_not_stale() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_stale_run(tmp.path(), "nonexistent").unwrap());
    }

    #[test]
    fn mark_stale_runs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create a stale run (dead PID)
        let stale_id = "stale-20260317T140000";
        create_run_dir(root, stale_id).unwrap();
        let meta = RunMetadata {
            run_id: stale_id.to_string(),
            cursus: "stale".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();
        fs::write(pid_path(root, stale_id), "4000000").unwrap();

        // Create an alive run (our PID)
        let alive_id = "alive-20260317T150000";
        create_run_dir(root, alive_id).unwrap();
        let alive_meta = RunMetadata {
            run_id: alive_id.to_string(),
            cursus: "alive".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T15:00:00Z".to_string(),
            updated_at: "2026-03-17T15:00:00Z".to_string(),
        };
        write_metadata(root, &alive_meta).unwrap();
        write_pid_file(root, alive_id).unwrap();

        let marked = mark_stale_runs_interrupted(root).unwrap();
        assert_eq!(marked, vec![stale_id.to_string()]);

        let stale_read = read_metadata(root, stale_id).unwrap().unwrap();
        assert_eq!(stale_read.status, RunStatus::Interrupted);

        let alive_read = read_metadata(root, alive_id).unwrap().unwrap();
        assert_eq!(alive_read.status, RunStatus::Running);
    }

    #[test]
    fn mark_stale_runs_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let marked = mark_stale_runs_interrupted(tmp.path()).unwrap();
        assert!(marked.is_empty());
    }

    #[test]
    fn read_metadata_nonexistent() {
        let tmp = TempDir::new().unwrap();
        assert!(read_metadata(tmp.path(), "nonexistent").unwrap().is_none());
    }

    #[test]
    fn metadata_atomic_write_no_tmp_left() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "test-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "test".to_string(),
            status: RunStatus::Running,
            current_iter: "build".to_string(),
            current_iter_index: 0,
            iters_completed: Vec::new(),
            spec: None,
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:00:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();

        let dir = run_dir(root, run_id);
        assert!(!dir.join("meta.json.tmp").exists());
        assert!(dir.join("meta.json").exists());
    }

    #[test]
    fn status_display() {
        assert_eq!(RunStatus::Running.to_string(), "running");
        assert_eq!(RunStatus::Completed.to_string(), "completed");
        assert_eq!(RunStatus::Stalled.to_string(), "stalled");
        assert_eq!(RunStatus::Interrupted.to_string(), "interrupted");
    }

    #[test]
    fn status_serde_roundtrip() {
        let json = serde_json::to_string(&RunStatus::Stalled).unwrap();
        assert_eq!(json, "\"stalled\"");
        let back: RunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RunStatus::Stalled);
    }

    #[test]
    fn metadata_with_mode_override() {
        let meta = RunMetadata::new("build", "compile", None, Some("afk"));
        assert_eq!(meta.mode_override.as_deref(), Some("afk"));
    }

    #[test]
    fn resume_from_stalled_preserves_iter_position() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let run_id = "spec-20260317T140000";

        create_run_dir(root, run_id).unwrap();
        let meta = RunMetadata {
            run_id: run_id.to_string(),
            cursus: "spec".to_string(),
            status: RunStatus::Stalled,
            current_iter: "draft".to_string(),
            current_iter_index: 1,
            iters_completed: vec![CompletedIter {
                name: "discuss".to_string(),
                session_id: "sess-1".to_string(),
                completed_at: "2026-03-17T14:05:00Z".to_string(),
                outcome: "complete".to_string(),
            }],
            spec: Some("auth".to_string()),
            mode_override: None,
            created_at: "2026-03-17T14:00:00Z".to_string(),
            updated_at: "2026-03-17T14:10:00Z".to_string(),
        };
        write_metadata(root, &meta).unwrap();

        let read_back = read_metadata(root, run_id).unwrap().unwrap();
        assert_eq!(read_back.status, RunStatus::Stalled);
        assert_eq!(read_back.current_iter, "draft");
        assert_eq!(read_back.current_iter_index, 1);
        assert_eq!(read_back.iters_completed.len(), 1);
    }
}
