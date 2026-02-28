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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = sgf::init::run(&root) {
                eprintln!("sgf init: {e}");
                std::process::exit(1);
            }
        }
        Commands::Spec => {
            eprintln!("sgf spec: not yet implemented");
        }
        Commands::Build { .. } => {
            eprintln!("sgf build: not yet implemented");
        }
        Commands::Verify { .. } => {
            eprintln!("sgf verify: not yet implemented");
        }
        Commands::TestPlan { .. } => {
            eprintln!("sgf test-plan: not yet implemented");
        }
        Commands::Test { .. } => {
            eprintln!("sgf test: not yet implemented");
        }
        Commands::Issues { subcmd } => match subcmd {
            IssuesSubcommand::Log => {
                eprintln!("sgf issues log: not yet implemented");
            }
            IssuesSubcommand::Plan { .. } => {
                eprintln!("sgf issues plan: not yet implemented");
            }
        },
        Commands::Status => {
            println!("Not yet implemented");
        }
        Commands::Logs { loop_id } => {
            let root = std::env::current_dir().expect("failed to get current directory");
            if let Err(e) = sgf::loop_mgmt::run_logs(&root, &loop_id) {
                eprintln!("sgf logs: {e}");
                std::process::exit(1);
            }
        }
        Commands::Template { subcmd } => match subcmd {
            TemplateSubcommand::Build => {
                eprintln!("sgf template build: not yet implemented");
            }
        },
    }
}
