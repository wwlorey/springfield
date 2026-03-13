use serde::Deserialize;
use std::collections::HashMap;
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommandConfig {
    pub alias: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_iterations")]
    pub iterations: u32,
    #[serde(default)]
    pub auto_push: bool,
}

fn default_iterations() -> u32 {
    1
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            alias: None,
            mode: Mode::default(),
            iterations: 1,
            auto_push: false,
        }
    }
}

pub type PromptConfigs = HashMap<String, CommandConfig>;

pub fn parse(content: &str) -> Result<PromptConfigs, io::Error> {
    let configs: PromptConfigs = toml::from_str(content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(configs)
}

pub fn validate(configs: &PromptConfigs, prompt_names: &[String]) -> Result<(), io::Error> {
    let mut seen_aliases: HashMap<&str, &str> = HashMap::new();

    for (name, config) in configs {
        if let Some(ref alias) = config.alias {
            if let Some(existing) = seen_aliases.get(alias.as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "duplicate alias \"{alias}\": used by both \"{existing}\" and \"{name}\""
                    ),
                ));
            }

            if prompt_names.iter().any(|p| p == alias) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("alias \"{alias}\" for \"{name}\" shadows prompt file \"{alias}.md\""),
                ));
            }

            seen_aliases.insert(alias, name);
        }
    }

    Ok(())
}

pub fn resolve_alias<'a>(configs: &'a PromptConfigs, name: &str) -> Option<&'a str> {
    configs.iter().find_map(|(cmd, cfg)| {
        cfg.alias
            .as_deref()
            .filter(|a| *a == name)
            .map(|_| cmd.as_str())
    })
}

pub fn load(root: &Path) -> Result<PromptConfigs, io::Error> {
    let config_path = root.join(".sgf/prompts/config.toml");
    if !config_path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(&config_path)?;
    let configs = parse(&content)?;

    let prompts_dir = root.join(".sgf/prompts");
    let prompt_names: Vec<String> = std::fs::read_dir(&prompts_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    validate(&configs, &prompt_names)?;
    Ok(configs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_valid_config() {
        let toml = r#"
[build]
alias = "b"
mode = "afk"
iterations = 30
auto_push = true

[spec]
alias = "s"
mode = "interactive"
iterations = 1
auto_push = false
"#;
        let configs = parse(toml).unwrap();
        assert_eq!(configs.len(), 2);

        let build = &configs["build"];
        assert_eq!(build.alias.as_deref(), Some("b"));
        assert_eq!(build.mode, Mode::Afk);
        assert_eq!(build.iterations, 30);
        assert!(build.auto_push);

        let spec = &configs["spec"];
        assert_eq!(spec.alias.as_deref(), Some("s"));
        assert_eq!(spec.mode, Mode::Interactive);
        assert_eq!(spec.iterations, 1);
        assert!(!spec.auto_push);
    }

    #[test]
    fn fallback_defaults() {
        let toml = r#"
[doc]
"#;
        let configs = parse(toml).unwrap();
        let doc = &configs["doc"];
        assert_eq!(doc.alias, None);
        assert_eq!(doc.mode, Mode::Interactive);
        assert_eq!(doc.iterations, 1);
        assert!(!doc.auto_push);
    }

    #[test]
    fn partial_fields_use_defaults() {
        let toml = r#"
[verify]
mode = "afk"
"#;
        let configs = parse(toml).unwrap();
        let verify = &configs["verify"];
        assert_eq!(verify.alias, None);
        assert_eq!(verify.mode, Mode::Afk);
        assert_eq!(verify.iterations, 1);
        assert!(!verify.auto_push);
    }

    #[test]
    fn alias_resolution() {
        let toml = r#"
[build]
alias = "b"
mode = "afk"

[install]
alias = "i"
"#;
        let configs = parse(toml).unwrap();
        assert_eq!(resolve_alias(&configs, "b"), Some("build"));
        assert_eq!(resolve_alias(&configs, "i"), Some("install"));
        assert_eq!(resolve_alias(&configs, "x"), None);
        assert_eq!(resolve_alias(&configs, "build"), None);
    }

    #[test]
    fn duplicate_alias_error() {
        let toml = r#"
[build]
alias = "x"

[install]
alias = "x"
"#;
        let configs = parse(toml).unwrap();
        let prompt_names: Vec<String> = vec![];
        let err = validate(&configs, &prompt_names).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("duplicate alias \"x\""));
    }

    #[test]
    fn alias_shadows_prompt_name_error() {
        let toml = r#"
[install]
alias = "build"
"#;
        let configs = parse(toml).unwrap();
        let prompt_names = vec!["build".to_string(), "install".to_string()];
        let err = validate(&configs, &prompt_names).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("shadows prompt file"));
    }

    #[test]
    fn validate_ok_with_no_conflicts() {
        let toml = r#"
[build]
alias = "b"

[install]
alias = "i"
"#;
        let configs = parse(toml).unwrap();
        let prompt_names = vec!["build".to_string(), "install".to_string()];
        assert!(validate(&configs, &prompt_names).is_ok());
    }

    #[test]
    fn load_missing_config_returns_empty() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        let configs = load(tmp.path()).unwrap();
        assert!(configs.is_empty());
    }

    #[test]
    fn load_valid_config_from_disk() {
        let tmp = TempDir::new().unwrap();
        let prompts_dir = tmp.path().join(".sgf/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("build.md"), "prompt").unwrap();
        fs::write(
            prompts_dir.join("config.toml"),
            r#"
[build]
alias = "b"
mode = "afk"
iterations = 10
auto_push = true
"#,
        )
        .unwrap();

        let configs = load(tmp.path()).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs["build"].alias.as_deref(), Some("b"));
    }

    #[test]
    fn load_rejects_shadowing_alias() {
        let tmp = TempDir::new().unwrap();
        let prompts_dir = tmp.path().join(".sgf/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("build.md"), "prompt").unwrap();
        fs::write(prompts_dir.join("install.md"), "prompt").unwrap();
        fs::write(
            prompts_dir.join("config.toml"),
            r#"
[install]
alias = "build"
"#,
        )
        .unwrap();

        let err = load(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("shadows prompt file"));
    }

    #[test]
    fn invalid_toml_returns_error() {
        let err = parse("not valid toml {{{{").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn invalid_mode_returns_error() {
        let toml = r#"
[build]
mode = "turbo"
"#;
        let err = parse(toml).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn default_command_config() {
        let cfg = CommandConfig::default();
        assert_eq!(cfg.alias, None);
        assert_eq!(cfg.mode, Mode::Interactive);
        assert_eq!(cfg.iterations, 1);
        assert!(!cfg.auto_push);
    }
}
