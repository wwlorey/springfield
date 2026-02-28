use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pn", about = "Agent persistent memory — issue/task tracker")]
struct Cli {
    #[arg(long, env = "PN_ACTOR")]
    actor: Option<String>,

    #[arg(long, default_value_t = false)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Daemon {
        #[arg(long, default_value_t = 7533)]
        port: u16,
        #[arg(long)]
        project_dir: Option<std::path::PathBuf>,
        #[command(subcommand)]
        subcmd: Option<DaemonSubcommand>,
    },
    Where,
}

#[derive(Subcommand)]
enum DaemonSubcommand {
    Status,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon {
            port,
            project_dir,
            subcmd,
        } => match subcmd {
            Some(DaemonSubcommand::Status) => {
                eprintln!("daemon status: not yet implemented");
                std::process::exit(1);
            }
            None => {
                let dir = project_dir.unwrap_or_else(|| std::env::current_dir().unwrap());
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(pensa::daemon::start(port, dir));
            }
        },
        Commands::Where => {
            let dir = std::env::current_dir().unwrap();
            println!("{}", dir.join(".pensa").display());
        }
    }
}
