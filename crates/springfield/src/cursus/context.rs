use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cursus::state::context_dir;
use crate::cursus::toml::CursusDefinition;

pub fn context_file_path(root: &Path, run_id: &str, key: &str) -> PathBuf {
    context_dir(root, run_id).join(format!("{key}.md"))
}

pub fn context_env_var(root: &Path, run_id: &str) -> (String, String) {
    let dir = context_dir(root, run_id);
    (
        "SGF_RUN_CONTEXT".to_string(),
        dir.to_string_lossy().into_owned(),
    )
}

pub fn check_produces(root: &Path, run_id: &str, key: &str) -> bool {
    let path = context_file_path(root, run_id, key);
    if path.exists() {
        true
    } else {
        tracing::warn!(
            run_id = %run_id,
            key = %key,
            "produces file not written by agent: {}",
            path.display()
        );
        false
    }
}

fn build_key_to_iter_map(def: &CursusDefinition) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    for iter in &def.iters {
        if let Some(ref key) = iter.produces {
            map.insert(key.as_str(), iter.name.as_str());
        }
    }
    map
}

pub fn resolve_consumes(
    root: &Path,
    run_id: &str,
    consumes: &[String],
    def: &CursusDefinition,
) -> String {
    if consumes.is_empty() {
        return String::new();
    }

    let key_to_iter = build_key_to_iter_map(def);
    let mut parts = Vec::new();

    for key in consumes {
        let path = context_file_path(root, run_id, key);
        let iter_name = key_to_iter.get(key.as_str()).copied().unwrap_or("unknown");

        match fs::read_to_string(&path) {
            Ok(content) => {
                parts.push(format!(
                    "=== Context from iter: {iter_name} ({key}) ===\n\n{content}"
                ));
            }
            Err(_) => {
                tracing::warn!(
                    run_id = %run_id,
                    key = %key,
                    "consumed context file missing: {}",
                    path.display()
                );
            }
        }
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursus::state::create_run_dir;
    use crate::cursus::toml::{IterDefinition, Mode, Transitions};
    use tempfile::TempDir;

    fn make_def_with_produces(iters: Vec<(&str, Option<&str>, Vec<&str>)>) -> CursusDefinition {
        CursusDefinition {
            description: "test".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push: false,
            iters: iters
                .into_iter()
                .map(|(name, produces, consumes)| IterDefinition {
                    name: name.to_string(),
                    prompt: format!("{name}.md"),
                    mode: Mode::default(),
                    iterations: 1,
                    produces: produces.map(|p| p.to_string()),
                    consumes: consumes.into_iter().map(|c| c.to_string()).collect(),
                    auto_push: None,
                    next: None,
                    transitions: Transitions::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn produces_file_path_correct() {
        let tmp = TempDir::new().unwrap();
        let path = context_file_path(tmp.path(), "spec-20260317T140000", "discuss-summary");
        assert!(path.ends_with(".sgf/run/spec-20260317T140000/context/discuss-summary.md"));
    }

    #[test]
    fn context_env_var_set() {
        let tmp = TempDir::new().unwrap();
        let (name, value) = context_env_var(tmp.path(), "spec-20260317T140000");
        assert_eq!(name, "SGF_RUN_CONTEXT");
        assert!(value.ends_with(".sgf/run/spec-20260317T140000/context"));
    }

    #[test]
    fn check_produces_exists() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();
        let path = context_file_path(tmp.path(), run_id, "summary");
        fs::write(&path, "some content").unwrap();

        assert!(check_produces(tmp.path(), run_id, "summary"));
    }

    #[test]
    fn check_produces_missing_returns_false() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        assert!(!check_produces(tmp.path(), run_id, "nonexistent"));
    }

    #[test]
    fn resolve_consumes_single_file() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        let path = context_file_path(tmp.path(), run_id, "discuss-summary");
        fs::write(&path, "Discussion notes here.").unwrap();

        let def = make_def_with_produces(vec![
            ("discuss", Some("discuss-summary"), vec![]),
            ("draft", None, vec!["discuss-summary"]),
        ]);

        let result = resolve_consumes(tmp.path(), run_id, &["discuss-summary".to_string()], &def);

        assert!(result.contains("=== Context from iter: discuss (discuss-summary) ==="));
        assert!(result.contains("Discussion notes here."));
    }

    #[test]
    fn resolve_consumes_multiple_files_concatenated_in_order() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        fs::write(
            context_file_path(tmp.path(), run_id, "discuss-summary"),
            "Discussion content.",
        )
        .unwrap();
        fs::write(
            context_file_path(tmp.path(), run_id, "draft-presentation"),
            "Draft content.",
        )
        .unwrap();

        let def = make_def_with_produces(vec![
            ("discuss", Some("discuss-summary"), vec![]),
            ("draft", Some("draft-presentation"), vec!["discuss-summary"]),
            (
                "review",
                None,
                vec!["discuss-summary", "draft-presentation"],
            ),
        ]);

        let consumes = vec![
            "discuss-summary".to_string(),
            "draft-presentation".to_string(),
        ];
        let result = resolve_consumes(tmp.path(), run_id, &consumes, &def);

        let discuss_pos = result
            .find("Context from iter: discuss")
            .expect("discuss header");
        let draft_pos = result
            .find("Context from iter: draft")
            .expect("draft header");
        assert!(discuss_pos < draft_pos);
        assert!(result.contains("Discussion content."));
        assert!(result.contains("Draft content."));
    }

    #[test]
    fn resolve_consumes_missing_file_returns_empty_for_that_key() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        fs::write(
            context_file_path(tmp.path(), run_id, "discuss-summary"),
            "Discussion content.",
        )
        .unwrap();

        let def = make_def_with_produces(vec![
            ("discuss", Some("discuss-summary"), vec![]),
            ("draft", Some("missing-key"), vec!["discuss-summary"]),
            ("review", None, vec!["discuss-summary", "missing-key"]),
        ]);

        let consumes = vec!["discuss-summary".to_string(), "missing-key".to_string()];
        let result = resolve_consumes(tmp.path(), run_id, &consumes, &def);

        assert!(result.contains("Discussion content."));
        assert!(!result.contains("missing-key"));
    }

    #[test]
    fn resolve_consumes_empty_list_returns_empty_string() {
        let tmp = TempDir::new().unwrap();
        let def = make_def_with_produces(vec![("build", None, vec![])]);
        let result = resolve_consumes(tmp.path(), "any-run", &[], &def);
        assert!(result.is_empty());
    }

    #[test]
    fn produces_overwrite_later_wins() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        let path = context_file_path(tmp.path(), run_id, "draft-presentation");
        fs::write(&path, "First draft.").unwrap();
        fs::write(&path, "Revised draft.").unwrap();

        let def = make_def_with_produces(vec![
            ("draft", Some("draft-presentation"), vec![]),
            ("revise", Some("draft-presentation"), vec![]),
            ("approve", None, vec!["draft-presentation"]),
        ]);

        let result = resolve_consumes(
            tmp.path(),
            run_id,
            &["draft-presentation".to_string()],
            &def,
        );

        assert!(result.contains("Revised draft."));
        assert!(!result.contains("First draft."));
    }

    #[test]
    fn resolve_consumes_unknown_producer_uses_unknown_iter_name() {
        let tmp = TempDir::new().unwrap();
        let run_id = "test-20260317T140000";
        create_run_dir(tmp.path(), run_id).unwrap();

        fs::write(
            context_file_path(tmp.path(), run_id, "orphan-key"),
            "Orphan content.",
        )
        .unwrap();

        let def = make_def_with_produces(vec![("build", None, vec![])]);

        let result = resolve_consumes(tmp.path(), run_id, &["orphan-key".to_string()], &def);

        assert!(result.contains("Context from iter: unknown (orphan-key)"));
        assert!(result.contains("Orphan content."));
    }
}
