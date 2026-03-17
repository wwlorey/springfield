use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

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
    pub transitions: Transitions,
}

fn default_iterations() -> u32 {
    1
}

fn default_trigger() -> String {
    "manual".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CursusDefinition {
    pub description: String,
    pub alias: Option<String>,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    #[serde(default)]
    pub auto_push: bool,
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

    #[test]
    fn parse_single_iter_cursus() {
        let toml = r#"
description = "Implementation loop"
alias = "b"
auto_push = true

[[iters]]
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

[[iters]]
name = "discuss"
prompt = "spec-discuss.md"
mode = "interactive"
iterations = 1
produces = "discuss-summary"
consumes = []
auto_push = false

[[iters]]
name = "draft"
prompt = "spec-draft.md"
mode = "afk"
iterations = 10
produces = "draft-presentation"
consumes = ["discuss-summary"]
auto_push = true

[[iters]]
name = "review"
prompt = "spec-review.md"
mode = "interactive"
consumes = ["discuss-summary", "draft-presentation"]

  [iters.transitions]
  on_reject = "draft"
  on_revise = "revise"

[[iters]]
name = "revise"
prompt = "spec-revise.md"
mode = "afk"
iterations = 5
consumes = ["discuss-summary", "draft-presentation"]
produces = "draft-presentation"
next = "review"

[[iters]]
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

[[iters]]
name = "generate"
prompt = "gen.md"
produces = "output-summary"

[[iters]]
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

[[iters]]
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
    fn reject_duplicate_iter_names() {
        let toml = r#"
description = "Duplicate names"

[[iters]]
name = "build"
prompt = "build.md"

[[iters]]
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

[[iters]]
name = "review"
prompt = "review.md"

  [iters.transitions]
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

[[iters]]
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

[[iters]]
name = "review"
prompt = "review.md"

  [iters.transitions]
  on_revise = "missing"
"#;
        let def = parse(toml).unwrap();
        let err = validate(&def).unwrap_err();
        assert!(err.to_string().contains("on_revise target \"missing\""));
    }

    #[test]
    fn validate_passes_for_valid_def() {
        let toml = r#"
description = "Valid pipeline"

[[iters]]
name = "draft"
prompt = "draft.md"
produces = "summary"

[[iters]]
name = "review"
prompt = "review.md"
consumes = ["summary"]

  [iters.transitions]
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

[[iters]]
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

[[iters]]
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
            iters: vec![IterDefinition {
                name: iter_name.to_string(),
                prompt: format!("{iter_name}.md"),
                mode: Mode::default(),
                iterations: 1,
                produces: None,
                consumes: vec![],
                auto_push: None,
                next: None,
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
    fn parse_invalid_toml() {
        let err = parse("not valid {{{{").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parse_invalid_mode() {
        let toml = r#"
description = "Bad mode"

[[iters]]
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
}
