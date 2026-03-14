use std::path::Path;

pub struct ResolvedContext {
    pub files: Vec<String>,
}

pub fn resolve_context_files(cwd: &Path, home: &Path) -> ResolvedContext {
    let mut files = Vec::new();

    let layered = [("MEMENTO.md", true), ("BACKPRESSURE.md", true)];

    for (filename, has_global) in layered {
        let local = cwd.join(".sgf").join(filename);
        if local.exists() {
            files.push(local.to_string_lossy().into_owned());
            continue;
        }
        if has_global {
            let global = home.join(".sgf").join(filename);
            if global.exists() {
                files.push(global.to_string_lossy().into_owned());
                continue;
            }
        }
        eprintln!("warning: {filename} not found in .sgf/ (local or global), skipping");
    }

    let specs_readme = cwd.join("specs").join("README.md");
    if specs_readme.exists() {
        files.push(specs_readme.to_string_lossy().into_owned());
    } else {
        eprintln!("warning: specs/README.md not found, skipping");
    }

    ResolvedContext { files }
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

        let result = resolve_context_files(cwd, home);
        assert!(
            result
                .files
                .iter()
                .any(|f| f.contains("local") || f.starts_with(cwd.to_str().unwrap()))
        );
        let memento = result
            .files
            .iter()
            .find(|f| f.contains("MEMENTO.md"))
            .unwrap();
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

        let result = resolve_context_files(cwd, home);
        let memento = result
            .files
            .iter()
            .find(|f| f.contains("MEMENTO.md"))
            .unwrap();
        assert!(memento.starts_with(home.to_str().unwrap()));
    }

    #[test]
    fn both_missing_skips_file() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), home_dir.path());
        assert!(result.files.is_empty());
    }

    #[test]
    fn specs_readme_only_checks_local() {
        let (cwd_dir, home_dir) = setup();
        let cwd = cwd_dir.path();

        fs::create_dir_all(cwd.join("specs")).unwrap();
        fs::write(cwd.join("specs/README.md"), "specs").unwrap();

        let result = resolve_context_files(cwd, home_dir.path());
        assert!(result.files.iter().any(|f| f.contains("specs/README.md")));
    }

    #[test]
    fn all_files_missing_returns_empty() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), home_dir.path());
        assert!(result.files.is_empty());
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

        fs::create_dir_all(cwd.join("specs")).unwrap();
        fs::write(cwd.join("specs/README.md"), "specs").unwrap();

        let result = resolve_context_files(cwd, home);
        assert_eq!(result.files.len(), 3);

        let memento = &result.files[0];
        assert!(memento.starts_with(cwd.to_str().unwrap()));

        let bp = &result.files[1];
        assert!(bp.starts_with(home.to_str().unwrap()));

        let specs = &result.files[2];
        assert!(specs.contains("specs/README.md"));
    }
}
