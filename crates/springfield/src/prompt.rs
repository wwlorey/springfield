use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_PROMPT_FILES: &str = "$HOME/.MEMENTO.md:./BACKPRESSURE.md:./specs/README.md";

pub fn resolve_system_prompt_files(root: &Path) -> Vec<PathBuf> {
    let raw = match std::env::var("PROMPT_FILES") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Warning: PROMPT_FILES not set, using default");
            DEFAULT_PROMPT_FILES.to_string()
        }
    };
    let home = std::env::var("HOME").unwrap_or_default();

    raw.split(':')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let expanded = entry.replace("$HOME", &home).replace('~', &home);
            let path = if expanded.starts_with("./") || expanded.starts_with("../") {
                root.join(&expanded)
            } else {
                PathBuf::from(&expanded)
            };
            if path.exists() {
                Some(path)
            } else {
                eprintln!("Warning: prompt file not found: {}", path.display());
                None
            }
        })
        .collect()
}

pub fn assemble(
    root: &Path,
    stage: &str,
    vars: &HashMap<String, String>,
    prepend_system_files: bool,
) -> io::Result<PathBuf> {
    let template_path = root.join(format!(".sgf/prompts/{stage}.md"));
    if !template_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("template not found: {}", template_path.display()),
        ));
    }

    let mut content = fs::read_to_string(&template_path)?;

    for (key, value) in vars {
        content = content.replace(&format!("{{{{{key}}}}}"), value);
    }

    let unresolved: Vec<String> = find_unresolved_tokens(&content);
    if !unresolved.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unresolved template variables: {}", unresolved.join(", ")),
        ));
    }

    let final_content = if prepend_system_files {
        let files = resolve_system_prompt_files(root);
        let mut parts: Vec<String> = Vec::new();
        for file in files {
            if let Ok(text) = fs::read_to_string(&file) {
                parts.push(text);
            }
        }
        if parts.is_empty() {
            content
        } else {
            parts.push(content);
            parts.join("\n")
        }
    } else {
        content
    };

    let assembled_dir = root.join(".sgf/prompts/.assembled");
    fs::create_dir_all(&assembled_dir)?;

    let output_path = assembled_dir.join(format!("{stage}.md"));
    fs::write(&output_path, &final_content)?;

    Ok(output_path)
}

fn find_unresolved_tokens(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let token = &after_open[..end];
            if !token.is_empty() && !token.contains('\n') {
                tokens.push(format!("{{{{{token}}}}}"));
            }
            rest = &after_open[end + 2..];
        } else {
            break;
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project(tmp: &Path) {
        fs::create_dir_all(tmp.join(".sgf/prompts/.assembled")).unwrap();
    }

    #[test]
    fn substitutes_spec_variable() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "Run `pn ready --spec {{spec}} --json`.\n",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("spec".to_string(), "auth".to_string());

        let path = assemble(tmp.path(), "build", &vars, false).unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(content, "Run `pn ready --spec auth --json`.\n");
        assert!(path.ends_with(".sgf/prompts/.assembled/build.md"));
    }

    #[test]
    fn substitutes_multiple_occurrences() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "Build {{spec}} then test {{spec}}.",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("spec".to_string(), "auth".to_string());

        let path = assemble(tmp.path(), "build", &vars, false).unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(content, "Build auth then test auth.");
    }

    #[test]
    fn passthrough_without_variables() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let original = "No variables here, just plain text.\n";
        fs::write(tmp.path().join(".sgf/prompts/verify.md"), original).unwrap();

        let vars = HashMap::new();
        let path = assemble(tmp.path(), "verify", &vars, false).unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(content, original);
    }

    #[test]
    fn error_on_unresolved_token() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "Hello {{unknown}} world.",
        )
        .unwrap();

        let vars = HashMap::new();
        let err = assemble(tmp.path(), "build", &vars, false).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let msg = err.to_string();
        assert!(
            msg.contains("{{unknown}}"),
            "error should name the token: {msg}"
        );
    }

    #[test]
    fn error_on_missing_template() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let vars = HashMap::new();
        let err = assemble(tmp.path(), "nonexistent", &vars, false).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("template not found"));
        assert!(err.to_string().contains("nonexistent.md"));
    }

    #[test]
    fn creates_assembled_dir_if_absent() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        // Note: .assembled/ directory does NOT exist yet
        fs::write(tmp.path().join(".sgf/prompts/verify.md"), "plain text").unwrap();

        let vars = HashMap::new();
        let path = assemble(tmp.path(), "verify", &vars, false).unwrap();

        assert!(path.exists());
        assert!(tmp.path().join(".sgf/prompts/.assembled").is_dir());
    }

    #[test]
    fn error_lists_multiple_unresolved_tokens() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "{{foo}} and {{bar}}",
        )
        .unwrap();

        let vars = HashMap::new();
        let err = assemble(tmp.path(), "build", &vars, false).unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("{{foo}}"), "should list {{foo}}: {msg}");
        assert!(msg.contains("{{bar}}"), "should list {{bar}}: {msg}");
    }

    #[test]
    fn partial_substitution_errors_on_remaining() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "{{spec}} and {{unknown}}",
        )
        .unwrap();

        let mut vars = HashMap::new();
        vars.insert("spec".to_string(), "auth".to_string());

        let err = assemble(tmp.path(), "build", &vars, false).unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("{{unknown}}"),
            "should report unresolved: {msg}"
        );
        assert!(
            !msg.contains("{{spec}}"),
            "should not report resolved: {msg}"
        );
    }

    #[test]
    fn prepend_false_skips_system_files() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(tmp.path().join(".sgf/prompts/build.md"), "template body").unwrap();

        let vars = HashMap::new();
        let path = assemble(tmp.path(), "build", &vars, false).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "template body");
    }
}
