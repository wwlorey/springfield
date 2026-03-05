use std::process::Command;
use std::sync::OnceLock;

static RESOLVED_CONTEXT: OnceLock<Option<String>> = OnceLock::new();

fn resolve_context() -> Option<String> {
    if let Ok(ctx) = std::env::var("SGF_DOCKER_CONTEXT")
        && !ctx.is_empty()
    {
        return Some(ctx);
    }
    let output = Command::new("docker")
        .args(["context", "show"])
        .output()
        .ok()?;
    if output.status.success() {
        let ctx = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ctx.is_empty() {
            return Some(ctx);
        }
    }
    None
}

pub fn docker_command() -> Command {
    let ctx = RESOLVED_CONTEXT.get_or_init(resolve_context);
    let mut cmd = Command::new("docker");
    if let Some(ctx) = ctx {
        cmd.args(["--context", ctx]);
    }
    cmd
}
