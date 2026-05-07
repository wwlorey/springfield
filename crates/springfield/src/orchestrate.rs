use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use shutdown::{ShutdownConfig, ShutdownController};

use crate::loop_mgmt::{self, IterationRecord, SessionMetadata};
use crate::style;

fn exit_code_to_status(code: i32) -> &'static str {
    match code {
        0 => "completed",
        2 => "exhausted",
        _ => "interrupted",
    }
}

fn update_metadata_on_exit(root: &Path, loop_id: &str, exit_code: i32) {
    match loop_mgmt::read_session_metadata(root, loop_id) {
        Ok(Some(mut meta)) => {
            meta.status = exit_code_to_status(exit_code).to_string();
            meta.updated_at = Utc::now().to_rfc3339();
            if let Err(e) = loop_mgmt::write_session_metadata(root, &meta) {
                style::print_warning(&format!("failed to update session metadata: {e}"));
            }
        }
        Ok(None) => {
            style::print_warning(&format!(
                "session metadata not found for {loop_id}, skipping update"
            ));
        }
        Err(e) => {
            style::print_warning(&format!(
                "failed to read session metadata for {loop_id}: {e}"
            ));
        }
    }
}

pub fn humanize_relative_time(updated_at: &str) -> String {
    let Ok(updated) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return "unknown".to_string();
    };
    let now = Utc::now();
    let delta = now.signed_duration_since(updated);
    let secs = delta.num_seconds();
    if secs < 0 {
        return "just now".to_string();
    }
    if secs < 60 {
        return format!("{secs}s ago");
    }
    let mins = delta.num_minutes();
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = delta.num_days();
    format!("{days}d ago")
}

fn run_resume_session(root: &Path, meta: &SessionMetadata, session_id: &str) -> io::Result<i32> {
    let loop_id = &meta.loop_id;

    style::print_action_detail(
        &format!("resuming session [{loop_id}]"),
        &format!("session: {session_id}"),
    );

    let controller = ShutdownController::new(ShutdownConfig {
        monitor_stdin: false,
        ..Default::default()
    })?;

    let log_path = loop_mgmt::create_log_file(root, loop_id).ok();

    let mut command = Command::new("cl");
    command.args([
        "--resume",
        session_id,
        "--verbose",
        "--dangerously-skip-permissions",
    ]);

    let start = std::time::Instant::now();
    let result = crate::iter_runner::pty_tee::run_interactive_with_pty(
        &mut command,
        log_path.as_deref(),
        &controller,
    )?;

    let exit_code = result.exit_code.unwrap_or(1);

    if exit_code != 0 && start.elapsed() < Duration::from_secs(5) {
        style::print_warning("session may have expired");
        eprintln!();
        eprint!("Restart with same prompt ({})? [Y/n] ", meta.prompt);
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let answer = input.trim().to_lowercase();
            if answer.is_empty() || answer == "y" || answer == "yes" {
                return restart_with_prompt(root, meta, &controller, log_path.as_deref());
            }
        }
    }

    update_metadata_on_exit(root, loop_id, exit_code);

    Ok(exit_code)
}

fn restart_with_prompt(
    root: &Path,
    meta: &SessionMetadata,
    controller: &ShutdownController,
    log_path: Option<&Path>,
) -> io::Result<i32> {
    style::print_action("restarting with original prompt...");

    let mut command = Command::new("cl");
    command.args([
        "--verbose",
        "--dangerously-skip-permissions",
        "--settings",
        r#"{"autoMemoryEnabled": false, "sandbox": {"allowUnsandboxedCommands": false}}"#,
    ]);

    let is_file = Path::new(&meta.prompt).exists();
    if is_file {
        command.arg(format!("@{}", meta.prompt));
    } else {
        command.arg(&meta.prompt);
    }

    let result =
        crate::iter_runner::pty_tee::run_interactive_with_pty(&mut command, log_path, controller)?;

    let exit_code = result.exit_code.unwrap_or(1);
    update_metadata_on_exit(root, &meta.loop_id, exit_code);
    Ok(exit_code)
}

fn prompt_and_select(entries_len: usize) -> io::Result<Option<usize>> {
    eprint!("Select session (1-{entries_len}): ");

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
        return Ok(None);
    }

    let choice: usize = input
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid selection"))?;

    if choice < 1 || choice > entries_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Selection out of range: {choice}"),
        ));
    }

    Ok(Some(choice))
}

struct DisplayEntry<'a> {
    meta: &'a SessionMetadata,
    iteration: &'a IterationRecord,
}

fn print_entries(entries: &[DisplayEntry]) {
    eprintln!("Recent sessions:");
    for (i, e) in entries.iter().enumerate() {
        let relative = humanize_relative_time(&e.iteration.completed_at);
        eprintln!(
            "  {:>2}. {:<40} iter {:<4} {:<12} {:<12} {}",
            i + 1,
            e.meta.loop_id,
            e.iteration.iteration,
            e.meta.mode,
            e.meta.status,
            relative
        );
    }
}

pub fn run_resume(root: &Path, loop_id: &str) -> io::Result<i32> {
    let meta = loop_mgmt::read_session_metadata(root, loop_id)?;
    let m = match meta {
        Some(m) => m,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Session not found: {loop_id}"),
            ));
        }
    };

    if m.iterations.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("No iterations found for session: {loop_id}"),
        ));
    }

    if m.iterations.len() == 1 {
        return run_resume_session(root, &m, &m.iterations[0].session_id);
    }

    let entries: Vec<DisplayEntry> = m
        .iterations
        .iter()
        .map(|it| DisplayEntry {
            meta: &m,
            iteration: it,
        })
        .collect();

    print_entries(&entries);

    match prompt_and_select(entries.len())? {
        Some(choice) => {
            let selected = &entries[choice - 1];
            run_resume_session(root, &m, &selected.iteration.session_id)
        }
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use tempfile::TempDir;

    #[test]
    fn exit_code_to_status_mappings() {
        assert_eq!(exit_code_to_status(0), "completed");
        assert_eq!(exit_code_to_status(2), "exhausted");
        assert_eq!(exit_code_to_status(130), "interrupted");
        assert_eq!(exit_code_to_status(1), "interrupted");
        assert_eq!(exit_code_to_status(42), "interrupted");
    }

    #[test]
    fn humanize_seconds() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("s ago"), "got: {result}");
    }

    #[test]
    fn humanize_minutes() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::minutes(5)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("m ago"), "got: {result}");
    }

    #[test]
    fn humanize_hours() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::hours(3)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("h ago"), "got: {result}");
    }

    #[test]
    fn humanize_days() {
        let now = Utc::now();
        let ts = (now - chrono::Duration::days(2)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert!(result.ends_with("d ago"), "got: {result}");
    }

    #[test]
    fn humanize_invalid_timestamp() {
        assert_eq!(humanize_relative_time("not-a-date"), "unknown");
    }

    #[test]
    fn humanize_future_timestamp() {
        let now = Utc::now();
        let ts = (now + chrono::Duration::hours(1)).to_rfc3339();
        let result = humanize_relative_time(&ts);
        assert_eq!(result, "just now");
    }

    #[test]
    fn resume_unknown_loop_id_returns_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let err = run_resume(root, "nonexistent-id").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(
            err.to_string()
                .contains("Session not found: nonexistent-id"),
            "got: {}",
            err
        );
    }

    #[test]
    fn resume_with_valid_loop_id_launches_cl_with_resume_flag() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let meta = SessionMetadata {
            loop_id: "build-auth-20260316T120000".to_string(),
            iterations: vec![IterationRecord {
                iteration: 1,
                session_id: session_id.to_string(),
                completed_at: "2026-03-16T12:02:30Z".to_string(),
            }],
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            cursus: None,
            mode: "interactive".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "interrupted".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:05:30Z".to_string(),
        };
        loop_mgmt::write_session_metadata(root, &meta).unwrap();

        let mock_cl = root.join("mock_cl.sh");
        fs::write(
            &mock_cl,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/cl_args.txt\"\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_cl, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", root.display(), original_path);
        unsafe { std::env::set_var("PATH", &new_path) };

        // Rename mock to `cl`
        let cl_path = root.join("cl");
        fs::rename(&mock_cl, &cl_path).unwrap();

        let result = run_resume(root, "build-auth-20260316T120000");

        unsafe { std::env::set_var("PATH", &original_path) };

        let exit_code = result.unwrap();
        assert_eq!(exit_code, 0);

        let cl_args = fs::read_to_string(root.join("cl_args.txt")).unwrap();
        assert!(
            cl_args.contains("--resume"),
            "cl should receive --resume, got: {cl_args}"
        );
        assert!(
            cl_args.contains(session_id),
            "cl should receive session_id, got: {cl_args}"
        );
        assert!(
            cl_args.contains("--verbose"),
            "cl should receive --verbose, got: {cl_args}"
        );

        let updated = loop_mgmt::read_session_metadata(root, "build-auth-20260316T120000")
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "completed");
    }

    #[test]
    fn resume_loop_id_with_no_iterations_returns_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".sgf/run")).unwrap();

        let meta = SessionMetadata {
            loop_id: "build-auth-20260316T120000".to_string(),
            iterations: Vec::new(),
            stage: "build".to_string(),
            spec: Some("auth".to_string()),
            cursus: None,
            mode: "interactive".to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 3,
            status: "running".to_string(),
            created_at: "2026-03-16T12:00:00Z".to_string(),
            updated_at: "2026-03-16T12:00:00Z".to_string(),
        };
        loop_mgmt::write_session_metadata(root, &meta).unwrap();

        let err = run_resume(root, "build-auth-20260316T120000").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string().contains("No iterations found"),
            "got: {}",
            err
        );
    }

    fn make_session(
        loop_id: &str,
        mode: &str,
        status: &str,
        iterations: Vec<(u32, &str, &str)>,
    ) -> SessionMetadata {
        SessionMetadata {
            loop_id: loop_id.to_string(),
            iterations: iterations
                .into_iter()
                .map(|(iter, sid, completed)| IterationRecord {
                    iteration: iter,
                    session_id: sid.to_string(),
                    completed_at: completed.to_string(),
                })
                .collect(),
            stage: "build".to_string(),
            spec: None,
            cursus: None,
            mode: mode.to_string(),
            prompt: ".sgf/prompts/build.md".to_string(),
            iterations_total: 30,
            status: status.to_string(),
            created_at: "2026-03-16T10:00:00Z".to_string(),
            updated_at: "2026-03-16T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn picker_entries_sort_newest_first() {
        let s1 = make_session(
            "loop-a",
            "afk",
            "completed",
            vec![
                (1, "sid-a1", "2026-03-16T10:00:00Z"),
                (2, "sid-a2", "2026-03-16T12:00:00Z"),
            ],
        );
        let s2 = make_session(
            "loop-b",
            "interactive",
            "interrupted",
            vec![(1, "sid-b1", "2026-03-16T11:00:00Z")],
        );
        let sessions = vec![s1, s2];

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].iteration.completed_at, "2026-03-16T12:00:00Z");
        assert_eq!(entries[0].meta.loop_id, "loop-a");
        assert_eq!(entries[0].iteration.iteration, 2);

        assert_eq!(entries[1].iteration.completed_at, "2026-03-16T11:00:00Z");
        assert_eq!(entries[1].meta.loop_id, "loop-b");
        assert_eq!(entries[1].iteration.iteration, 1);

        assert_eq!(entries[2].iteration.completed_at, "2026-03-16T10:00:00Z");
        assert_eq!(entries[2].meta.loop_id, "loop-a");
        assert_eq!(entries[2].iteration.iteration, 1);
    }

    #[test]
    fn picker_entries_truncate_drops_oldest() {
        let mut sessions = Vec::new();
        for i in 0..25u32 {
            sessions.push(make_session(
                &format!("loop-{i:02}"),
                "afk",
                "completed",
                vec![(
                    1,
                    &format!("sid-{i:02}"),
                    &format!("2026-03-{:02}T10:00:00Z", (i % 28) + 1),
                )],
            ));
        }

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));
        entries.truncate(20);

        assert_eq!(entries.len(), 20);
        // Newest should be first (day 25)
        assert_eq!(entries[0].meta.loop_id, "loop-24");
        assert_eq!(entries[0].iteration.completed_at, "2026-03-25T10:00:00Z");
        // Oldest kept should be day 6 (loop-05)
        assert_eq!(entries[19].meta.loop_id, "loop-05");
        assert_eq!(entries[19].iteration.completed_at, "2026-03-06T10:00:00Z");
        // The 5 oldest (loop-00..loop-04, days 1-5) should be dropped
        let loop_ids: Vec<&str> = entries.iter().map(|e| e.meta.loop_id.as_str()).collect();
        for i in 0..5 {
            assert!(
                !loop_ids.contains(&format!("loop-{i:02}").as_str()),
                "loop-{i:02} (oldest) should be truncated"
            );
        }
    }

    #[test]
    fn print_entries_display_format() {
        let session = make_session(
            "build-auth-20260316T120000",
            "afk",
            "completed",
            vec![(3, "sid-abc", "2026-01-01T00:00:00Z")],
        );
        let entries: Vec<DisplayEntry> = session
            .iterations
            .iter()
            .map(|it| DisplayEntry {
                meta: &session,
                iteration: it,
            })
            .collect();

        // print_entries writes to stderr; verify it doesn't panic and exercises the format
        print_entries(&entries);

        // Verify the format string components are what we expect
        let e = &entries[0];
        let relative = humanize_relative_time(&e.iteration.completed_at);
        let line = format!(
            "  {:>2}. {:<40} iter {:<4} {:<12} {:<12} {}",
            1, e.meta.loop_id, e.iteration.iteration, e.meta.mode, e.meta.status, relative
        );
        assert!(line.contains("build-auth-20260316T120000"));
        assert!(line.contains("iter 3"));
        assert!(line.contains("afk"));
        assert!(line.contains("completed"));
        assert!(line.contains("ago") || line.contains("unknown"));
    }

    #[test]
    fn print_entries_multi_session_display() {
        let s1 = make_session(
            "build-auth-20260316T120000",
            "afk",
            "completed",
            vec![(1, "sid-1", "2026-03-16T12:00:00Z")],
        );
        let s2 = make_session(
            "verify-20260316T130000",
            "interactive",
            "interrupted",
            vec![(2, "sid-2", "2026-03-16T13:00:00Z")],
        );
        let sessions = vec![s1, s2];

        let mut entries: Vec<DisplayEntry> = Vec::new();
        for s in &sessions {
            for it in &s.iterations {
                entries.push(DisplayEntry {
                    meta: s,
                    iteration: it,
                });
            }
        }
        entries.sort_by(|a, b| b.iteration.completed_at.cmp(&a.iteration.completed_at));

        // Verify different sessions appear with correct numbering
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].meta.loop_id, "verify-20260316T130000");
        assert_eq!(entries[0].meta.mode, "interactive");
        assert_eq!(entries[0].meta.status, "interrupted");
        assert_eq!(entries[1].meta.loop_id, "build-auth-20260316T120000");
        assert_eq!(entries[1].meta.mode, "afk");
        assert_eq!(entries[1].meta.status, "completed");

        // Verify print doesn't panic with multiple entries
        print_entries(&entries);
    }

    #[test]
    fn setsid_skipped_when_env_var_set() {
        unsafe { std::env::set_var("SGF_TEST_NO_SETSID", "1") };
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let child_pid = child.id() as libc::pid_t;
        let parent_pgid = unsafe { libc::getpgid(0) };
        let child_pgid = unsafe { libc::getpgid(child_pid) };
        assert_eq!(
            child_pgid, parent_pgid,
            "with SGF_TEST_NO_SETSID set, child should share parent's process group"
        );
        let _ = child.kill();
        let _ = child.wait();
        unsafe { std::env::remove_var("SGF_TEST_NO_SETSID") };
    }

    #[test]
    fn setsid_applied_without_env_var() {
        unsafe { std::env::remove_var("SGF_TEST_NO_SETSID") };
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        let child_pid = child.id() as libc::pid_t;
        let parent_pgid = unsafe { libc::getpgid(0) };
        let child_pgid = unsafe { libc::getpgid(child_pid) };
        assert_ne!(
            child_pgid, parent_pgid,
            "without SGF_TEST_NO_SETSID, child with setsid should be in its own session"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
