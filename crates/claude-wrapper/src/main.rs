mod resolve;

use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

const DOWNSTREAM_BINARY: &str = "claude-wrapper-secret";

fn main() {
    let cwd = env::current_dir().unwrap_or_else(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        std::process::exit(1);
    });

    let home = dirs::home_dir();
    if home.is_none() {
        eprintln!("warning: cannot determine home directory, skipping global lookups");
    }

    let files = resolve::resolve_context_files(&cwd, home.as_deref());

    let passthrough_args: Vec<String> = env::args().skip(1).collect();

    let mut args: Vec<String> = Vec::new();

    if !files.is_empty() {
        let prompt = files
            .iter()
            .map(|f| format!("study @{f}"))
            .collect::<Vec<_>>()
            .join(";");
        args.push("--append-system-prompt".to_string());
        args.push(prompt);
    }

    args.extend(passthrough_args);

    let mut cmd = Command::new(DOWNSTREAM_BINARY);
    cmd.args(&args);

    let err = cmd.exec();
    eprintln!("error: failed to exec {DOWNSTREAM_BINARY}: {err}");
    std::process::exit(1);
}
