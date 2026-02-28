use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

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

const GITIGNORE_FULL: &str = "\
# Springfield
.pensa/db.sqlite
.sgf/logs/
.sgf/run/
.sgf/prompts/.assembled/
.ralph-complete
.ralph-ding

# Rust
/target

# Node
node_modules/

# SvelteKit
.svelte-kit/

# Environment
.env
.env.local
.env.*.local

# macOS
.DS_Store
";

const GITIGNORE_ENTRIES: &[&str] = &[
    ".pensa/db.sqlite",
    ".sgf/logs/",
    ".sgf/run/",
    ".sgf/prompts/.assembled/",
    ".ralph-complete",
    ".ralph-ding",
    "/target",
    "node_modules/",
    ".svelte-kit/",
    ".env",
    ".env.local",
    ".env.*.local",
    ".DS_Store",
];

const CLAUDE_SETTINGS_DENY_RULES: &[&str] = &[
    "Edit .sgf/**",
    "Write .sgf/**",
    "Bash rm .sgf/**",
    "Bash mv .sgf/**",
];

const PRE_COMMIT_YAML_FULL: &str = "\
repos:
  - repo: local
    hooks:
      - id: pensa-export
        name: pensa export
        entry: pn export
        language: system
        always_run: true
        stages: [pre-commit]
      - id: pensa-import
        name: pensa import
        entry: pn import
        language: system
        always_run: true
        stages: [post-merge, post-checkout, post-rewrite]
";

fn merge_gitignore(root: &Path) -> io::Result<()> {
    let path = root.join(".gitignore");
    if !path.exists() {
        return fs::write(&path, GITIGNORE_FULL);
    }

    let existing = fs::read_to_string(&path)?;
    let existing_lines: Vec<&str> = existing.lines().map(|l| l.trim()).collect();

    let mut to_add: Vec<&str> = Vec::new();
    for entry in GITIGNORE_ENTRIES {
        if !existing_lines.contains(entry) {
            to_add.push(entry);
        }
    }

    if to_add.is_empty() {
        return Ok(());
    }

    let mut content = existing;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str("# Springfield\n");
    for entry in to_add {
        content.push_str(entry);
        content.push('\n');
    }
    fs::write(&path, content)
}

fn merge_claude_settings(root: &Path) -> io::Result<()> {
    let dir = root.join(".claude");
    fs::create_dir_all(&dir)?;
    let path = dir.join("settings.json");

    let mut doc: Value = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
    } else {
        serde_json::json!({})
    };

    let permissions = doc
        .as_object_mut()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "settings.json root is not an object",
            )
        })?
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));

    let deny = permissions
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "permissions is not an object"))?
        .entry("deny")
        .or_insert_with(|| serde_json::json!([]));

    let deny_arr = deny
        .as_array_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "deny is not an array"))?;

    for rule in CLAUDE_SETTINGS_DENY_RULES {
        let rule_val = Value::String(rule.to_string());
        if !deny_arr.contains(&rule_val) {
            deny_arr.push(rule_val);
        }
    }

    let formatted = serde_json::to_string_pretty(&doc).map_err(io::Error::other)?;
    fs::write(&path, format!("{formatted}\n"))
}

fn merge_pre_commit_config(root: &Path) -> io::Result<()> {
    let path = root.join(".pre-commit-config.yaml");
    if !path.exists() {
        return fs::write(&path, PRE_COMMIT_YAML_FULL);
    }

    let content = fs::read_to_string(&path)?;
    let mut doc: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let has_hook = |doc: &serde_yaml::Value, hook_id: &str| -> bool {
        doc.get("repos")
            .and_then(|r| r.as_sequence())
            .map(|repos| {
                repos.iter().any(|repo| {
                    repo.get("hooks")
                        .and_then(|h| h.as_sequence())
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                hook.get("id")
                                    .and_then(|id| id.as_str())
                                    .is_some_and(|id| id == hook_id)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    let has_export = has_hook(&doc, "pensa-export");
    let has_import = has_hook(&doc, "pensa-import");

    if has_export && has_import {
        return Ok(());
    }

    let repos = doc
        .as_mapping_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "yaml root is not a mapping"))?
        .entry(serde_yaml::Value::String("repos".to_string()))
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));

    let repos_seq = repos
        .as_sequence_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "repos is not a sequence"))?;

    // Find or create the local repo entry
    let local_repo_idx = repos_seq.iter().position(|repo| {
        repo.get("repo")
            .and_then(|r| r.as_str())
            .is_some_and(|r| r == "local")
    });

    let local_repo = if let Some(idx) = local_repo_idx {
        &mut repos_seq[idx]
    } else {
        let new_repo: serde_yaml::Value =
            serde_yaml::from_str("repo: local\nhooks: []").map_err(io::Error::other)?;
        repos_seq.push(new_repo);
        repos_seq.last_mut().unwrap()
    };

    let hooks = local_repo
        .as_mapping_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "repo entry is not a mapping"))?
        .entry(serde_yaml::Value::String("hooks".to_string()))
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));

    let hooks_seq = hooks
        .as_sequence_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "hooks is not a sequence"))?;

    if !has_export {
        let export_hook: serde_yaml::Value = serde_yaml::from_str(
            "id: pensa-export\nname: pensa export\nentry: pn export\nlanguage: system\nalways_run: true\nstages: [pre-commit]",
        )
        .map_err(io::Error::other)?;
        hooks_seq.push(export_hook);
    }

    if !has_import {
        let import_hook: serde_yaml::Value = serde_yaml::from_str(
            "id: pensa-import\nname: pensa import\nentry: pn import\nlanguage: system\nalways_run: true\nstages: [post-merge, post-checkout, post-rewrite]",
        )
        .map_err(io::Error::other)?;
        hooks_seq.push(import_hook);
    }

    let output = serde_yaml::to_string(&doc).map_err(io::Error::other)?;
    fs::write(&path, output)
}

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

    merge_gitignore(root)?;
    merge_claude_settings(root)?;
    merge_pre_commit_config(root)?;

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

    // --- .gitignore tests ---

    #[test]
    fn gitignore_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        for entry in GITIGNORE_ENTRIES {
            assert!(
                content.lines().any(|l| l.trim() == *entry),
                "missing gitignore entry: {entry}"
            );
        }
        assert!(content.contains("# Springfield"));
    }

    #[test]
    fn gitignore_merges_with_existing() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".gitignore"), "# Custom\nmy-secret.key\n").unwrap();

        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(content.contains("my-secret.key"), "custom entry lost");
        for entry in GITIGNORE_ENTRIES {
            assert!(
                content.lines().any(|l| l.trim() == *entry),
                "missing gitignore entry after merge: {entry}"
            );
        }
    }

    #[test]
    fn gitignore_no_duplicates_on_rerun() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();
        let first = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

        run(tmp.path()).unwrap();
        let second = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

        assert_eq!(first, second, ".gitignore changed on second run");
    }

    #[test]
    fn gitignore_partial_existing_entries() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".gitignore"), "/target\n.DS_Store\n").unwrap();

        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        let target_count = content.lines().filter(|l| l.trim() == "/target").count();
        assert_eq!(target_count, 1, "/target duplicated");
        let ds_count = content.lines().filter(|l| l.trim() == ".DS_Store").count();
        assert_eq!(ds_count, 1, ".DS_Store duplicated");
        assert!(
            content.lines().any(|l| l.trim() == ".pensa/db.sqlite"),
            "missing new entry"
        );
    }

    // --- .claude/settings.json tests ---

    #[test]
    fn settings_json_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        let deny = doc["permissions"]["deny"].as_array().unwrap();

        for rule in CLAUDE_SETTINGS_DENY_RULES {
            assert!(
                deny.contains(&Value::String(rule.to_string())),
                "missing deny rule: {rule}"
            );
        }
        assert_eq!(deny.len(), 4);
    }

    #[test]
    fn settings_json_merges_with_existing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        fs::write(
            tmp.path().join(".claude/settings.json"),
            r#"{"permissions":{"deny":["Bash rm -rf /"]}}"#,
        )
        .unwrap();

        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        let deny = doc["permissions"]["deny"].as_array().unwrap();

        assert!(
            deny.contains(&Value::String("Bash rm -rf /".to_string())),
            "custom deny rule lost"
        );
        for rule in CLAUDE_SETTINGS_DENY_RULES {
            assert!(
                deny.contains(&Value::String(rule.to_string())),
                "missing deny rule after merge: {rule}"
            );
        }
        assert_eq!(deny.len(), 5);
    }

    #[test]
    fn settings_json_no_duplicates_on_rerun() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        let deny = doc["permissions"]["deny"].as_array().unwrap();
        assert_eq!(deny.len(), 4, "deny rules duplicated on rerun");
    }

    // --- .pre-commit-config.yaml tests ---

    #[test]
    fn pre_commit_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        assert!(content.contains("pensa-export"));
        assert!(content.contains("pensa-import"));
        assert!(content.contains("pn export"));
        assert!(content.contains("pn import"));
    }

    #[test]
    fn pre_commit_merges_with_existing() {
        let tmp = TempDir::new().unwrap();
        let existing = "\
repos:
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.5.0
    hooks:
      - id: trailing-whitespace
";
        fs::write(tmp.path().join(".pre-commit-config.yaml"), existing).unwrap();

        run(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        assert!(
            content.contains("trailing-whitespace"),
            "existing hook lost"
        );
        assert!(content.contains("pensa-export"), "pensa-export not added");
        assert!(content.contains("pensa-import"), "pensa-import not added");
    }

    #[test]
    fn pre_commit_no_duplicates_on_rerun() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();
        let first = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        let first_export_count = first.matches("pensa-export").count();

        run(tmp.path()).unwrap();
        let second = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        let second_export_count = second.matches("pensa-export").count();

        assert_eq!(
            first_export_count, second_export_count,
            "pensa-export duplicated on rerun"
        );
    }

    // --- Full idempotency including config files ---

    #[test]
    fn full_init_idempotent_with_config_files() {
        let tmp = TempDir::new().unwrap();
        run(tmp.path()).unwrap();

        let gitignore1 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        let settings1 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let precommit1 = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();

        run(tmp.path()).unwrap();

        let gitignore2 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        let settings2 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let precommit2 = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();

        assert_eq!(gitignore1, gitignore2, ".gitignore changed on second run");
        assert_eq!(settings1, settings2, "settings.json changed on second run");
        assert_eq!(
            precommit1, precommit2,
            ".pre-commit-config.yaml changed on second run"
        );
    }
}
