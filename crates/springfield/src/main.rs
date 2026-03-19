use std::ffi::OsString;
use std::path::Path;

use clap::{Parser, Subcommand};

use springfield::cursus;

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

    /// Show available commands with descriptions
    List,

    /// Show project state (future work)
    Status,

    /// Tail a running loop's output
    Logs {
        /// Loop ID to tail
        loop_id: String,
    },

    /// Resume a previous session
    Resume {
        /// Loop ID to resume (omit for interactive picker)
        loop_id: Option<String>,
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

fn resolve_command(root: &Path, name: &str) -> Result<cursus::ResolvedCursus, String> {
    if let Some(resolved) = cursus::resolve_cursus(root, name) {
        return Ok(resolved);
    }

    if let Some(resolved) = cursus::resolve_alias(root, name) {
        return Ok(resolved);
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

    run_cursus_dispatch(&root, &args, resolved);
}

fn run_cursus_dispatch(root: &Path, args: &DynamicArgs, resolved: cursus::ResolvedCursus) -> ! {
    let mut def = resolved.definition.clone();

    if let Err(e) = cursus::toml::validate(&def) {
        springfield::style::print_error(&format!("{}: {e}", resolved.name));
        std::process::exit(1);
    }
    if let Err(e) = cursus::toml::validate_prompts(root, &def) {
        springfield::style::print_error(&format!("{}: {e}", resolved.name));
        std::process::exit(1);
    }

    if let Some(n) = args.iterations {
        for iter in &mut def.iters {
            iter.iterations = n;
        }
    }

    if args.no_push {
        def.auto_push = false;
        for iter in &mut def.iters {
            iter.auto_push = Some(false);
        }
    }

    let mode_override = if args.afk {
        Some(cursus::toml::Mode::Afk)
    } else if args.interactive {
        Some(cursus::toml::Mode::Interactive)
    } else {
        None
    };

    if let Some(ref stem) = args.spec {
        let spec_path = root.join(format!("specs/{stem}.md"));
        if !spec_path.exists() {
            springfield::style::print_error(&format!("spec not found: specs/{stem}.md"));
            std::process::exit(1);
        }
    }

    let config = cursus::runner::CursusConfig {
        spec: args.spec.clone(),
        mode_override,
        no_push: args.no_push,
        ralph_binary: None,
        skip_preflight: false,
    };

    match cursus::runner::run_cursus(root, &resolved.name, &def, &config) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            springfield::style::print_error(&format!("{}: {e}", resolved.name));
            std::process::exit(1);
        }
    }
}


fn run_list(root: &Path) {
    let commands = cursus::list_all(root);

    let builtins = [
        ("init", "Scaffold a new project"),
        ("list", "Show available commands"),
        ("logs", "Tail a running loop's output"),
        ("resume", "Resume a stalled/interrupted run"),
    ];

    let max_name = commands
        .iter()
        .map(|(n, _)| n.len())
        .chain(builtins.iter().map(|(n, _)| n.len()))
        .max()
        .unwrap_or(0);

    if !commands.is_empty() {
        println!("Available commands:\n");
        for (name, desc) in &commands {
            println!("  {:<width$}  {}", name, desc, width = max_name);
        }
        println!();
    }

    println!("Built-ins:\n");
    for (name, desc) in &builtins {
        println!("  {:<width$}  {}", name, desc, width = max_name);
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
        Commands::List => {
            let root = std::env::current_dir().expect("failed to get current directory");
            run_list(&root);
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
        Commands::Resume { loop_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");

            if let Some(ref id) = loop_id
                && let Ok(Some(_)) = cursus::state::read_metadata(&root, id)
            {
                match cursus::runner::resume_cursus(&root, id) {
                    Ok(code) => std::process::exit(code),
                    Err(e) => {
                        springfield::style::print_error(&format!("resume: {e}"));
                        std::process::exit(1);
                    }
                }
            }

            if loop_id.is_none() {
                let cursus_runs = cursus::state::find_resumable_runs(&root).unwrap_or_default();
                if !cursus_runs.is_empty() {
                    eprintln!("Resumable cursus runs:");
                    for (i, run) in cursus_runs.iter().enumerate() {
                        eprintln!(
                            "  {:>2}. {:<40} iter: {:<15} {}",
                            i + 1,
                            run.run_id,
                            run.current_iter,
                            run.status
                        );
                    }
                    eprintln!();
                    eprint!(
                        "Select run (1-{}), or press Enter for legacy sessions: ",
                        cursus_runs.len()
                    );
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_ok()
                        && !input.trim().is_empty()
                        && let Ok(choice) = input.trim().parse::<usize>()
                        && choice >= 1
                        && choice <= cursus_runs.len()
                    {
                        let run_id = &cursus_runs[choice - 1].run_id;
                        match cursus::runner::resume_cursus(&root, run_id) {
                            Ok(code) => std::process::exit(code),
                            Err(e) => {
                                springfield::style::print_error(&format!("resume: {e}"));
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }

            match springfield::orchestrate::run_resume(&root, loop_id.as_deref()) {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    springfield::style::print_error(&format!("resume: {e}"));
                    std::process::exit(1);
                }
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

    const SIMPLE_CURSUS: &str = r#"
description = "Build loop"
alias = "b"
auto_push = true

[[iter]]
name = "build"
prompt = "build.md"
mode = "interactive"
iterations = 30
"#;

    #[test]
    fn resolve_cursus_toml_direct() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_cursus_toml_via_alias() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();

        let resolved = resolve_command(tmp.path(), "b").unwrap();
        assert_eq!(resolved.name, "build");
    }

    #[test]
    fn resolve_bare_prompt_without_cursus_is_unknown() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();
        fs::write(tmp.path().join(".sgf/prompts/deploy.md"), "prompt").unwrap();

        let err = resolve_command(tmp.path(), "deploy").unwrap_err();
        assert!(err.contains("unknown command: deploy"));
    }

    #[test]
    fn resolve_unknown_command_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/prompts")).unwrap();

        let err = resolve_command(tmp.path(), "nonexistent").unwrap_err();
        assert!(err.contains("unknown command: nonexistent"));
    }

    #[test]
    fn resolve_prefers_cursus_direct_over_cursus_alias() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".sgf/cursus")).unwrap();
        fs::write(tmp.path().join(".sgf/cursus/build.toml"), SIMPLE_CURSUS).unwrap();
        fs::write(
            tmp.path().join(".sgf/cursus/test.toml"),
            r#"
description = "Test"
alias = "build"

[[iter]]
name = "test"
prompt = "test.md"
"#,
        )
        .unwrap();

        let resolved = resolve_command(tmp.path(), "build").unwrap();
        assert_eq!(resolved.name, "build");
    }
}
