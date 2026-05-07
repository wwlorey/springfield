use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Afk,
    Interactive,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Interactive
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct Transitions {
    pub on_reject: Option<String>,
    pub on_revise: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IterDefinition {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_iterations")]
    pub iterations: u32,
    pub produces: Option<String>,
    #[serde(default)]
    pub consumes: Vec<String>,
    pub auto_push: Option<bool>,
    pub next: Option<String>,
    #[serde(default)]
    pub banner: bool,
    #[serde(default)]
    pub transitions: Transitions,
}

fn default_iterations() -> u32 {
    1
}

fn default_trigger() -> String {
    "manual".to_string()
}

fn default_immediate() -> u32 {
    3
}

fn default_interval_secs() -> u64 {
    300
}

fn default_max_duration_secs() -> u64 {
    43200
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_immediate")]
    pub immediate: u32,
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
    #[serde(default = "default_max_duration_secs")]
    pub max_duration_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            immediate: default_immediate(),
            interval_secs: default_interval_secs(),
            max_duration_secs: default_max_duration_secs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CursusDefinition {
    pub description: String,
    pub alias: Option<String>,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    #[serde(default)]
    pub auto_push: bool,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(rename = "iter")]
    pub iters: Vec<IterDefinition>,
}

impl CursusDefinition {
    pub fn effective_auto_push(&self, iter: &IterDefinition) -> bool {
        iter.auto_push.unwrap_or(self.auto_push)
    }
}

pub fn parse(content: &str) -> Result<CursusDefinition, io::Error> {
    toml::from_str(content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

pub fn parse_file(path: &Path) -> Result<CursusDefinition, io::Error> {
    let content = std::fs::read_to_string(path)?;
    parse(&content).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse cursus definition: {}: {e}", path.display()),
        )
    })
}

pub fn clamp_iterations(def: &mut CursusDefinition) {
    use crate::iter_runner::MAX_ITERATIONS;
    for iter in &mut def.iters {
        if iter.iterations > MAX_ITERATIONS {
            tracing::warn!(
                iter = %iter.name,
                requested = iter.iterations,
                max = MAX_ITERATIONS,
                "clamping iter iterations to hard limit"
            );
            iter.iterations = MAX_ITERATIONS;
        }
    }
}

pub fn validate(def: &CursusDefinition) -> Result<(), io::Error> {
    let mut seen_names = HashSet::new();
    let mut produces_keys = HashSet::new();

    for iter in &def.iters {
        if !seen_names.insert(&iter.name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate iter name: \"{}\"", iter.name),
            ));
        }
        if let Some(ref key) = iter.produces {
            produces_keys.insert(key.clone());
        }
    }

    let valid_names: HashSet<&str> = def.iters.iter().map(|i| i.name.as_str()).collect();

    for iter in &def.iters {
        if let Some(ref target) = iter.next
            && !valid_names.contains(target.as_str())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter \"{}\" has next target \"{}\" which does not exist",
                    iter.name, target
                ),
            ));
        }
        if let Some(ref target) = iter.transitions.on_reject
            && !valid_names.contains(target.as_str())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter \"{}\" has on_reject target \"{}\" which does not exist",
                    iter.name, target
                ),
            ));
        }
        if let Some(ref target) = iter.transitions.on_revise
            && !valid_names.contains(target.as_str())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter \"{}\" has on_revise target \"{}\" which does not exist",
                    iter.name, target
                ),
            ));
        }
        for key in &iter.consumes {
            if !produces_keys.contains(key) {
                tracing::warn!(
                    iter = %iter.name,
                    key = %key,
                    "consumes key not produced by any iter in this cursus"
                );
            }
        }
    }

    Ok(())
}

pub fn validate_prompts(root: &Path, def: &CursusDefinition) -> Result<(), io::Error> {
    let global_prompts = std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".sgf/prompts"));

    for iter in &def.iters {
        let local = root.join(format!(".sgf/prompts/{}", iter.prompt));
        if local.exists() {
            continue;
        }
        if let Some(ref global_dir) = global_prompts {
            let global = global_dir.join(&iter.prompt);
            if global.exists() {
                continue;
            }
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "prompt not found: {} (checked .sgf/prompts/ and ~/.sgf/prompts/)",
                iter.prompt
            ),
        ));
    }

    Ok(())
}

pub fn validate_aliases(definitions: &HashMap<String, CursusDefinition>) -> Result<(), io::Error> {
    let mut seen_aliases: HashMap<&str, &str> = HashMap::new();
    let cursus_names: HashSet<&str> = definitions.keys().map(|k| k.as_str()).collect();

    for (name, def) in definitions {
        if let Some(ref alias) = def.alias {
            if let Some(existing) = seen_aliases.get(alias.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "duplicate alias \"{alias}\": used by both \"{existing}\" and \"{name}\""
                    ),
                ));
            }
            if cursus_names.contains(alias.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("alias \"{alias}\" for \"{name}\" shadows cursus name \"{alias}\""),
                ));
            }
            seen_aliases.insert(alias, name);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_single_iter_cursus() {
        let toml = r#"
description = "Implementation loop"
alias = "b"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.description, "Implementation loop");
        assert_eq!(def.alias.as_deref(), Some("b"));
        assert_eq!(def.trigger, "manual");
        assert!(def.auto_push);
        assert_eq!(def.iters.len(), 1);

        let iter = &def.iters[0];
        assert_eq!(iter.name, "build");
        assert_eq!(iter.prompt, "build.md");
        assert_eq!(iter.mode, Mode::Interactive);
        assert_eq!(iter.iterations, 30);
        assert!(iter.produces.is_none());
        assert!(iter.consumes.is_empty());
        assert!(iter.auto_push.is_none());
        assert!(iter.next.is_none());
    }

    #[test]
    fn parse_multi_iter_with_transitions() {
        let toml = r#"
description = "Spec creation and refinement"
alias = "s"
trigger = "manual"
auto_push = true

[[iter]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
iterations = 1
produces = "discuss-summary"
consumes = []
auto_push = false

[[iter]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
produces = "draft-presentation"
consumes = ["discuss-summary"]
auto_push = true

[[iter]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iter.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iter]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iter]]
name = "approve"
prompt = "spec-approve.md"
mode = "interactive"
consumes = ["draft-presentation"]
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.iters.len(), 5);

        let review = &def.iters[2];
        assert_eq!(review.transitions.on_reject, Some("draft".to_string()));
        assert_eq!(review.transitions.on_revise, Some("revise".to_string()));

        let revise = &def.iters[3];
        assert_eq!(revise.next, Some("review".to_string()));
        assert_eq!(revise.produces, Some("draft-presentation".to_string()));
        assert_eq!(
            revise.consumes,
            vec!["discuss-summary", "draft-presentation"]
        );
    }

    #[test]
    fn parse_produces_and_consumes() {
        let toml = r#"
description = "Pipeline with context passing"

[[iter]]
name = "generate"
prompt = "gen.md"
produces = "output-summary"

[[iter]]
name = "verify"
prompt = "verify.md"
consumes = ["output-summary"]
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.iters[0].produces, Some("output-summary".to_string()));
        assert_eq!(def.iters[1].consumes, vec!["output-summary"]);
    }

    #[test]
    fn defaults_applied() {
        let toml = r#"
description = "Minimal"

[[iter]]
name = "run"
prompt = "run.md"
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.trigger, "manual");
        assert!(!def.auto_push);
        assert!(def.alias.is_none());

        let iter = &def.iters[0];
        assert_eq!(iter.mode, Mode::Interactive);
        assert_eq!(iter.iterations, 1);
        assert!(iter.consumes.is_empty());
        assert!(iter.produces.is_none());
        assert!(iter.auto_push.is_none());
        assert!(iter.next.is_none());
        assert!(iter.transitions.on_reject.is_none());
        assert!(iter.transitions.on_revise.is_none());
    }

    #[test]
    fn parse_banner_true() {
        let toml = r#"
description = "Banner test"

[[iter]]
name = "run"
prompt = "run.md"
banner = true
"#;
        let def = parse(toml).unwrap();
        assert!(def.iters[0].banner);
    }

    #[test]
    fn parse_banner_false() {
        let toml = r#"
description = "Banner test"

[[iter]]
name = "run"
prompt = "run.md"
banner = false
"#;
        let def = parse(toml).unwrap();
        assert!(!def.iters[0].banner);
    }

    #[test]
    fn parse_banner_defaults_to_false() {
        let toml = r#"
description = "Banner test"

[[iter]]
name = "run"
prompt = "run.md"
"#;
        let def = parse(toml).unwrap();
        assert!(!def.iters[0].banner);
    }

    #[test]
    fn reject_duplicate_iter_names() {
        let toml = r#"
description = "Duplicate names"

[[iter]]
name = "build"
prompt = "build.md"

[[iter]]
name = "build"
prompt = "build2.md"
"#;
        let def = parse(toml).unwrap();
        let err = validate(&def).unwrap_err();
        assert!(err.to_string().contains("duplicate iter name"));
    }

    #[test]
    fn reject_invalid_transition_target() {
        let toml = r#"
description = "Bad transition"

[[iter]]
name = "review"
prompt = "review.md"

  [iter.transitions]
  on_reject = "nonexistent"
"#;
        let def = parse(toml).unwrap();
        let err = validate(&def).unwrap_err();
        assert!(err.to_string().contains("on_reject target \"nonexistent\""));
    }

    #[test]
    fn reject_invalid_next_target() {
        let toml = r#"
description = "Bad next"

[[iter]]
name = "step"
prompt = "step.md"
next = "ghost"
"#;
        let def = parse(toml).unwrap();
        let err = validate(&def).unwrap_err();
        assert!(err.to_string().contains("next target \"ghost\""));
    }

    #[test]
    fn reject_invalid_on_revise_target() {
        let toml = r#"
description = "Bad revise"

[[iter]]
name = "review"
prompt = "review.md"

  [iter.transitions]
  on_revise = "missing"
"#;
        let def = parse(toml).unwrap();
        let err = validate(&def).unwrap_err();
        assert!(err.to_string().contains("on_revise target \"missing\""));
    }

    #[test]
    fn warn_interactive_iter_with_iterations_gt_1() {
        let toml = r#"
description = "Interactive warning"

[[iter]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 5
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.iters[0].mode, Mode::Interactive);
        assert_eq!(def.iters[0].iterations, 5);
        // validate() should succeed (warning, not error)
        assert!(validate(&def).is_ok());
    }

    #[test]
    fn no_warn_interactive_iter_with_iterations_1() {
        let toml = r#"
description = "Interactive no warning"

[[iter]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 1
"#;
        let def = parse(toml).unwrap();
        assert!(validate(&def).is_ok());
    }

    #[test]
    fn no_warn_afk_iter_with_iterations_gt_1() {
        let toml = r#"
description = "Afk no warning"

[[iter]]
name = "build"
prompt = "build.md"
mode = "afk"
iterations = 10
"#;
        let def = parse(toml).unwrap();
        assert!(validate(&def).is_ok());
    }

    #[test]
    fn validate_passes_for_valid_def() {
        let toml = r#"
description = "Valid pipeline"

[[iter]]
name = "draft"
prompt = "draft.md"
produces = "summary"

[[iter]]
name = "review"
prompt = "review.md"
consumes = ["summary"]

  [iter.transitions]
  on_reject = "draft"
"#;
        let def = parse(toml).unwrap();
        assert!(validate(&def).is_ok());
    }

    #[test]
    fn effective_auto_push_inherits_from_cursus() {
        let toml = r#"
description = "Auto push test"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let def = parse(toml).unwrap();
        assert!(def.effective_auto_push(&def.iters[0]));
    }

    #[test]
    fn effective_auto_push_iter_overrides_cursus() {
        let toml = r#"
description = "Auto push override"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
auto_push = false
"#;
        let def = parse(toml).unwrap();
        assert!(!def.effective_auto_push(&def.iters[0]));
    }

    fn make_def(desc: &str, alias: Option<&str>, iter_name: &str) -> CursusDefinition {
        CursusDefinition {
            description: desc.to_string(),
            alias: alias.map(|a| a.to_string()),
            trigger: "manual".to_string(),
            auto_push: false,
            retry: RetryConfig::default(),
            iters: vec![IterDefinition {
                name: iter_name.to_string(),
                prompt: format!("{iter_name}.md"),
                mode: Mode::default(),
                iterations: 1,
                produces: None,
                consumes: vec![],
                auto_push: None,
                next: None,
                banner: false,
                transitions: Transitions::default(),
            }],
        }
    }

    #[test]
    fn validate_aliases_rejects_duplicates() {
        let mut defs = HashMap::new();
        defs.insert("build".to_string(), make_def("Build", Some("x"), "b"));
        defs.insert("test".to_string(), make_def("Test", Some("x"), "t"));
        let err = validate_aliases(&defs).unwrap_err();
        assert!(err.to_string().contains("duplicate alias \"x\""));
    }

    #[test]
    fn validate_aliases_rejects_shadow() {
        let mut defs = HashMap::new();
        defs.insert("build".to_string(), make_def("Build", Some("test"), "b"));
        defs.insert("test".to_string(), make_def("Test", None, "t"));
        let err = validate_aliases(&defs).unwrap_err();
        assert!(err.to_string().contains("shadows cursus name"));
    }

    #[test]
    fn validate_aliases_ok() {
        let mut defs = HashMap::new();
        defs.insert("build".to_string(), make_def("Build", Some("b"), "b"));
        defs.insert("test".to_string(), make_def("Test", Some("t"), "t"));
        assert!(validate_aliases(&defs).is_ok());
    }

    #[test]
    fn clamp_iterations_above_max() {
        let mut def = make_def("Test", None, "build");
        def.iters[0].iterations = 2000;
        clamp_iterations(&mut def);
        assert_eq!(def.iters[0].iterations, 1000);
    }

    #[test]
    fn clamp_iterations_at_max_unchanged() {
        let mut def = make_def("Test", None, "build");
        def.iters[0].iterations = 1000;
        clamp_iterations(&mut def);
        assert_eq!(def.iters[0].iterations, 1000);
    }

    #[test]
    fn clamp_iterations_below_max_unchanged() {
        let mut def = make_def("Test", None, "build");
        def.iters[0].iterations = 500;
        clamp_iterations(&mut def);
        assert_eq!(def.iters[0].iterations, 500);
    }

    #[test]
    fn clamp_iterations_multiple_iters() {
        let mut def = CursusDefinition {
            description: "Multi".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push: false,
            retry: RetryConfig::default(),
            iters: vec![
                IterDefinition {
                    name: "a".to_string(),
                    prompt: "a.md".to_string(),
                    mode: Mode::default(),
                    iterations: 1500,
                    produces: None,
                    consumes: vec![],
                    auto_push: None,
                    next: None,
                    banner: false,
                    transitions: Transitions::default(),
                },
                IterDefinition {
                    name: "b".to_string(),
                    prompt: "b.md".to_string(),
                    mode: Mode::default(),
                    iterations: 30,
                    produces: None,
                    consumes: vec![],
                    auto_push: None,
                    next: None,
                    banner: false,
                    transitions: Transitions::default(),
                },
            ],
        };
        clamp_iterations(&mut def);
        assert_eq!(def.iters[0].iterations, 1000);
        assert_eq!(def.iters[1].iterations, 30);
    }

    #[test]
    fn parse_invalid_toml() {
        let err = parse("not valid {{{{").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_invalid_mode() {
        let toml = r#"
description = "Bad mode"

[[iter]]
name = "run"
prompt = "run.md"
mode = "turbo"
"#;
        let err = parse(toml).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_file_nonexistent() {
        let err = parse_file(Path::new("/nonexistent/path.toml")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn validate_prompts_found_locally() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        std::fs::write(tmp.path().join(".sgf/prompts/build.md"), "prompt").unwrap();

        let def = CursusDefinition {
            description: "Test".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push: false,
            retry: RetryConfig::default(),
            iters: vec![IterDefinition {
                name: "build".to_string(),
                prompt: "build.md".to_string(),
                mode: Mode::default(),
                iterations: 1,
                produces: None,
                consumes: vec![],
                auto_push: None,
                next: None,
                banner: false,
                transitions: Transitions::default(),
            }],
        };

        assert!(validate_prompts(tmp.path(), &def).is_ok());
    }

    #[test]
    fn validate_prompts_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

        let def = CursusDefinition {
            description: "Test".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push: false,
            retry: RetryConfig::default(),
            iters: vec![IterDefinition {
                name: "build".to_string(),
                prompt: "missing.md".to_string(),
                mode: Mode::default(),
                iterations: 1,
                produces: None,
                consumes: vec![],
                auto_push: None,
                next: None,
                banner: false,
                transitions: Transitions::default(),
            }],
        };

        let err = validate_prompts(tmp.path(), &def).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("prompt not found: missing.md"));
    }

    #[test]
    fn validate_prompts_partial_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        std::fs::write(tmp.path().join(".sgf/prompts/build.md"), "prompt").unwrap();

        let def = CursusDefinition {
            description: "Test".to_string(),
            alias: None,
            trigger: "manual".to_string(),
            auto_push: false,
            retry: RetryConfig::default(),
            iters: vec![
                IterDefinition {
                    name: "build".to_string(),
                    prompt: "build.md".to_string(),
                    mode: Mode::default(),
                    iterations: 1,
                    produces: None,
                    consumes: vec![],
                    auto_push: None,
                    next: None,
                    banner: false,
                    transitions: Transitions::default(),
                },
                IterDefinition {
                    name: "review".to_string(),
                    prompt: "review.md".to_string(),
                    mode: Mode::default(),
                    iterations: 1,
                    produces: None,
                    consumes: vec![],
                    auto_push: None,
                    next: None,
                    banner: false,
                    transitions: Transitions::default(),
                },
            ],
        };

        let err = validate_prompts(tmp.path(), &def).unwrap_err();
        assert!(err.to_string().contains("prompt not found: review.md"));
    }

    #[test]
    fn parse_file_from_tempdir() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");
        std::fs::write(
            &path,
            r#"
description = "Test cursus"

[[iter]]
name = "build"
prompt = "build.md"
mode = "afk"
"#,
        )
        .unwrap();

        let def = parse_file(&path).unwrap();
        validate(&def).unwrap();
        assert_eq!(def.description, "Test cursus");
        assert_eq!(def.iters.len(), 1);
    }

    #[test]
    fn all_global_cursus_tomls_parse_and_validate() {
        let home = std::env::var("HOME").expect("HOME not set");
        let cursus_dir = PathBuf::from(home).join(".sgf/cursus");
        if !cursus_dir.exists() {
            return;
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&cursus_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let def = parse_file(&path).unwrap_or_else(|e| {
                    panic!("failed to parse {}: {e}", path.display());
                });
                validate(&def).unwrap_or_else(|e| {
                    panic!("validation failed for {}: {e}", path.display());
                });
                count += 1;
            }
        }
        assert!(count > 0, "no TOML files found in {}", cursus_dir.display());
    }

    #[test]
    fn parse_retry_all_fields() {
        let toml = r#"
description = "Retry test"

[retry]
immediate = 5
interval_secs = 600
max_duration_secs = 86400

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.retry.immediate, 5);
        assert_eq!(def.retry.interval_secs, 600);
        assert_eq!(def.retry.max_duration_secs, 86400);
    }

    #[test]
    fn parse_retry_partial_fields() {
        let toml = r#"
description = "Retry partial"

[retry]
immediate = 10

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.retry.immediate, 10);
        assert_eq!(def.retry.interval_secs, 300);
        assert_eq!(def.retry.max_duration_secs, 43200);
    }

    #[test]
    fn parse_retry_defaults_when_omitted() {
        let toml = r#"
description = "No retry section"

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.retry.immediate, 3);
        assert_eq!(def.retry.interval_secs, 300);
        assert_eq!(def.retry.max_duration_secs, 43200);
    }

    #[test]
    fn parse_retry_defaults_when_empty_table() {
        let toml = r#"
description = "Empty retry"

[retry]

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let def = parse(toml).unwrap();
        assert_eq!(def.retry.immediate, 3);
        assert_eq!(def.retry.interval_secs, 300);
        assert_eq!(def.retry.max_duration_secs, 43200);
    }

    #[test]
    fn parse_retry_invalid_type_rejected() {
        let toml = r#"
description = "Bad retry"

[retry]
immediate = "not_a_number"

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let err = parse(toml).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_retry_invalid_interval_type_rejected() {
        let toml = r#"
description = "Bad retry interval"

[retry]
interval_secs = true

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let err = parse(toml).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_retry_invalid_max_duration_type_rejected() {
        let toml = r#"
description = "Bad retry max duration"

[retry]
max_duration_secs = [1, 2, 3]

[[iter]]
name = "build"
prompt = "build.md"
"#;
        let err = parse(toml).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
