use std::io::Read;
use std::process::{self, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};

use forma::client::Client;
use forma::output::{self, OutputMode};
use forma::types::{FormaError, validate_stem};

#[derive(Parser)]
#[command(name = "fm", about = "Specification management — forma")]
struct Cli {
    #[arg(long, env = "FM_ACTOR", global = true)]
    actor: Option<String>,

    #[arg(long, default_value_t = false, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Create {
        stem: String,
        #[arg(long)]
        src: Option<String>,
        #[arg(long)]
        purpose: String,
    },
    Show {
        stem: String,
    },
    List {
        #[arg(long)]
        status: Option<String>,
    },
    Update {
        stem: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        src: Option<String>,
        #[arg(long)]
        purpose: Option<String>,
    },
    Delete {
        stem: String,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    Search {
        query: String,
    },
    Count {
        #[arg(long, default_value_t = false)]
        by_status: bool,
    },
    Status,
    History {
        stem: String,
    },
    Section {
        #[command(subcommand)]
        subcmd: SectionSubcommand,
    },
    Ref {
        #[command(subcommand)]
        subcmd: RefSubcommand,
    },
    Export,
    Import,
    Check,
    Doctor {
        #[arg(long, default_value_t = false)]
        fix: bool,
    },
    Where,
    Daemon {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        project_dir: Option<std::path::PathBuf>,
        #[command(subcommand)]
        subcmd: Option<DaemonSubcommand>,
    },
}

#[derive(Subcommand)]
enum SectionSubcommand {
    Add {
        stem: String,
        name: String,
        #[arg(long, default_value_t = false)]
        body_stdin: bool,
        #[arg(long)]
        after: Option<String>,
    },
    Set {
        stem: String,
        slug: String,
        #[arg(long, default_value_t = false)]
        body_stdin: bool,
    },
    Get {
        stem: String,
        slug: String,
    },
    List {
        stem: String,
    },
    Remove {
        stem: String,
        slug: String,
    },
    Move {
        stem: String,
        slug: String,
        #[arg(long)]
        after: String,
    },
}

#[derive(Subcommand)]
enum RefSubcommand {
    Add {
        stem: String,
        target: String,
    },
    Remove {
        stem: String,
        target: String,
    },
    List {
        stem: String,
    },
    Tree {
        stem: String,
        #[arg(long, default_value = "down")]
        direction: String,
    },
    Cycles,
}

#[derive(Subcommand)]
enum DaemonSubcommand {
    Status,
}

fn resolve_actor(flag: Option<String>) -> String {
    if let Some(a) = flag {
        return a;
    }
    if let Ok(a) = std::env::var("FM_ACTOR") {
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

fn fail(err: FormaError, mode: OutputMode) -> ! {
    output::print_error(&err, mode);
    process::exit(1);
}

fn check_stem(stem: &str, mode: OutputMode) {
    if let Err(e) = validate_stem(stem) {
        fail(e, mode);
    }
}

fn read_stdin() -> String {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
    buf
}

fn project_dir() -> std::path::PathBuf {
    forma::db::find_project_root().unwrap_or_else(|| std::env::current_dir().unwrap())
}

fn is_remote_host() -> bool {
    if let Ok(host) = std::env::var("FM_DAEMON_HOST") {
        let h = host.trim();
        return !h.is_empty() && h != "localhost" && h != "127.0.0.1" && h != "::1";
    }
    if std::env::var("FM_DAEMON").is_ok() {
        return true;
    }
    if let Some(dir) = forma::db::find_project_root() {
        let url_file = dir.join(".forma/daemon.url");
        if let Ok(contents) = std::fs::read_to_string(&url_file)
            && !contents.trim().is_empty()
        {
            return true;
        }
    }
    false
}

fn clear_stale_daemon_files(forma_dir: &std::path::Path) {
    let _ = std::fs::remove_file(forma_dir.join("daemon.port"));
    let _ = std::fs::remove_file(forma_dir.join("daemon.project"));
}

fn is_daemon_stale(dir: &std::path::Path) -> bool {
    let project_file = dir.join(".forma/daemon.project");
    let Ok(stored) = std::fs::read_to_string(&project_file) else {
        return false;
    };
    let stored = stored.trim();
    if stored.is_empty() {
        return false;
    }
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let stored_path = std::path::Path::new(stored);
    stored_path != canonical
}

fn ensure_daemon() {
    let dir = project_dir();

    if is_daemon_stale(&dir) {
        eprintln!("fm: stale daemon detected (project directory changed), restarting...");
        clear_stale_daemon_files(&dir.join(".forma"));
    }

    let client = Client::new();
    if client.check_reachable().is_ok() {
        return;
    }

    if is_remote_host() {
        eprintln!(
            "fm: daemon unreachable at {} (remote host configured via FM_DAEMON or FM_DAEMON_HOST)",
            client.base_url()
        );
        process::exit(1);
    }

    let port = forma::db::project_port(&dir);
    eprintln!("fm: starting daemon on port {port}...");

    if let Err(e) = Command::new(std::env::current_exe().unwrap())
        .args([
            "daemon",
            "--port",
            &port.to_string(),
            "--project-dir",
            &dir.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        eprintln!("fm: failed to start daemon: {e}");
        return;
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
        if client.check_reachable().is_ok() {
            eprintln!("fm: daemon ready");
            return;
        }
    }

    eprintln!("fm: warning: daemon did not become ready within 5s");
}

fn needs_daemon(cmd: &Commands) -> bool {
    !matches!(cmd, Commands::Daemon { .. } | Commands::Where)
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

    if needs_daemon(&cli.command) {
        ensure_daemon();
    }

    match cli.command {
        Commands::Daemon {
            port,
            project_dir: explicit_dir,
            subcmd,
        } => match subcmd {
            Some(DaemonSubcommand::Status) => {
                let client = Client::new();
                match client.check_reachable() {
                    Ok(()) => {
                        println!("daemon reachable at {}", client.base_url());
                        let dir = project_dir();
                        let project_file = dir.join(".forma/daemon.project");
                        if let Ok(contents) = std::fs::read_to_string(&project_file) {
                            let stored = contents.trim();
                            if !stored.is_empty() {
                                println!("project directory: {stored}");
                            }
                        }
                        process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("daemon unreachable: {e}");
                        process::exit(1);
                    }
                }
            }
            None => {
                let dir = explicit_dir.unwrap_or_else(project_dir);
                let port = port.unwrap_or_else(|| forma::db::project_port(&dir));
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(forma::daemon::start(port, dir));
            }
        },

        Commands::Where => {
            let dir = project_dir();
            output::print_where(
                &dir.join(".forma").display().to_string(),
                &forma::db::data_dir(&dir).display().to_string(),
            );
        }

        Commands::Create { stem, src, purpose } => {
            check_stem(&stem, mode);
            let client = Client::new();
            match client.create_spec(&stem, src.as_deref(), &purpose, &actor) {
                Ok(v) => output::print_spec(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Show { stem } => {
            check_stem(&stem, mode);
            let client = Client::new();
            match client.get_spec(&stem) {
                Ok(v) => output::print_spec_detail(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::List { status } => {
            let client = Client::new();
            match client.list_specs(status.as_deref()) {
                Ok(v) => output::print_spec_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Update {
            stem,
            status,
            src,
            purpose,
        } => {
            check_stem(&stem, mode);
            let client = Client::new();
            match client.update_spec(
                &stem,
                status.as_deref(),
                src.as_deref(),
                purpose.as_deref(),
                &actor,
            ) {
                Ok(v) => output::print_spec(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Delete { stem, force } => {
            check_stem(&stem, mode);
            let client = Client::new();
            match client.delete_spec(&stem, force, &actor) {
                Ok(_) => output::print_deleted(&stem, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Search { query } => {
            let client = Client::new();
            match client.search_specs(&query) {
                Ok(v) => output::print_spec_list(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Count { by_status } => {
            let client = Client::new();
            match client.count_specs(by_status) {
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

        Commands::History { stem } => {
            check_stem(&stem, mode);
            let client = Client::new();
            match client.spec_history(&stem) {
                Ok(v) => output::print_events(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Section { subcmd } => {
            let client = Client::new();
            match subcmd {
                SectionSubcommand::Add {
                    stem,
                    name,
                    body_stdin,
                    after,
                } => {
                    check_stem(&stem, mode);
                    let body = if body_stdin {
                        read_stdin()
                    } else {
                        String::new()
                    };
                    match client.add_section(&stem, &name, &body, after.as_deref(), &actor) {
                        Ok(v) => output::print_section(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                SectionSubcommand::Set {
                    stem,
                    slug,
                    body_stdin,
                } => {
                    check_stem(&stem, mode);
                    let body = if body_stdin {
                        read_stdin()
                    } else {
                        String::new()
                    };
                    match client.set_section(&stem, &slug, &body, &actor) {
                        Ok(v) => output::print_section(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                SectionSubcommand::Get { stem, slug } => {
                    check_stem(&stem, mode);
                    match client.get_section(&stem, &slug) {
                        Ok(v) => output::print_section_body(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                SectionSubcommand::List { stem } => {
                    check_stem(&stem, mode);
                    match client.list_sections(&stem) {
                        Ok(v) => output::print_section_list(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                SectionSubcommand::Remove { stem, slug } => {
                    check_stem(&stem, mode);
                    match client.remove_section(&stem, &slug, &actor) {
                        Ok(_) => output::print_section_removed(&stem, &slug, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                SectionSubcommand::Move { stem, slug, after } => {
                    check_stem(&stem, mode);
                    match client.move_section(&stem, &slug, &after, &actor) {
                        Ok(v) => output::print_section(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
            }
        }

        Commands::Ref { subcmd } => {
            let client = Client::new();
            match subcmd {
                RefSubcommand::Add { stem, target } => {
                    check_stem(&stem, mode);
                    check_stem(&target, mode);
                    match client.add_ref(&stem, &target, &actor) {
                        Ok(v) => output::print_ref_status(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                RefSubcommand::Remove { stem, target } => {
                    check_stem(&stem, mode);
                    check_stem(&target, mode);
                    match client.remove_ref(&stem, &target, &actor) {
                        Ok(v) => output::print_ref_status(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                RefSubcommand::List { stem } => {
                    check_stem(&stem, mode);
                    match client.list_refs(&stem) {
                        Ok(v) => output::print_ref_list(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                RefSubcommand::Tree { stem, direction } => {
                    check_stem(&stem, mode);
                    match client.ref_tree(&stem, &direction) {
                        Ok(v) => output::print_ref_tree(&v, mode),
                        Err(e) => fail(e, mode),
                    }
                }
                RefSubcommand::Cycles => match client.ref_cycles() {
                    Ok(v) => output::print_cycles(&v, mode),
                    Err(e) => fail(e, mode),
                },
            }
        }

        Commands::Export => {
            let client = Client::new();
            match client.export() {
                Ok(v) => {
                    output::print_import_export(&v, mode);
                    let _ = Command::new("git")
                        .args(["add", ".forma/"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                Err(e) => fail(e, mode),
            }
        }

        Commands::Import => {
            let client = Client::new();
            match client.import() {
                Ok(v) => output::print_import_export(&v, mode),
                Err(e) => fail(e, mode),
            }
        }

        Commands::Check => {
            let client = Client::new();
            match client.check() {
                Ok(v) => {
                    output::print_check(&v, mode);
                    let ok = v.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
                    if !ok {
                        process::exit(1);
                    }
                }
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
