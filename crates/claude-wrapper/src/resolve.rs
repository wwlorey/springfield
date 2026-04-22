use std::path::Path;

pub fn resolve_context_files(cwd: &Path, home: Option<&Path>) -> Vec<String> {
    let mut files = Vec::new();

    let layered = ["MEMENTO.md", "BACKPRESSURE.md"];

    for filename in layered {
        let local = cwd.join(".sgf").join(filename);
        if local.exists() {
            files.push(local.to_string_lossy().into_owned());
            continue;
        }
        if let Some(home) = home {
            let global = home.join(".sgf").join(filename);
            if global.exists() {
                files.push(global.to_string_lossy().into_owned());
                continue;
            }
        }
        eprintln!("warning: {filename} not found in .sgf/ (local or global), skipping");
    }

    let lookbook = cwd.join("LOOKBOOK.html");
    if lookbook.exists() {
        files.push(lookbook.to_string_lossy().into_owned());
    } else {
        eprintln!("note: LOOKBOOK.html not found at repo root, skipping");
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, TempDir) {
        (TempDir::new().unwrap(), TempDir::new().unwrap())
    }

    #[test]
    fn local_file_takes_precedence_over_global() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "local").unwrap();

        fs::create_dir_all(home.join(".sgf")).unwrap();
        fs::write(home.join(".sgf/MEMENTO.md"), "global").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        let memento = result.iter().find(|f| f.contains("MEMENTO.md")).unwrap();
        assert!(memento.starts_with(cwd.to_str().unwrap()));
    }

    #[test]
    fn falls_back_to_global_when_local_missing() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(home.join(".sgf")).unwrap();
        fs::write(home.join(".sgf/MEMENTO.md"), "global").unwrap();
        fs::write(home.join(".sgf/BACKPRESSURE.md"), "global").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        let memento = result.iter().find(|f| f.contains("MEMENTO.md")).unwrap();
        assert!(memento.starts_with(home.to_str().unwrap()));
    }

    #[test]
    fn both_missing_skips_file() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), Some(home_dir.path()));
        assert!(result.is_empty());
    }

    #[test]
    fn all_files_missing_returns_empty() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), Some(home_dir.path()));
        assert!(result.is_empty());
    }

    #[test]
    fn mixed_local_and_global_resolution() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "local memento").unwrap();

        fs::create_dir_all(home.join(".sgf")).unwrap();
        fs::write(home.join(".sgf/BACKPRESSURE.md"), "global bp").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        assert_eq!(result.len(), 2);

        let memento = &result[0];
        assert!(memento.starts_with(cwd.to_str().unwrap()));

        let bp = &result[1];
        assert!(bp.starts_with(home.to_str().unwrap()));
    }

    #[test]
    fn none_home_returns_only_local_files() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "local").unwrap();

        fs::create_dir_all(home.join(".sgf")).unwrap();
        fs::write(home.join(".sgf/MEMENTO.md"), "global memento").unwrap();
        fs::write(home.join(".sgf/BACKPRESSURE.md"), "global bp").unwrap();

        let result = resolve_context_files(cwd, None);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("MEMENTO.md"));
        assert!(
            result[0].starts_with(cwd.to_str().unwrap()),
            "should resolve from local, not global"
        );
        assert!(
            !result.iter().any(|f| f.contains("BACKPRESSURE.md")),
            "global BACKPRESSURE.md should not be resolved when home is None"
        );
    }

    #[test]
    fn lookbook_present_at_repo_root_included_last() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();
        fs::write(cwd.join(".sgf/BACKPRESSURE.md"), "b").unwrap();
        fs::write(cwd.join("LOOKBOOK.html"), "<html>lookbook</html>").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        assert_eq!(result.len(), 3);
        assert!(result[2].contains("LOOKBOOK.html"));
    }

    #[test]
    fn lookbook_absent_skipped() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        assert!(!result.iter().any(|f| f.contains("LOOKBOOK.html")));
    }

    #[test]
    fn lookbook_ordering_always_after_layered_files() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();
        let home = home_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "m").unwrap();
        fs::write(cwd.join(".sgf/BACKPRESSURE.md"), "b").unwrap();
        fs::write(cwd.join("LOOKBOOK.html"), "lb").unwrap();

        let result = resolve_context_files(cwd, Some(home));
        let memento_pos = result
            .iter()
            .position(|f| f.contains("MEMENTO.md"))
            .unwrap();
        let bp_pos = result
            .iter()
            .position(|f| f.contains("BACKPRESSURE.md"))
            .unwrap();
        let lb_pos = result
            .iter()
            .position(|f| f.contains("LOOKBOOK.html"))
            .unwrap();
        assert!(lb_pos > memento_pos);
        assert!(lb_pos > bp_pos);
    }

    #[test]
    fn none_home_with_no_local_files_returns_empty() {
        let (_, home_dir) = setup();
        let home = home_dir.path();
        let cwd_dir = TempDir::new().unwrap();

        fs::create_dir_all(home.join(".sgf")).unwrap();
        fs::write(home.join(".sgf/MEMENTO.md"), "global").unwrap();
        fs::write(home.join(".sgf/BACKPRESSURE.md"), "global").unwrap();

        let result = resolve_context_files(cwd_dir.path(), None);
        assert!(
            result.is_empty(),
            "should return empty when home is None and no local files exist, even if global files do"
        );
    }
}
