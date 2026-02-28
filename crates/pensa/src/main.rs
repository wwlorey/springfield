use std::process;

use clap::{Parser, Subcommand};

use pensa::client::Client;
use pensa::error::PensaError;
use pensa::output::{self, OutputMode};
use pensa::types::{CreateIssueParams, IssueType, ListFilters, Priority, Status};

#[derive(Parser)]
#[command(name = "pn", about = "Agent persistent memory — issue/task tracker")]
struct Cli {
    #[arg(long, env = "PN_ACTOR", global = true)]
    actor: Option<String>,

    #[arg(long, default_value_t = false, global = true)]
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
    Create {
        title: String,
        #[arg(short = 't', long)]
        issue_type: IssueType,
        #[arg(short = 'p', long, default_value = "p2")]
        priority: Priority,
        #[arg(short = 'a', long)]
        assignee: Option<String>,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        fixes: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "dep")]
        deps: Vec<String>,
    },
    Show {
        id: String,
    },
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        status: Option<Status>,
        #[arg(short = 'p', long)]
        priority: Option<Priority>,
        #[arg(short = 'a', long)]
        assignee: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        fixes: Option<String>,
        #[arg(long, default_value_t = false)]
        claim: bool,
        #[arg(long, default_value_t = false)]
        unclaim: bool,
    },
    Close {
        id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Reopen {
        id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Release {
        id: String,
    },
    Delete {
        id: String,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    List {
        #[arg(long)]
        status: Option<Status>,
        #[arg(short = 'p', long)]
        priority: Option<Priority>,
        #[arg(short = 'a', long)]
        assignee: Option<String>,
        #[arg(short = 't', long)]
        issue_type: Option<IssueType>,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        sort: Option<String>,
        #[arg(short = 'n', long)]
        limit: Option<usize>,
    },
    Ready {
        #[arg(short = 'n', long)]
        limit: Option<usize>,
        #[arg(short = 'p', long)]
        priority: Option<Priority>,
        #[arg(short = 'a', long)]
        assignee: Option<String>,
        #[arg(short = 't', long)]
        issue_type: Option<IssueType>,
        #[arg(long)]
        spec: Option<String>,
    },
    Blocked,
    Search {
        query: String,
    },
    Count {
        #[arg(long, default_value_t = false)]
        by_status: bool,
        #[arg(long, default_value_t = false)]
        by_priority: bool,
        #[arg(long, default_value_t = false)]
        by_issue_type: bool,
        #[arg(long, default_value_t = false)]
        by_assignee: bool,
    },
    Status,
    History {
        id: String,
    },
    Dep {
        #[command(subcommand)]
        subcmd: DepSubcommand,
    },
    Comment {
        #[command(subcommand)]
        subcmd: CommentSubcommand,
    },
    Export,
    Import,
    Doctor {
        #[arg(long, default_value_t = false)]
        fix: bool,
    },
}

#[derive(Subcommand)]
enum DaemonSubcommand {
    Status,
}

#[derive(Subcommand)]
enum DepSubcommand {
    Add {
        child: String,
        parent: String,
    },
    Remove {
        child: String,
        parent: String,
    },
    List {
        id: String,
    },
    Tree {
        id: String,
        #[arg(long, default_value = "down")]
        direction: String,
    },
    Cycles,
}

#[derive(Subcommand)]
enum CommentSubcommand {
    Add { id: String, text: String },
    List { id: String },
}

fn resolve_actor(flag: Option<String>) -> String {
    if let Some(a) = flag {
        return a;
    }
    if let Ok(a) = std::env::var("PN_ACTOR") {
        return a;
    }
    if let Ok(out) = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        && out.status.success()
    {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !name.is_empty() {
            return name;
        }
    }
    std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
}

fn fail(err: PensaError, mode: OutputMode) -> ! {
    output::print_error(&err, mode);
    process::exit(1);
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    let mode = if cli.json {
        OutputMode::Json
    } else {
        OutputMode::Human
    };
    let actor = resolve_actor(cli.actor);

    match cli.command {
        Commands::Daemon {
            port,
            project_dir,
            subcmd,
        } => match subcmd {
            Some(DaemonSubcommand::Status) => {
                let client = Client::new();
                match client.check_reachable() {
                    Ok(()) => {
                        println!("daemon reachable at {}", client.base_url());
                        process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("daemon unreachable: {e}");
                        process::exit(1);
                    }
                }
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

        Commands::Create {
            title,
            issue_type,
            priority,
            assignee,
            spec,
            fixes,
            description,
            deps,
        } => {
            let client = Client::new();
            let params = CreateIssueParams {
                title,
                issue_type,
                priority,
                description,
                spec,
                fixes,
                assignee,
                deps,
                actor: actor.clone(),
            };
            match client.create_issue(&params) {
                Ok(v) => output::print_issue(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Show { id } => {
            let client = Client::new();
            match client.get_issue(&id) {
                Ok(v) => output::print_issue_detail(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Update {
            id,
            title,
            status,
            priority,
            assignee,
            description,
            spec,
            fixes,
            claim,
            unclaim,
        } => {
            let client = Client::new();
            let mut body = serde_json::Map::new();
            if let Some(t) = title {
                body.insert("title".into(), serde_json::Value::String(t));
            }
            if let Some(s) = status {
                body.insert(
                    "status".into(),
                    serde_json::Value::String(s.as_str().to_string()),
                );
            }
            if let Some(p) = priority {
                body.insert(
                    "priority".into(),
                    serde_json::Value::String(p.as_str().to_string()),
                );
            }
            if let Some(a) = assignee {
                body.insert("assignee".into(), serde_json::Value::String(a));
            }
            if let Some(d) = description {
                body.insert("description".into(), serde_json::Value::String(d));
            }
            if let Some(s) = spec {
                body.insert("spec".into(), serde_json::Value::String(s));
            }
            if let Some(f) = fixes {
                body.insert("fixes".into(), serde_json::Value::String(f));
            }
            if claim {
                body.insert("claim".into(), serde_json::Value::Bool(true));
            }
            if unclaim {
                body.insert("unclaim".into(), serde_json::Value::Bool(true));
            }

            match client.update_issue(&id, &serde_json::Value::Object(body), &actor) {
                Ok(v) => output::print_issue(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Close { id, reason, force } => {
            let client = Client::new();
            match client.close_issue(&id, reason.as_deref(), force, &actor) {
                Ok(v) => output::print_issue(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Reopen { id, reason } => {
            let client = Client::new();
            match client.reopen_issue(&id, reason.as_deref(), &actor) {
                Ok(v) => output::print_issue(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Release { id } => {
            let client = Client::new();
            match client.release_issue(&id, &actor) {
                Ok(v) => output::print_issue(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Delete { id, force } => {
            let client = Client::new();
            match client.delete_issue(&id, force) {
                Ok(()) => output::print_deleted(mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::List {
            status,
            priority,
            assignee,
            issue_type,
            spec,
            sort,
            limit,
        } => {
            let client = Client::new();
            let filters = ListFilters {
                status,
                priority,
                assignee,
                issue_type,
                spec,
                sort,
                limit,
            };
            match client.list_issues(&filters) {
                Ok(v) => output::print_issue_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Ready {
            limit,
            priority,
            assignee,
            issue_type,
            spec,
        } => {
            let client = Client::new();
            let filters = ListFilters {
                priority,
                assignee,
                issue_type,
                spec,
                limit,
                ..Default::default()
            };
            match client.ready_issues(&filters) {
                Ok(v) => output::print_issue_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Blocked => {
            let client = Client::new();
            match client.blocked_issues() {
                Ok(v) => output::print_issue_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Search { query } => {
            let client = Client::new();
            match client.search_issues(&query) {
                Ok(v) => output::print_issue_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Count {
            by_status,
            by_priority,
            by_issue_type,
            by_assignee,
        } => {
            let client = Client::new();
            match client.count_issues(by_status, by_priority, by_issue_type, by_assignee) {
                Ok(v) => output::print_count(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Status => {
            let client = Client::new();
            match client.project_status() {
                Ok(v) => output::print_status(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::History { id } => {
            let client = Client::new();
            match client.issue_history(&id) {
                Ok(v) => output::print_events(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Dep { subcmd } => {
            let client = Client::new();
            match subcmd {
                DepSubcommand::Add { child, parent } => {
                    match client.add_dep(&child, &parent, &actor) {
                        Ok(v) => output::print_dep_status(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                DepSubcommand::Remove { child, parent } => {
                    match client.remove_dep(&child, &parent) {
                        Ok(v) => output::print_dep_status(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                DepSubcommand::List { id } => match client.list_deps(&id) {
                    Ok(v) => output::print_issue_list(&v, mode),
                    Err(e) => fail(e, mode),
                },
                DepSubcommand::Tree { id, direction } => match client.dep_tree(&id, &direction) {
                    Ok(v) => output::print_dep_tree(&v, mode),
                    Err(e) => fail(e, mode),
                },
                DepSubcommand::Cycles => match client.dep_cycles() {
                    Ok(v) => output::print_cycles(&v, mode),
                    Err(e) => fail(e, mode),
                },
            }
        }

        Commands::Comment { subcmd } => {
            let client = Client::new();
            match subcmd {
                CommentSubcommand::Add { id, text } => {
                    match client.add_comment(&id, &text, &actor) {
                        Ok(v) => output::print_comment(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                CommentSubcommand::List { id } => match client.list_comments(&id) {
                    Ok(v) => output::print_comment_list(&v, mode),
                    Err(e) => fail(e, mode),
                },
            }
        }

        Commands::Export => {
            let client = Client::new();
            match client.export() {
                Ok(v) => {
                    output::print_export_import(&v, mode);
                    let _ = std::process::Command::new("git")
                        .args(["add", ".pensa/*.jsonl"])
                        .status();
                }
                Err(e) => fail(e, mode),
            }
        }

        Commands::Import => {
            let client = Client::new();
            match client.import() {
                Ok(v) => output::print_export_import(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Doctor { fix } => {
            let client = Client::new();
            match client.doctor(fix) {
                Ok(v) => output::print_doctor(&v, mode),
                Err(e) => fail(e, mode),
            }
        }
    }
}
