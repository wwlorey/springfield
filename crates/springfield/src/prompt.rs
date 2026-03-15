use std::io;
use std::path::{Path, PathBuf};

fn global_prompts_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".sgf/prompts"))
}

pub fn resolve(root: &Path, name: &str) -> Option<PathBuf> {
    let local = root.join(format!(".sgf/prompts/{name}.md"));
    if local.exists() {
        return Some(local);
    }
    let global = global_prompts_dir()?.join(format!("{name}.md"));
    if global.exists() {
        return Some(global);
    }
    None
}

pub fn validate(root: &Path, stage: &str, spec: Option<&str>) -> io::Result<PathBuf> {
    let template_path = resolve(root, stage).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("prompt not found: {stage}.md (checked .sgf/prompts/ and ~/.sgf/prompts/)"),
        )
    })?;

    if let Some(stem) = spec {
        let spec_path = root.join(format!("specs/{stem}.md"));
        if !spec_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("spec not found: specs/{stem}.md"),
            ));
        }
    }

    Ok(template_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_project(tmp: &Path) {
        fs::create_dir_all(tmp.join(".sgf/prompts")).unwrap();
    }

    #[test]
    fn validate_existing_template() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(tmp.path().join(".sgf/prompts/build.md"), "Build prompt.").unwrap();

        let path = validate(tmp.path(), "build", None).unwrap();
        assert!(path.ends_with(".sgf/prompts/build.md"));
    }

    #[test]
    fn validate_missing_template() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());

        let err = validate(tmp.path(), "nonexistent", None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("prompt not found"));
        assert!(err.to_string().contains("nonexistent.md"));
    }

    #[test]
    fn validate_spec_exists() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join("specs")).unwrap();
        fs::write(tmp.path().join(".sgf/prompts/build.md"), "Build prompt.").unwrap();
        fs::write(tmp.path().join("specs/auth.md"), "# Auth spec").unwrap();

        let path = validate(tmp.path(), "build", Some("auth")).unwrap();
        assert!(path.ends_with(".sgf/prompts/build.md"));
    }

    #[test]
    fn validate_spec_missing() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(tmp.path().join(".sgf/prompts/build.md"), "Build prompt.").unwrap();

        let err = validate(tmp.path(), "build", Some("auth")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("spec not found: specs/auth.md"));
    }

    #[test]
    fn validate_dynamic_command_name() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/install.md"),
            "Install prompt.",
        )
        .unwrap();

        let path = validate(tmp.path(), "install", None).unwrap();
        assert!(path.ends_with(".sgf/prompts/install.md"));
    }

    #[test]
    fn validate_custom_command_name() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::write(
            tmp.path().join(".sgf/prompts/deploy-staging.md"),
            "Deploy prompt.",
        )
        .unwrap();

        let path = validate(tmp.path(), "deploy-staging", None).unwrap();
        assert!(path.ends_with(".sgf/prompts/deploy-staging.md"));
    }

    #[test]
    fn validate_custom_command_with_spec() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        fs::create_dir_all(tmp.path().join("specs")).unwrap();
        fs::write(
            tmp.path().join(".sgf/prompts/install.md"),
            "Install prompt.",
        )
        .unwrap();
        fs::write(tmp.path().join("specs/ralph.md"), "# Ralph spec").unwrap();

        let path = validate(tmp.path(), "install", Some("ralph")).unwrap();
        assert!(path.ends_with(".sgf/prompts/install.md"));
    }

    #[test]
    fn validate_returns_raw_path() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path());
        let content = "No variables here, just plain text.";
        fs::write(tmp.path().join(".sgf/prompts/verify.md"), content).unwrap();

        let path = validate(tmp.path(), "verify", None).unwrap();
        let read_back = fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, content);
    }
}
