use std::fs;
use std::io;
use std::path::Path;

const TEMPLATE_BACKPRESSURE: &str = include_str!("../templates/backpressure.md");
const TEMPLATE_SPEC: &str = include_str!("../templates/spec.md");
const TEMPLATE_BUILD: &str = include_str!("../templates/build.md");
const TEMPLATE_VERIFY: &str = include_str!("../templates/verify.md");
const TEMPLATE_TEST_PLAN: &str = include_str!("../templates/test-plan.md");
const TEMPLATE_TEST: &str = include_str!("../templates/test.md");
const TEMPLATE_ISSUES: &str = include_str!("../templates/issues.md");
const TEMPLATE_ISSUES_PLAN: &str = include_str!("../templates/issues-plan.md");

const MEMENTO_CONTENT: &str = "\
# Memento

## Stack

<!-- Replace with your project's stack (e.g., Rust, TypeScript, Tauri, Go) -->

## References

- Build, test, lint, format commands: `.sgf/backpressure.md`
- Spec index: `specs/README.md`
- Issue and task tracking: `pn` CLI (pensa)
";

const CLAUDE_MD_CONTENT: &str = "Read memento.md and AGENTS.md before starting work.\n";

const SPECS_README_CONTENT: &str = "\
# Specs

| Spec | Code | Purpose |
|------|------|---------|
";

const DIRECTORIES: &[&str] = &[
    ".pensa",
    ".sgf",
    ".sgf/logs",
    ".sgf/run",
    ".sgf/prompts",
    ".sgf/prompts/.assembled",
    "specs",
];

struct TemplateFile {
    path: &'static str,
    content: &'static str,
}

const TEMPLATE_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: ".sgf/backpressure.md",
        content: TEMPLATE_BACKPRESSURE,
    },
    TemplateFile {
        path: ".sgf/prompts/spec.md",
        content: TEMPLATE_SPEC,
    },
    TemplateFile {
        path: ".sgf/prompts/build.md",
        content: TEMPLATE_BUILD,
    },
    TemplateFile {
        path: ".sgf/prompts/verify.md",
        content: TEMPLATE_VERIFY,
    },
    TemplateFile {
        path: ".sgf/prompts/test-plan.md",
        content: TEMPLATE_TEST_PLAN,
    },
    TemplateFile {
        path: ".sgf/prompts/test.md",
        content: TEMPLATE_TEST,
    },
    TemplateFile {
        path: ".sgf/prompts/issues.md",
        content: TEMPLATE_ISSUES,
    },
    TemplateFile {
        path: ".sgf/prompts/issues-plan.md",
        content: TEMPLATE_ISSUES_PLAN,
    },
];

struct SkeletonFile {
    path: &'static str,
    content: &'static str,
}

const SKELETON_FILES: &[SkeletonFile] = &[
    SkeletonFile {
        path: "memento.md",
        content: MEMENTO_CONTENT,
    },
    SkeletonFile {
        path: "CLAUDE.md",
        content: CLAUDE_MD_CONTENT,
    },
    SkeletonFile {
        path: "specs/README.md",
        content: SPECS_README_CONTENT,
    },
];

fn create_directories(root: &Path) -> io::Result<()> {
    for dir in DIRECTORIES {
        let path = root.join(dir);
        fs::create_dir_all(&path)?;
    }
    Ok(())
}

fn write_if_missing(path: &Path, content: &str) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

pub fn run(root: &Path) -> io::Result<()> {
    create_directories(root)?;

    for tf in TEMPLATE_FILES {
        write_if_missing(&root.join(tf.path), tf.content)?;
    }

    for sf in SKELETON_FILES {
        write_if_missing(&root.join(sf.path), sf.content)?;
    }

    println!("sgf init: project scaffolded successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_all_directories() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        for dir in DIRECTORIES {
            assert!(tmp.path().join(dir).is_dir(), "directory missing: {dir}");
        }
    }

    #[test]
    fn creates_all_template_files() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        for tf in TEMPLATE_FILES {
            let path = tmp.path().join(tf.path);
            assert!(path.is_file(), "template file missing: {}", tf.path);
            let content = fs::read_to_string(&path).unwrap();
            assert_eq!(content, tf.content, "content mismatch: {}", tf.path);
        }
    }

    #[test]
    fn creates_all_skeleton_files() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        for sf in SKELETON_FILES {
            let path = tmp.path().join(sf.path);
            assert!(path.is_file(), "skeleton file missing: {}", sf.path);
            let content = fs::read_to_string(&path).unwrap();
            assert_eq!(content, sf.content, "content mismatch: {}", sf.path);
        }
    }

    #[test]
    fn claude_md_content() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert!(content.contains("Read memento.md and AGENTS.md"));
    }

    #[test]
    fn memento_content() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join("memento.md")).unwrap();
        assert!(content.contains("## Stack"));
        assert!(content.contains("## References"));
    }

    #[test]
    fn does_not_overwrite_existing_files() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let modified = "custom content";
        fs::write(tmp.path().join("CLAUDE.md"), modified).unwrap();
        fs::write(tmp.path().join(".sgf/prompts/build.md"), modified).unwrap();

        run(tmp.path()).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap(),
            modified
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join(".sgf/prompts/build.md")).unwrap(),
            modified
        );
    }

    #[test]
    fn idempotent_run() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let first_run: Vec<(String, String)> = TEMPLATE_FILES
            .iter()
            .map(|tf| {
                (
                    tf.path.to_string(),
                    fs::read_to_string(tmp.path().join(tf.path)).unwrap(),
                )
            })
            .chain(SKELETON_FILES.iter().map(|sf| {
                (
                    sf.path.to_string(),
                    fs::read_to_string(tmp.path().join(sf.path)).unwrap(),
                )
            }))
            .collect();

        run(tmp.path()).unwrap();

        for (path, content) in &first_run {
            let after = fs::read_to_string(tmp.path().join(path)).unwrap();
            assert_eq!(&after, content, "file changed on second run: {path}");
        }
    }
}
