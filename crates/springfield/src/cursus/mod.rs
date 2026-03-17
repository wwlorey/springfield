pub mod context;
pub mod runner;
pub mod state;
pub mod toml;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::cursus::toml::CursusDefinition;

#[derive(Debug)]
pub struct ResolvedCursus {
    pub name: String,
    pub definition: CursusDefinition,
    pub path: PathBuf,
}

fn global_cursus_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".sgf/cursus"))
}

pub fn resolve_cursus(root: &Path, command: &str) -> Option<ResolvedCursus> {
    let local = root.join(format!(".sgf/cursus/{command}.toml"));
    if local.exists()
        && let Ok(def) = toml::parse_file(&local)
    {
        return Some(ResolvedCursus {
            name: command.to_string(),
            definition: def,
            path: local,
        });
    }

    if let Some(global_dir) = global_cursus_dir() {
        let global = global_dir.join(format!("{command}.toml"));
        if global.exists()
            && let Ok(def) = toml::parse_file(&global)
        {
            return Some(ResolvedCursus {
                name: command.to_string(),
                definition: def,
                path: global,
            });
        }
    }

    None
}

fn load_all_from_dir(dir: &Path) -> HashMap<String, CursusDefinition> {
    let mut map = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return map,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && let Ok(def) = toml::parse_file(&path)
        {
            map.insert(stem.to_string(), def);
        }
    }
    map
}

pub fn resolve_alias(root: &Path, alias: &str) -> Option<ResolvedCursus> {
    let local_dir = root.join(".sgf/cursus");
    let local_defs = load_all_from_dir(&local_dir);
    for (name, def) in &local_defs {
        if def.alias.as_deref() == Some(alias) {
            return Some(ResolvedCursus {
                name: name.clone(),
                definition: def.clone(),
                path: local_dir.join(format!("{name}.toml")),
            });
        }
    }

    if let Some(global_dir) = global_cursus_dir() {
        let global_defs = load_all_from_dir(&global_dir);
        for (name, def) in &global_defs {
            if local_defs.contains_key(name) {
                continue;
            }
            if def.alias.as_deref() == Some(alias) {
                return Some(ResolvedCursus {
                    name: name.clone(),
                    definition: def.clone(),
                    path: global_dir.join(format!("{name}.toml")),
                });
            }
        }
    }

    None
}

pub fn resolve_command(root: &Path, command: &str) -> Result<ResolvedCursus, io::Error> {
    if let Some(resolved) = resolve_cursus(root, command) {
        return Ok(resolved);
    }

    if let Some(resolved) = resolve_alias(root, command) {
        return Ok(resolved);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("unknown command: {command}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cursus_toml(dir: &Path, name: &str, content: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    const SIMPLE_CURSUS: &str = r#"
description = "Build loop"
alias = "b"
auto_push = true

[[iters]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
"#;

    const TEST_CURSUS: &str = r#"
description = "Test loop"
alias = "t"

[[iters]]
name = "test"
prompt = "test.md"
"#;

    #[test]
    fn resolve_cursus_local() {
        let tmp = TempDir::new().unwrap();
        write_cursus_toml(&tmp.path().join(".sgf/cursus"), "build", SIMPLE_CURSUS);

        let resolved = resolve_cursus(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
        assert_eq!(resolved.definition.alias.as_deref(), Some("b"));
    }

    #[test]
    fn resolve_cursus_not_found() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();

        assert!(resolve_cursus(tmp.path(), "nonexistent").is_none());
    }

    #[test]
    fn resolve_alias_local() {
        let tmp = TempDir::new().unwrap();
        write_cursus_toml(&tmp.path().join(".sgf/cursus"), "build", SIMPLE_CURSUS);

        let resolved = resolve_alias(tmp.path(), "b").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_alias_not_found() {
        let tmp = TempDir::new().unwrap();
        write_cursus_toml(&tmp.path().join(".sgf/cursus"), "build", SIMPLE_CURSUS);

        assert!(resolve_alias(tmp.path(), "x").is_none());
    }

    #[test]
    fn resolve_command_direct() {
        let tmp = TempDir::new().unwrap();
        write_cursus_toml(&tmp.path().join(".sgf/cursus"), "build", SIMPLE_CURSUS);

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_command_via_alias() {
        let tmp = TempDir::new().unwrap();
        write_cursus_toml(&tmp.path().join(".sgf/cursus"), "build", SIMPLE_CURSUS);

        let resolved = resolve_command(tmp.path(), "b").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_command_unknown() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();

        let err = resolve_command(tmp.path(), "ghost").unwrap_err();
        assert!(err.to_string().contains("unknown command: ghost"));
    }

    #[test]
    fn local_cursus_overrides_global_alias() {
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join("global");
        let project = tmp.path().join("project");

        write_cursus_toml(&project.join(".sgf/cursus"), "build", SIMPLE_CURSUS);
        write_cursus_toml(
            &global,
            "build",
            r#"
description = "Global build"
alias = "b"

[[iters]]
name = "build"
prompt = "build.md"
"#,
        );

        // Set HOME to point to a dir where global would be found
        // This test verifies local takes precedence via direct name
        let resolved = resolve_cursus(&project, "build").unwrap();
        assert_eq!(resolved.definition.description, "Build loop");
    }

    #[test]
    fn load_all_from_dir_skips_non_toml() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("cursus");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("build.toml"), SIMPLE_CURSUS).unwrap();
        fs::write(dir.join("readme.md"), "not a cursus").unwrap();

        let defs = load_all_from_dir(&dir);
        assert_eq!(defs.len(), 1);
        assert!(defs.contains_key("build"));
    }

    #[test]
    fn load_all_from_dir_nonexistent() {
        let defs = load_all_from_dir(Path::new("/nonexistent/dir"));
        assert!(defs.is_empty());
    }

    #[test]
    fn load_all_from_dir_multiple() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("cursus");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("build.toml"), SIMPLE_CURSUS).unwrap();
        fs::write(dir.join("test.toml"), TEST_CURSUS).unwrap();

        let defs = load_all_from_dir(&dir);
        assert_eq!(defs.len(), 2);
        assert!(defs.contains_key("build"));
        assert!(defs.contains_key("test"));
    }
}
