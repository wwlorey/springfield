use std::path::Path;

pub fn resolve_context_files(cwd: &Path, home: Option<&Path>) -> Vec<String> {
    let mut files = Vec::new();

    let layered = [("MEMENTO.md", true), ("BACKPRESSURE.md", true)];

    for (filename, has_global) in layered {
        let local = cwd.join(".sgf").join(filename);
        if local.exists() {
            files.push(local.to_string_lossy().into_owned());
            continue;
        }
        if has_global && let Some(home) = home {
            let global = home.join(".sgf").join(filename);
            if global.exists() {
                files.push(global.to_string_lossy().into_owned());
                continue;
            }
        }
        eprintln!("warning: {filename} not found in .sgf/ (local or global), skipping");
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
    fn none_home_skips_global_lookups() {
        let cwd_dir = TempDir::new().unwrap();
        let cwd = cwd_dir.path();

        fs::create_dir_all(cwd.join(".sgf")).unwrap();
        fs::write(cwd.join(".sgf/MEMENTO.md"), "local").unwrap();

        let result = resolve_context_files(cwd, None);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("MEMENTO.md"));
    }

    #[test]
    fn none_home_with_no_local_files_returns_empty() {
        let cwd_dir = TempDir::new().unwrap();
        let result = resolve_context_files(cwd_dir.path(), None);
        assert!(result.is_empty());
    }
}
