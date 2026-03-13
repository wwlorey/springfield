use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs as unix_fs;

use serde_json::Value;

const TEMPLATE_BACKPRESSURE: &str = include_str!("../templates/backpressure.md");
const TEMPLATE_SPEC: &str = include_str!("../templates/spec.md");
const TEMPLATE_BUILD: &str = include_str!("../templates/build.md");
const TEMPLATE_VERIFY: &str = include_str!("../templates/verify.md");
const TEMPLATE_TEST_PLAN: &str = include_str!("../templates/test-plan.md");
const TEMPLATE_TEST: &str = include_str!("../templates/test.md");
const TEMPLATE_ISSUES_LOG: &str = include_str!("../templates/issues-log.md");
const TEMPLATE_DOC: &str = include_str!("../templates/doc.md");
const TEMPLATE_INSTALL: &str = include_str!("../templates/install.md");
const TEMPLATE_CONFIG_TOML: &str = include_str!("../templates/config.toml");
const TEMPLATE_LOOM_SPECS_README: &str = include_str!("../templates/loom-specs-README.md");

const SPECS_README_CONTENT: &str = "\
# Specifications

| Spec | Code | Purpose |
|------|------|---------|
";

const DIRECTORIES: &[&str] = &[
    ".pensa",
    ".sgf",
    ".sgf/logs",
    ".sgf/run",
    ".sgf/prompts",
    "specs",
];

struct TemplateFile {
    path: &'static str,
    content: &'static str,
}

const TEMPLATE_FILES: &[TemplateFile] = &[
    TemplateFile {
        path: "BACKPRESSURE.md",
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
        path: ".sgf/prompts/issues-log.md",
        content: TEMPLATE_ISSUES_LOG,
    },
    TemplateFile {
        path: ".sgf/prompts/doc.md",
        content: TEMPLATE_DOC,
    },
    TemplateFile {
        path: ".sgf/prompts/install.md",
        content: TEMPLATE_INSTALL,
    },
    TemplateFile {
        path: ".sgf/prompts/config.toml",
        content: TEMPLATE_CONFIG_TOML,
    },
    TemplateFile {
        path: ".sgf/loom-specs-README.md",
        content: TEMPLATE_LOOM_SPECS_README,
    },
];

struct SkeletonFile {
    path: &'static str,
    content: &'static str,
}

const SKELETON_FILES: &[SkeletonFile] = &[SkeletonFile {
    path: "specs/README.md",
    content: SPECS_README_CONTENT,
}];

const GITIGNORE_FULL: &str = "\
# Springfield
.sgf/logs/
.sgf/run/
.ralph-complete
.ralph-ding
**/.pensa/daemon.port
**/.pensa/daemon.project
**/.pensa/daemon.url

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
    ".sgf/logs/",
    ".sgf/run/",
    ".ralph-complete",
    ".ralph-ding",
    "**/.pensa/daemon.port",
    "**/.pensa/daemon.project",
    "**/.pensa/daemon.url",
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
    "Edit .claude/**",
    "Write .claude/**",
    "Bash rm .claude/**",
    "Bash mv .claude/**",
];

const SANDBOX_ALLOWED_DOMAINS: &[&str] = &[
    "localhost",
    "github.com",
    "*.githubusercontent.com",
    "crates.io",
    "*.crates.io",
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
        pass_filenames: false
        stages: [pre-commit]
      - id: pensa-import
        name: pensa import
        entry: pn import
        language: system
        always_run: true
        pass_filenames: false
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

    let root_obj = doc.as_object_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.json root is not an object",
        )
    })?;

    let sandbox = root_obj
        .entry("sandbox")
        .or_insert_with(|| serde_json::json!({}));
    let sandbox_obj = sandbox
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "sandbox is not an object"))?;

    sandbox_obj
        .entry("enabled")
        .or_insert(serde_json::json!(true));
    sandbox_obj
        .entry("autoAllowBashIfSandboxed")
        .or_insert(serde_json::json!(true));

    let network = sandbox_obj
        .entry("network")
        .or_insert_with(|| serde_json::json!({}));
    let net_obj = network
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "network is not an object"))?;
    let allowed_domains = net_obj
        .entry("allowedDomains")
        .or_insert_with(|| serde_json::json!([]));
    let domains_arr = allowed_domains.as_array_mut().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "allowedDomains is not an array")
    })?;
    for domain in SANDBOX_ALLOWED_DOMAINS {
        let val = Value::String(domain.to_string());
        if !domains_arr.contains(&val) {
            domains_arr.push(val);
        }
    }
    net_obj
        .entry("allowLocalBinding")
        .or_insert(serde_json::json!(true));

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
            "id: pensa-export\nname: pensa export\nentry: pn export\nlanguage: system\nalways_run: true\npass_filenames: false\nstages: [pre-commit]",
        )
        .map_err(io::Error::other)?;
        hooks_seq.push(export_hook);
    }

    if !has_import {
        let import_hook: serde_yaml::Value = serde_yaml::from_str(
            "id: pensa-import\nname: pensa import\nentry: pn import\nlanguage: system\nalways_run: true\npass_filenames: false\nstages: [post-merge, post-checkout, post-rewrite]",
        )
        .map_err(io::Error::other)?;
        hooks_seq.push(import_hook);
    }

    let output = serde_yaml::to_string(&doc).map_err(io::Error::other)?;
    fs::write(&path, output)
}

const SHIPPED_CONFIG_SECTIONS: &[&str] = &[
    "install",
    "spec",
    "build",
    "verify",
    "test-plan",
    "test",
    "issues-log",
    "doc",
];

fn merge_config_toml(root: &Path) -> io::Result<()> {
    let path = root.join(".sgf/prompts/config.toml");
    if !path.exists() {
        return fs::write(&path, TEMPLATE_CONFIG_TOML);
    }

    let existing = fs::read_to_string(&path)?;
    let mut existing_table: toml::Table = existing
        .parse()
        .map_err(|e: toml::de::Error| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    let shipped_table: toml::Table = TEMPLATE_CONFIG_TOML
        .parse()
        .expect("shipped config.toml is valid");

    for (key, value) in &shipped_table {
        existing_table.insert(key.clone(), value.clone());
    }

    let mut output = String::new();
    let shipped_set: std::collections::HashSet<&str> =
        SHIPPED_CONFIG_SECTIONS.iter().copied().collect();

    let serialize = |table: toml::Table| -> io::Result<String> {
        toml::to_string_pretty(&table).map_err(io::Error::other)
    };

    for section in SHIPPED_CONFIG_SECTIONS {
        if let Some(value) = existing_table.remove(*section) {
            output.push_str(&serialize(toml::Table::from_iter([(
                section.to_string(),
                value,
            )]))?);
            output.push('\n');
        }
    }

    for (key, value) in &existing_table {
        if !shipped_set.contains(key.as_str()) {
            output.push_str(&serialize(toml::Table::from_iter([(
                key.clone(),
                value.clone(),
            )]))?);
            output.push('\n');
        }
    }

    fs::write(&path, output.trim_end().to_string() + "\n")
}

fn install_prek_hooks(root: &Path) -> io::Result<()> {
    let output = Command::new("prek")
        .arg("install")
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "prek install failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
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

fn check_git_clean(root: &Path, paths: &[&str]) -> io::Result<Vec<String>> {
    let mut problems = Vec::new();

    let output = Command::new("git")
        .args(["ls-files", "--"])
        .args(paths)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let tracked_output = String::from_utf8_lossy(&output.stdout).to_string();
    let tracked: HashSet<&str> = tracked_output.lines().collect();

    for p in paths {
        if !tracked.contains(*p) {
            problems.push(format!("{p} (untracked)"));
        }
    }

    let output = Command::new("git")
        .args(["status", "--porcelain", "--"])
        .args(paths)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let path = line
            .get(3..)
            .unwrap_or("")
            .split(" -> ")
            .next()
            .unwrap_or("");
        if tracked.contains(path) {
            problems.push(format!("{path} (uncommitted changes)"));
        }
    }

    Ok(problems)
}

fn confirm_overwrite(files: &[&str]) -> io::Result<bool> {
    let file_list = files.join(", ");
    crate::style::print_warning(&format!("overwriting: {file_list}"));
    eprint!("Overwrite {} files? [y/N] ", files.len());
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn write_force_files(root: &Path) -> io::Result<()> {
    for tf in TEMPLATE_FILES {
        let path = root.join(tf.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if tf.path == ".sgf/prompts/config.toml" {
            merge_config_toml(root)?;
        } else {
            fs::write(&path, tf.content)?;
        }
    }
    for sf in SKELETON_FILES {
        write_if_missing(&root.join(sf.path), sf.content)?;
    }
    Ok(())
}

pub fn run(root: &Path, force: bool) -> io::Result<()> {
    create_directories(root)?;

    if force {
        let all_paths: Vec<&str> = TEMPLATE_FILES
            .iter()
            .map(|tf| tf.path)
            .chain(
                SKELETON_FILES
                    .iter()
                    .filter(|sf| sf.path != "specs/README.md")
                    .map(|sf| sf.path),
            )
            .collect();
        let existing: Vec<&str> = all_paths
            .iter()
            .filter(|p| root.join(p).exists())
            .copied()
            .collect();

        if !existing.is_empty() {
            let problems = check_git_clean(root, &existing)?;
            if !problems.is_empty() {
                let list = problems.join("\n  ");
                return Err(io::Error::other(format!(
                    "cannot --force: the following files have issues:\n  {list}"
                )));
            }
            if !confirm_overwrite(&existing)? {
                return Err(io::Error::other("aborted"));
            }
        }

        write_force_files(root)?;
    } else {
        for tf in TEMPLATE_FILES {
            write_if_missing(&root.join(tf.path), tf.content)?;
        }
        for sf in SKELETON_FILES {
            write_if_missing(&root.join(sf.path), sf.content)?;
        }
    }

    // CLAUDE.md is a symlink to AGENTS.md
    let claude_md = root.join("CLAUDE.md");
    if claude_md.symlink_metadata().is_err() {
        #[cfg(unix)]
        unix_fs::symlink("AGENTS.md", &claude_md)?;
    }

    merge_gitignore(root)?;
    merge_claude_settings(root)?;
    merge_pre_commit_config(root)?;
    install_prek_hooks(root)?;

    crate::style::print_success("project scaffolded successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_init(path: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    fn git_add_commit(path: &Path, msg: &str) {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg, "--no-gpg-sign", "--no-verify"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn creates_all_directories() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        for dir in DIRECTORIES {
            assert!(tmp.path().join(dir).is_dir(), "directory missing: {dir}");
        }
    }

    #[test]
    fn creates_all_template_files() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

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
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        for sf in SKELETON_FILES {
            let path = tmp.path().join(sf.path);
            assert!(path.is_file(), "skeleton file missing: {}", sf.path);
            let content = fs::read_to_string(&path).unwrap();
            assert_eq!(content, sf.content, "content mismatch: {}", sf.path);
        }
    }

    #[test]
    fn claude_md_is_symlink() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let claude_md = tmp.path().join("CLAUDE.md");
        let meta = claude_md.symlink_metadata().unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "CLAUDE.md should be a symlink"
        );
        let target = fs::read_link(&claude_md).unwrap();
        assert_eq!(
            target.to_str().unwrap(),
            "AGENTS.md",
            "CLAUDE.md should point to AGENTS.md"
        );
    }

    #[test]
    fn no_memento_or_pensa_scaffolded() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        assert!(
            !tmp.path().join(".sgf/MEMENTO.md").exists(),
            ".sgf/MEMENTO.md should NOT be created"
        );
        assert!(
            !tmp.path().join(".sgf/PENSA.md").exists(),
            ".sgf/PENSA.md should NOT be created"
        );
    }

    #[test]
    fn does_not_overwrite_existing_files() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let modified = "custom content";
        fs::write(tmp.path().join(".sgf/prompts/build.md"), modified).unwrap();

        run(tmp.path(), false).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join(".sgf/prompts/build.md")).unwrap(),
            modified
        );

        // CLAUDE.md symlink should not be recreated
        let claude_md = tmp.path().join("CLAUDE.md");
        assert!(
            claude_md
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn idempotent_run() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

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

        run(tmp.path(), false).unwrap();

        for (path, content) in &first_run {
            let after = fs::read_to_string(tmp.path().join(path)).unwrap();
            assert_eq!(&after, content, "file changed on second run: {path}");
        }
    }

    #[test]
    fn prek_hooks_installed() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        assert!(
            tmp.path().join(".git/hooks/pre-commit").exists(),
            "pre-commit hook not installed"
        );
    }

    // --- .gitignore tests ---

    #[test]
    fn gitignore_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

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
        git_init(tmp.path());
        fs::write(tmp.path().join(".gitignore"), "# Custom\nmy-secret.key\n").unwrap();

        run(tmp.path(), false).unwrap();

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
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        let first = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

        run(tmp.path(), false).unwrap();
        let second = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

        assert_eq!(first, second, ".gitignore changed on second run");
    }

    #[test]
    fn gitignore_partial_existing_entries() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        fs::write(tmp.path().join(".gitignore"), "/target\n.DS_Store\n").unwrap();

        run(tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        let target_count = content.lines().filter(|l| l.trim() == "/target").count();
        assert_eq!(target_count, 1, "/target duplicated");
        let ds_count = content.lines().filter(|l| l.trim() == ".DS_Store").count();
        assert_eq!(ds_count, 1, ".DS_Store duplicated");
        assert!(
            content.lines().any(|l| l.trim() == ".ralph-complete"),
            "missing new entry"
        );
    }

    // --- .claude/settings.json tests ---

    #[test]
    fn settings_json_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        let deny = doc["permissions"]["deny"].as_array().unwrap();

        for rule in CLAUDE_SETTINGS_DENY_RULES {
            assert!(
                deny.contains(&Value::String(rule.to_string())),
                "missing deny rule: {rule}"
            );
        }
        assert_eq!(deny.len(), 8);

        assert_eq!(doc["sandbox"]["enabled"], true);
        assert_eq!(doc["sandbox"]["autoAllowBashIfSandboxed"], true);
        assert_eq!(doc["sandbox"]["network"]["allowLocalBinding"], true);

        let domains = doc["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap();
        for domain in SANDBOX_ALLOWED_DOMAINS {
            assert!(
                domains.contains(&Value::String(domain.to_string())),
                "missing allowedDomains entry: {domain}"
            );
        }
    }

    #[test]
    fn settings_json_merges_with_existing() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        fs::write(
            tmp.path().join(".claude/settings.json"),
            r#"{"permissions":{"deny":["Bash rm -rf /"]},"sandbox":{"enabled":false,"network":{"allowedDomains":["registry.npmjs.org"],"allowLocalBinding":false}}}"#,
        )
        .unwrap();

        run(tmp.path(), false).unwrap();

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
        assert_eq!(deny.len(), 9);

        assert_eq!(
            doc["sandbox"]["enabled"], false,
            "scalar should not be overwritten"
        );
        assert_eq!(
            doc["sandbox"]["network"]["allowLocalBinding"], false,
            "scalar should not be overwritten"
        );

        let domains = doc["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap();
        assert!(
            domains.contains(&Value::String("registry.npmjs.org".to_string())),
            "custom domain lost"
        );
        for domain in SANDBOX_ALLOWED_DOMAINS {
            assert!(
                domains.contains(&Value::String(domain.to_string())),
                "missing domain after merge: {domain}"
            );
        }
    }

    #[test]
    fn settings_json_no_duplicates_on_rerun() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        run(tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        let deny = doc["permissions"]["deny"].as_array().unwrap();
        assert_eq!(deny.len(), 8, "deny rules duplicated on rerun");

        let domains = doc["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap();
        assert_eq!(
            domains.len(),
            SANDBOX_ALLOWED_DOMAINS.len(),
            "allowedDomains duplicated on rerun"
        );
    }

    // --- .pre-commit-config.yaml tests ---

    #[test]
    fn pre_commit_created_from_scratch() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        assert!(content.contains("pensa-export"));
        assert!(content.contains("pensa-import"));
        assert!(content.contains("pn export"));
        assert!(content.contains("pn import"));
    }

    #[test]
    fn pre_commit_merges_with_existing() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        let existing = "\
repos:
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.5.0
    hooks:
      - id: trailing-whitespace
";
        fs::write(tmp.path().join(".pre-commit-config.yaml"), existing).unwrap();

        run(tmp.path(), false).unwrap();

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
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        let first = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        let first_export_count = first.matches("pensa-export").count();

        run(tmp.path(), false).unwrap();
        let second = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        let second_export_count = second.matches("pensa-export").count();

        assert_eq!(
            first_export_count, second_export_count,
            "pensa-export duplicated on rerun"
        );
    }

    #[test]
    fn pre_commit_hooks_have_pass_filenames_false() {
        // pn export/import don't accept filename args; without pass_filenames: false
        // pre-commit passes staged filenames causing "unexpected argument" errors.
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let content = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
        let hooks: Vec<&serde_yaml::Value> = doc["repos"]
            .as_sequence()
            .unwrap()
            .iter()
            .flat_map(|repo| repo["hooks"].as_sequence().into_iter().flatten())
            .filter(|hook| {
                hook["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("pensa-"))
            })
            .collect();

        assert!(!hooks.is_empty(), "no pensa hooks found");
        for hook in &hooks {
            let id = hook["id"].as_str().unwrap();
            assert_eq!(
                hook["pass_filenames"].as_bool(),
                Some(false),
                "{id} missing pass_filenames: false"
            );
        }
    }

    #[test]
    fn pre_commit_merge_path_matches_template() {
        // Ensure hooks added via the merge path (existing config) have the same
        // properties as those from the fresh-template path.
        let tmp_fresh = TempDir::new().unwrap();
        git_init(tmp_fresh.path());
        run(tmp_fresh.path(), false).unwrap();

        let tmp_merge = TempDir::new().unwrap();
        git_init(tmp_merge.path());
        // Seed with an unrelated hook so merge_pre_commit_config takes the merge path
        fs::write(
            tmp_merge.path().join(".pre-commit-config.yaml"),
            "repos:\n  - repo: https://example.com\n    rev: v1\n    hooks:\n      - id: dummy\n",
        )
        .unwrap();
        run(tmp_merge.path(), false).unwrap();

        let fresh: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(tmp_fresh.path().join(".pre-commit-config.yaml")).unwrap(),
        )
        .unwrap();
        let merged: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(tmp_merge.path().join(".pre-commit-config.yaml")).unwrap(),
        )
        .unwrap();

        let extract_hook = |doc: &serde_yaml::Value, hook_id: &str| -> serde_yaml::Value {
            doc["repos"]
                .as_sequence()
                .unwrap()
                .iter()
                .flat_map(|repo| repo["hooks"].as_sequence().into_iter().flatten())
                .find(|h| h["id"].as_str() == Some(hook_id))
                .unwrap()
                .clone()
        };

        for hook_id in &["pensa-export", "pensa-import"] {
            let from_fresh = extract_hook(&fresh, hook_id);
            let from_merge = extract_hook(&merged, hook_id);
            assert_eq!(
                from_fresh, from_merge,
                "{hook_id} differs between fresh and merge paths"
            );
        }
    }

    // --- Full idempotency including config files ---

    #[test]
    fn full_init_idempotent_with_config_files() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let gitignore1 = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        let settings1 = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let precommit1 = fs::read_to_string(tmp.path().join(".pre-commit-config.yaml")).unwrap();

        run(tmp.path(), false).unwrap();

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

    // --- --force tests ---

    /// Non-interactive force init: git safety check + write files, no prompt.
    fn force_init(root: &Path) -> io::Result<()> {
        create_directories(root)?;

        let all_paths: Vec<&str> = TEMPLATE_FILES
            .iter()
            .map(|tf| tf.path)
            .chain(
                SKELETON_FILES
                    .iter()
                    .filter(|sf| sf.path != "specs/README.md")
                    .map(|sf| sf.path),
            )
            .collect();
        let existing: Vec<&str> = all_paths
            .iter()
            .filter(|p| root.join(p).exists())
            .copied()
            .collect();

        if !existing.is_empty() {
            let problems = check_git_clean(root, &existing)?;
            if !problems.is_empty() {
                let list = problems.join("\n  ");
                return Err(io::Error::other(format!(
                    "cannot --force: the following files have issues:\n  {list}"
                )));
            }
        }

        write_force_files(root)
    }

    #[test]
    fn backpressure_at_root() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        assert!(
            tmp.path().join("BACKPRESSURE.md").is_file(),
            "BACKPRESSURE.md should exist at project root"
        );
    }

    #[test]
    fn backpressure_not_in_sgf() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        assert!(
            !tmp.path().join(".sgf/BACKPRESSURE.md").exists(),
            "BACKPRESSURE.md should NOT exist inside .sgf/"
        );
    }

    #[test]
    fn force_overwrites_existing_files() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        git_add_commit(tmp.path(), "init");

        let build_path = tmp.path().join(".sgf/prompts/build.md");
        fs::write(&build_path, "custom content").unwrap();
        git_add_commit(tmp.path(), "modify build.md");

        force_init(tmp.path()).unwrap();

        let content = fs::read_to_string(&build_path).unwrap();
        assert_eq!(
            content, TEMPLATE_BUILD,
            "force should restore template content"
        );
    }

    #[test]
    fn force_does_not_overwrite_specs_readme() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let readme_path = tmp.path().join("specs/README.md");
        let custom = "# My Specs\n\nCustom content\n";
        fs::write(&readme_path, custom).unwrap();
        git_add_commit(tmp.path(), "customize specs readme");

        force_init(tmp.path()).unwrap();

        let content = fs::read_to_string(&readme_path).unwrap();
        assert_eq!(
            content, custom,
            "force should not overwrite specs/README.md"
        );
    }

    #[test]
    fn force_fails_on_uncommitted_changes() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        git_add_commit(tmp.path(), "init");

        fs::write(tmp.path().join(".sgf/prompts/build.md"), "dirty").unwrap();

        let err = force_init(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("uncommitted changes"),
            "expected uncommitted changes error, got: {err}"
        );
    }

    #[test]
    fn force_fails_on_untracked_file() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());

        create_directories(tmp.path()).unwrap();
        fs::write(
            tmp.path().join(".sgf/prompts/build.md"),
            "untracked content",
        )
        .unwrap();

        let err = force_init(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("untracked"),
            "expected untracked error, got: {err}"
        );
    }

    #[test]
    fn sandbox_scalars_not_overwritten_when_present() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        fs::write(
            tmp.path().join(".claude/settings.json"),
            r#"{"sandbox":{"enabled":false,"autoAllowBashIfSandboxed":false,"network":{"allowLocalBinding":false}}}"#,
        )
        .unwrap();

        merge_claude_settings(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["sandbox"]["enabled"], false);
        assert_eq!(doc["sandbox"]["autoAllowBashIfSandboxed"], false);
        assert_eq!(doc["sandbox"]["network"]["allowLocalBinding"], false);
    }

    #[test]
    fn sandbox_arrays_merge_additively() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        fs::write(
            tmp.path().join(".claude/settings.json"),
            r#"{"sandbox":{"network":{"allowedDomains":["custom.example.com"]}}}"#,
        )
        .unwrap();

        merge_claude_settings(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();

        let domains = doc["sandbox"]["network"]["allowedDomains"]
            .as_array()
            .unwrap();
        assert!(domains.contains(&Value::String("custom.example.com".to_string())));
        for domain in SANDBOX_ALLOWED_DOMAINS {
            assert!(
                domains.contains(&Value::String(domain.to_string())),
                "missing domain: {domain}"
            );
        }
        assert_eq!(domains.len(), SANDBOX_ALLOWED_DOMAINS.len() + 1);
    }

    #[test]
    fn sandbox_empty_existing_file() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        fs::write(tmp.path().join(".claude/settings.json"), "{}").unwrap();

        merge_claude_settings(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(doc["sandbox"]["enabled"], true);
        assert_eq!(doc["sandbox"]["autoAllowBashIfSandboxed"], true);
        assert_eq!(doc["sandbox"]["network"]["allowLocalBinding"], true);
        assert!(doc["sandbox"]["filesystem"].is_null());
        assert_eq!(
            doc["sandbox"]["network"]["allowedDomains"]
                .as_array()
                .unwrap()
                .len(),
            SANDBOX_ALLOWED_DOMAINS.len()
        );
    }

    #[test]
    fn force_writes_missing_files_normally() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());

        force_init(tmp.path()).unwrap();

        for tf in TEMPLATE_FILES {
            let path = tmp.path().join(tf.path);
            assert!(path.is_file(), "template file missing: {}", tf.path);
            let content = fs::read_to_string(&path).unwrap();
            assert_eq!(content, tf.content, "content mismatch: {}", tf.path);
        }
        for sf in SKELETON_FILES {
            let path = tmp.path().join(sf.path);
            assert!(path.is_file(), "skeleton file missing: {}", sf.path);
            let content = fs::read_to_string(&path).unwrap();
            assert_eq!(content, sf.content, "content mismatch: {}", sf.path);
        }
    }

    // --- config.toml scaffolding tests ---

    #[test]
    fn config_toml_scaffolded() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let path = tmp.path().join(".sgf/prompts/config.toml");
        assert!(path.is_file(), "config.toml should be scaffolded");
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, TEMPLATE_CONFIG_TOML);
    }

    #[test]
    fn install_md_scaffolded() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let path = tmp.path().join(".sgf/prompts/install.md");
        assert!(path.is_file(), "install.md should be scaffolded");
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, TEMPLATE_INSTALL);
    }

    #[test]
    fn issues_log_md_scaffolded() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();

        let path = tmp.path().join(".sgf/prompts/issues-log.md");
        assert!(path.is_file(), "issues-log.md should be scaffolded");
        assert!(
            !tmp.path().join(".sgf/prompts/issues.md").exists(),
            "issues.md should NOT exist (renamed to issues-log.md)"
        );
    }

    // --- config.toml --force merge tests ---

    #[test]
    fn force_config_toml_overwrites_shipped_sections() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        git_add_commit(tmp.path(), "init");

        let config_path = tmp.path().join(".sgf/prompts/config.toml");
        fs::write(
            &config_path,
            "[build]\nalias = \"b\"\nmode = \"afk\"\niterations = 999\nauto_push = false\n",
        )
        .unwrap();
        git_add_commit(tmp.path(), "modify config");

        force_init(tmp.path()).unwrap();

        let content = fs::read_to_string(&config_path).unwrap();
        let table: toml::Table = content.parse().unwrap();
        let build = table["build"].as_table().unwrap();
        assert_eq!(build["iterations"].as_integer(), Some(30));
        assert_eq!(build["auto_push"].as_bool(), Some(true));
    }

    #[test]
    fn force_config_toml_preserves_user_sections() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        git_add_commit(tmp.path(), "init");

        let config_path = tmp.path().join(".sgf/prompts/config.toml");
        let mut content = fs::read_to_string(&config_path).unwrap();
        content
            .push_str("\n[my-custom-prompt]\nmode = \"afk\"\niterations = 5\nauto_push = true\n");
        fs::write(&config_path, &content).unwrap();
        git_add_commit(tmp.path(), "add custom section");

        force_init(tmp.path()).unwrap();

        let result = fs::read_to_string(&config_path).unwrap();
        let table: toml::Table = result.parse().unwrap();
        assert!(
            table.contains_key("my-custom-prompt"),
            "user-added section should be preserved"
        );
        let custom = table["my-custom-prompt"].as_table().unwrap();
        assert_eq!(custom["mode"].as_str(), Some("afk"));
        assert_eq!(custom["iterations"].as_integer(), Some(5));
    }

    #[test]
    fn force_config_toml_merge_preserves_all_shipped_sections() {
        let tmp = TempDir::new().unwrap();
        git_init(tmp.path());
        run(tmp.path(), false).unwrap();
        git_add_commit(tmp.path(), "init");

        force_init(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".sgf/prompts/config.toml")).unwrap();
        let table: toml::Table = content.parse().unwrap();
        for section in SHIPPED_CONFIG_SECTIONS {
            assert!(
                table.contains_key(*section),
                "shipped section missing after force: {section}"
            );
        }
    }

    #[test]
    fn merge_config_toml_creates_from_scratch() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

        merge_config_toml(tmp.path()).unwrap();

        let content = fs::read_to_string(tmp.path().join(".sgf/prompts/config.toml")).unwrap();
        assert_eq!(content, TEMPLATE_CONFIG_TOML);
    }
}
