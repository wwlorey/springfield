use std::path::Path;
use std::process::Command;

pub struct ResolvedContext {
    pub files: Vec<String>,
    pub spec_index: Option<String>,
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

    let spec_index = resolve_spec_index();

    ResolvedContext { files, spec_index }
}

fn resolve_spec_index() -> Option<String> {
    if let Some(index) = resolve_spec_index_from_fm() {
        return Some(index);
    }

    eprintln!("warning: spec index not available (fm unreachable), skipping");
    None
}

fn resolve_spec_index_from_fm() -> Option<String> {
    let output = Command::new("fm").args(["list", "--json"]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8(output.stdout).ok()?;
    format_spec_index_as_markdown(&json_str)
}

fn format_spec_index_as_markdown(json_str: &str) -> Option<String> {
    let specs: Vec<serde_json::Value> = serde_json::from_str(json_str).ok()?;

    let mut table =
        String::from("# Specifications\n\n| Spec | Src | Purpose |\n|------|-----|---------|");

    for spec in &specs {
        let stem = spec.get("stem")?.as_str()?;
        let src_col = spec
            .get("src")
            .and_then(|v| v.as_str())
            .map(|s| format!("`{s}`"))
            .unwrap_or_default();
        let purpose = spec.get("purpose")?.as_str()?;
        table.push_str(&format!("\n| {stem} | {src_col} | {purpose} |"));
    }

    Some(table)
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
        assert!(result.spec_index.is_none());
    }

    #[test]
    fn all_files_missing_returns_empty() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), home_dir.path());
        assert!(result.files.is_empty());
        assert!(result.spec_index.is_none());
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

        let result = resolve_context_files(cwd, home);
        assert_eq!(result.files.len(), 2);

        let memento = &result.files[0];
        assert!(memento.starts_with(cwd.to_str().unwrap()));

        let bp = &result.files[1];
        assert!(bp.starts_with(home.to_str().unwrap()));
    }

    #[test]
    fn spec_index_none_when_no_source_available() {
        let (cwd_dir, home_dir) = setup();
        let result = resolve_context_files(cwd_dir.path(), home_dir.path());
        assert!(result.spec_index.is_none());
    }

    #[test]
    fn format_spec_index_as_markdown_produces_table() {
        let json = r#"[
            {"stem":"auth","src":"crates/auth/","purpose":"Authentication","status":"stable","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"},
            {"stem":"ralph","src":"crates/ralph/","purpose":"Runner","status":"draft","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}
        ]"#;
        let result = format_spec_index_as_markdown(json).unwrap();
        assert!(result.contains("# Specifications"));
        assert!(result.contains("| auth | `crates/auth/` | Authentication |"));
        assert!(result.contains("| ralph | `crates/ralph/` | Runner |"));
    }

    #[test]
    fn format_spec_index_as_markdown_empty_array() {
        let result = format_spec_index_as_markdown("[]").unwrap();
        assert!(result.contains("# Specifications"));
        assert!(result.contains("| Spec | Src | Purpose |"));
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn format_spec_index_as_markdown_invalid_json() {
        assert!(format_spec_index_as_markdown("not json").is_none());
    }
}
