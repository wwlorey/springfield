use std::ffi::OsString;
use std::path::Path;

use clap::{Parser, Subcommand};

use springfield::config::{self, Mode};

/// CLI entry point for Springfield — scaffolding, prompt delivery, loop orchestration.
#[derive(Parser)]
#[command(name = "sgf")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new project
    Init {
        #[arg(long)]
        force: bool,
    },

    /// Show project state (future work)
    Status,

    /// Tail a running loop's output
    Logs {
        /// Loop ID to tail
        loop_id: String,
    },

    #[command(external_subcommand)]
    Dynamic(Vec<OsString>),
}

#[derive(Debug)]
struct DynamicArgs {
    command: String,
    spec: Option<String>,
    afk: bool,
    interactive: bool,
    iterations: Option<u32>,
    no_push: bool,
}

fn parse_dynamic_args(args: Vec<OsString>) -> Result<DynamicArgs, String> {
    if args.is_empty() {
        return Err("no command specified".to_string());
    }

    let strs: Vec<String> = args
        .into_iter()
        .map(|a| {
            a.into_string()
                .map_err(|_| "invalid UTF-8 in argument".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let command = strs[0].clone();
    let rest = &strs[1..];

    let mut spec = None;
    let mut afk = false;
    let mut interactive = false;
    let mut iterations = None;
    let mut no_push = false;

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-a" | "--afk" => afk = true,
            "-i" | "--interactive" => interactive = true,
            "--no-push" => no_push = true,
            "-n" | "--iterations" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--iterations requires a value".to_string());
                }
                iterations = Some(
                    rest[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid iteration count: {}", rest[i]))?,
                );
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag: {arg}"));
            }
            positional => {
                if spec.is_some() {
                    return Err(format!("unexpected argument: {positional}"));
                }
                spec = Some(positional.to_string());
            }
        }
        i += 1;
    }

    if afk && interactive {
        return Err("-a/--afk and -i/--interactive are mutually exclusive".to_string());
    }

    Ok(DynamicArgs {
        command,
        spec,
        afk,
        interactive,
        iterations,
        no_push,
    })
}

fn resolve_command(root: &Path, name: &str) -> Result<String, String> {
    let prompt_path = root.join(format!(".sgf/prompts/{name}.md"));
    if prompt_path.exists() {
        return Ok(name.to_string());
    }

    let configs = config::load(root).map_err(|e| format!("failed to load config: {e}"))?;
    if let Some(resolved) = config::resolve_alias(&configs, name) {
        let resolved_path = root.join(format!(".sgf/prompts/{resolved}.md"));
        if resolved_path.exists() {
            return Ok(resolved.to_string());
        }
    }

    Err(format!("unknown command: {name}"))
}

fn run_dynamic(args: DynamicArgs) -> ! {
    let root = std::env::current_dir().expect("failed to get current directory");

    let resolved = match resolve_command(&root, &args.command) {
        Ok(r) => r,
        Err(e) => {
            springfield::style::print_error(&e);
            std::process::exit(1);
        }
    };

    let configs = config::load(&root).unwrap_or_default();
    let cmd_config = configs.get(&resolved).cloned().unwrap_or_default();

    let interactive = if args.afk {
        false
    } else if args.interactive {
        true
    } else {
        cmd_config.mode == Mode::Interactive
    };

    let afk = if args.afk {
        true
    } else if args.interactive {
        false
    } else {
        cmd_config.mode == Mode::Afk
    };

    let iterations = args.iterations.unwrap_or(cmd_config.iterations);
    let no_push = args.no_push || !cmd_config.auto_push;

    let config = springfield::orchestrate::LoopConfig {
        stage: resolved.clone(),
        spec: args.spec,
        afk,
        no_push,
        iterations,
        interactive,
        ralph_binary: None,
        skip_preflight: false,
        prompt_template: None,
    };

    match springfield::orchestrate::run(&root, &config) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            springfield::style::print_error(&format!("{resolved}: {e}"));
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { force } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::init::run(&root, force) {
                springfield::style::print_error(&format!("init: {e}"));
                std::process::exit(1);
            }
        }
        Commands::Status => {
            springfield::style::print_warning("not yet implemented");
        }
        Commands::Logs { loop_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::loop_mgmt::run_logs(&root, &loop_id) {
                springfield::style::print_error(&format!("logs: {e}"));
                std::process::exit(1);
            }
        }
        Commands::Dynamic(args) => {
            let parsed = match parse_dynamic_args(args) {
                Ok(a) => a,
                Err(e) => {
                    springfield::style::print_error(&e);
                    std::process::exit(1);
                }
            };
            run_dynamic(parsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn parse_command_only() {
        let args = vec![os("build")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert!(parsed.spec.is_none());
        assert!(!parsed.afk);
        assert!(!parsed.interactive);
        assert!(parsed.iterations.is_none());
        assert!(!parsed.no_push);
    }

    #[test]
    fn parse_command_with_spec() {
        let args = vec![os("build"), os("auth")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert_eq!(parsed.spec.as_deref(), Some("auth"));
    }

    #[test]
    fn parse_afk_flag_short() {
        let args = vec![os("build"), os("-a")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.afk);
        assert!(!parsed.interactive);
    }

    #[test]
    fn parse_afk_flag_long() {
        let args = vec![os("build"), os("--afk")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.afk);
    }

    #[test]
    fn parse_interactive_flag_short() {
        let args = vec![os("build"), os("-i")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.interactive);
        assert!(!parsed.afk);
    }

    #[test]
    fn parse_interactive_flag_long() {
        let args = vec![os("build"), os("--interactive")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.interactive);
    }

    #[test]
    fn parse_iterations_short() {
        let args = vec![os("build"), os("-n"), os("10")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(10));
    }

    #[test]
    fn parse_iterations_long() {
        let args = vec![os("build"), os("--iterations"), os("50")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.iterations, Some(50));
    }

    #[test]
    fn parse_no_push() {
        let args = vec![os("build"), os("--no-push")];
        let parsed = parse_dynamic_args(args).unwrap();
        assert!(parsed.no_push);
    }

    #[test]
    fn parse_all_flags_with_spec() {
        let args = vec![
            os("build"),
            os("auth"),
            os("-a"),
            os("-n"),
            os("30"),
            os("--no-push"),
        ];
        let parsed = parse_dynamic_args(args).unwrap();
        assert_eq!(parsed.command, "build");
        assert_eq!(parsed.spec.as_deref(), Some("auth"));
        assert!(parsed.afk);
        assert_eq!(parsed.iterations, Some(30));
        assert!(parsed.no_push);
    }

    #[test]
    fn parse_mutual_exclusion_error() {
        let args = vec![os("build"), os("-a"), os("-i")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn parse_empty_args_error() {
        let err = parse_dynamic_args(vec![]).unwrap_err();
        assert!(err.contains("no command"));
    }

    #[test]
    fn parse_unknown_flag_error() {
        let args = vec![os("build"), os("--turbo")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("unknown flag: --turbo"));
    }

    #[test]
    fn parse_iterations_missing_value() {
        let args = vec![os("build"), os("-n")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("requires a value"));
    }

    #[test]
    fn parse_iterations_invalid_value() {
        let args = vec![os("build"), os("-n"), os("abc")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("invalid iteration count"));
    }

    #[test]
    fn parse_duplicate_spec_error() {
        let args = vec![os("build"), os("auth"), os("extra")];
        let err = parse_dynamic_args(args).unwrap_err();
        assert!(err.contains("unexpected argument"));
    }

    #[test]
    fn resolve_direct_prompt_file() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        fs::write(tmp.path().join(".sgf/prompts/build.md"), "prompt").unwrap();

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved, "build");
    }

    #[test]
    fn resolve_via_alias() {
        let tmp = TempDir::new().unwrap();
        let prompts_dir = tmp.path().join(".sgf/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("build.md"), "prompt").unwrap();
        fs::write(prompts_dir.join("config.toml"), "[build]\nalias = \"b\"\n").unwrap();

        let resolved = resolve_command(tmp.path(), "b").unwrap();
        assert_eq!(resolved, "build");
    }

    #[test]
    fn resolve_unknown_command_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

        let err = resolve_command(tmp.path(), "nonexistent").unwrap_err();
        assert!(err.contains("unknown command: nonexistent"));
    }

    #[test]
    fn resolve_prefers_direct_over_alias() {
        let tmp = TempDir::new().unwrap();
        let prompts_dir = tmp.path().join(".sgf/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("build.md"), "prompt").unwrap();
        fs::write(prompts_dir.join("install.md"), "prompt").unwrap();
        fs::write(
            prompts_dir.join("config.toml"),
            "[install]\nalias = \"build\"\n",
        )
        .unwrap();

        // "build" matches the prompt file directly, not the alias
        // (though config validation would catch this shadow — testing resolution order)
        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved, "build");
    }
}
