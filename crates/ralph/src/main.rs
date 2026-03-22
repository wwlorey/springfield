use springfield::iter_runner::{self, IterRunnerConfig};

use clap::Parser;
use shutdown::{ShutdownConfig, ShutdownController};
use tracing::error;

/// Iterative Claude Code runner via direct `cl` invocation.
///
/// Runs Claude Code repeatedly against a prompt file, formatting NDJSON
/// stream output for readable AFK execution.
#[derive(Parser)]
#[command(name = "ralph")]
struct Cli {
    /// Run in AFK mode (non-interactive, formatted NDJSON stream)
    #[arg(short = 'a', long)]
    afk: bool,

    /// Display ASCII art startup banner
    #[arg(long)]
    banner: bool,

    /// Loop identifier (sgf-generated, included in banner output)
    #[arg(long)]
    loop_id: Option<String>,

    /// Number of iterations to run
    #[arg(default_value_t = 1)]
    iterations: u32,

    /// Prompt file path or inline text string
    #[arg(default_value = "prompt.md")]
    prompt: String,

    /// Auto-push after new commits
    #[arg(long, env = "RALPH_AUTO_PUSH", default_value = "true", value_parser = parse_bool, num_args = 1)]
    auto_push: bool,

    /// Override: path to executable replacing agent invocation (for testing)
    #[arg(long, env = "RALPH_COMMAND")]
    command: Option<String>,

    /// Additional prompt file path (repeatable)
    #[arg(long = "prompt-file")]
    prompt_file: Vec<String>,

    /// Path to log file — ralph tees its output here
    #[arg(long)]
    log_file: Option<std::path::PathBuf>,

    /// Pre-assigned Claude session ID (UUID), passed through to cl as --session-id
    #[arg(long, conflicts_with = "resume")]
    session_id: Option<String>,

    /// Resume a previous Claude session by session ID, passed through to cl as --resume
    #[arg(long, conflicts_with = "session_id")]
    resume: Option<String>,
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(format!(
            "invalid boolean value '{other}': expected true/false, 1/0, or yes/no"
        )),
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Validate prompt files before passing to iter runner
    for path in &cli.prompt_file {
        if !std::path::Path::new(path).exists() {
            error!(path, "prompt file not found");
            std::process::exit(1);
        }
    }

    let config = IterRunnerConfig {
        afk: cli.afk,
        banner: cli.banner,
        loop_id: cli.loop_id,
        iterations: cli.iterations,
        prompt: cli.prompt,
        auto_push: cli.auto_push,
        command: cli.command.clone(),
        prompt_files: cli.prompt_file,
        log_file: cli.log_file,
        session_id: cli.session_id,
        resume: cli.resume,
        env_vars: Vec::new(),
        runner_name: Some("Ralph".to_string()),
    };

    let shutdown_config = ShutdownConfig {
        monitor_stdin: std::env::var("SGF_MANAGED").is_err(),
        ..Default::default()
    };
    let controller =
        ShutdownController::new(shutdown_config).expect("Failed to create ShutdownController");

    let exit_code = iter_runner::run_iteration_loop(config, &controller);
    std::process::exit(exit_code as i32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use springfield::iter_runner::{SENTINEL, SENTINEL_MAX_DEPTH, find_sentinel};

    #[test]
    fn find_sentinel_at_root() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), SENTINEL_MAX_DEPTH).is_some());
    }

    #[test]
    fn find_sentinel_nested() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), SENTINEL_MAX_DEPTH).is_some());
    }

    #[test]
    fn find_sentinel_too_deep() {
        let dir = tempfile::TempDir::new().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join(SENTINEL), "").unwrap();
        assert!(find_sentinel(dir.path(), SENTINEL_MAX_DEPTH).is_none());
    }

    #[test]
    fn cli_session_id_flag_parses() {
        let cli = Cli::parse_from([
            "ralph",
            "--session-id",
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "1",
            "prompt.md",
        ]);
        assert_eq!(
            cli.session_id.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert!(cli.resume.is_none());
    }

    #[test]
    fn cli_resume_flag_parses() {
        let cli = Cli::parse_from([
            "ralph",
            "--resume",
            "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "1",
        ]);
        assert_eq!(
            cli.resume.as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
        assert!(cli.session_id.is_none());
    }

    #[test]
    fn cli_session_id_and_resume_conflict() {
        let result = Cli::try_parse_from(["ralph", "--session-id", "id1", "--resume", "id2", "1"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_banner_flag_defaults_false() {
        let cli = Cli::parse_from(["ralph", "1", "prompt.md"]);
        assert!(!cli.banner);
    }

    #[test]
    fn cli_banner_flag_parses() {
        let cli = Cli::parse_from(["ralph", "--banner", "1", "prompt.md"]);
        assert!(cli.banner);
    }
}
