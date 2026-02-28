use clap::{Parser, Subcommand};

/// CLI entry point for Springfield — scaffolding, prompt assembly, loop orchestration.
#[derive(Parser)]
#[command(name = "sgf")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new project
    Init,

    /// Generate specs and implementation plan (interactive)
    Spec,

    /// Run build loop
    Build {
        /// Spec stem to build
        spec: String,

        #[command(flatten)]
        opts: LoopOpts,
    },

    /// Run verification loop
    Verify {
        #[command(flatten)]
        opts: LoopOpts,
    },

    /// Run test plan generation loop
    TestPlan {
        #[command(flatten)]
        opts: LoopOpts,
    },

    /// Run test execution loop
    Test {
        /// Spec stem to test
        spec: String,

        #[command(flatten)]
        opts: LoopOpts,
    },

    /// Issue management
    Issues {
        #[command(subcommand)]
        subcmd: IssuesSubcommand,
    },

    /// Show project state (future work)
    Status,

    /// Tail a running loop's output
    Logs {
        /// Loop ID to tail
        loop_id: String,
    },

    /// Docker sandbox template management
    Template {
        #[command(subcommand)]
        subcmd: TemplateSubcommand,
    },
}

/// Common flags for looped stages.
#[derive(Parser)]
struct LoopOpts {
    /// AFK mode: NDJSON stream parsing with formatted output
    #[arg(short = 'a', long)]
    afk: bool,

    /// Disable auto-push after commits
    #[arg(long)]
    no_push: bool,

    /// Number of iterations
    #[arg(default_value_t = 30)]
    iterations: u32,
}

#[derive(Subcommand)]
enum IssuesSubcommand {
    /// Interactive session for logging bugs
    Log,

    /// Run bug planning loop
    Plan {
        #[command(flatten)]
        opts: LoopOpts,
    },
}

#[derive(Subcommand)]
enum TemplateSubcommand {
    /// Rebuild Docker sandbox template
    Build,
}

fn run_loop(stage: &str, spec: Option<&str>, opts: &LoopOpts, prompt_template: Option<&str>) -> ! {
    let root = std::env::current_dir().expect("failed to get current directory");
    let config = springfield::orchestrate::LoopConfig {
        stage: stage.to_string(),
        spec: spec.map(|s| s.to_string()),
        afk: opts.afk,
        no_push: opts.no_push,
        iterations: opts.iterations,
        ralph_binary: None,
        skip_preflight: false,
        prompt_template: prompt_template.map(|s| s.to_string()),
    };
    match springfield::orchestrate::run(&root, &config) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("sgf {stage}: {e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::init::run(&root) {
                eprintln!("sgf init: {e}");
                std::process::exit(1);
            }
        }
        Commands::Spec => {
            let opts = LoopOpts {
                afk: false,
                no_push: false,
                iterations: 1,
            };
            run_loop("spec", None, &opts, None);
        }
        Commands::Build { spec, opts } => {
            run_loop("build", Some(&spec), &opts, None);
        }
        Commands::Verify { opts } => {
            run_loop("verify", None, &opts, None);
        }
        Commands::TestPlan { opts } => {
            run_loop("test-plan", None, &opts, None);
        }
        Commands::Test { spec, opts } => {
            run_loop("test", Some(&spec), &opts, None);
        }
        Commands::Issues { subcmd } => match subcmd {
            IssuesSubcommand::Log => {
                let opts = LoopOpts {
                    afk: false,
                    no_push: false,
                    iterations: 1,
                };
                run_loop("issues-log", None, &opts, Some("issues"));
            }
            IssuesSubcommand::Plan { opts } => {
                run_loop("issues-plan", None, &opts, None);
            }
        },
        Commands::Status => {
            println!("Not yet implemented");
        }
        Commands::Logs { loop_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = springfield::loop_mgmt::run_logs(&root, &loop_id) {
                eprintln!("sgf logs: {e}");
                std::process::exit(1);
            }
        }
        Commands::Template { subcmd } => match subcmd {
            TemplateSubcommand::Build => {
                if let Err(e) = springfield::template::build_template() {
                    eprintln!("sgf template build: {e}");
                    std::process::exit(1);
                }
            }
        },
    }
}
