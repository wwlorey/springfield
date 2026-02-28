use std::path::Path;
use std::process::Command;

const DOCKERFILE: &str = include_str!("../../../.docker/sandbox-templates/ralph/Dockerfile");

pub fn build_template() -> Result<(), String> {
    let pn_path = locate_pn()?;

    let tmp = tempfile::tempdir()
        .map_err(|e| format!("failed to create temporary build context: {e}"))?;
    let ctx = tmp.path();

    std::fs::write(ctx.join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile: {e}"))?;

    std::fs::copy(&pn_path, ctx.join("pn"))
        .map_err(|e| format!("failed to copy pn binary: {e}"))?;

    let status = Command::new("docker")
        .args(["build", "-t", "ralph-sandbox:latest", "."])
        .current_dir(ctx)
        .status()
        .map_err(|e| format!("failed to run docker build: {e}"))?;

    if !status.success() {
        return Err(format!(
            "docker build failed with exit code {}",
            status.code().unwrap_or(-1)
        ));
    }

    println!("ralph-sandbox:latest built successfully");
    Ok(())
}

fn locate_pn() -> Result<String, String> {
    let output = Command::new("which")
        .arg("pn")
        .output()
        .map_err(|e| format!("failed to run `which pn`: {e}"))?;

    if !output.status.success() {
        return Err(
            "pn not found on PATH — install pensa first (`cargo install --path crates/pensa`)"
                .to_string(),
        );
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err("pn not found on PATH".to_string());
    }

    if !Path::new(&path).exists() {
        return Err(format!("pn binary at {path} does not exist"));
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dockerfile_is_embedded() {
        assert!(DOCKERFILE.contains("FROM docker/sandbox-templates:claude-code"));
        assert!(DOCKERFILE.contains("COPY pn /usr/local/bin/pn"));
        assert!(DOCKERFILE.contains("rustc --version"));
    }

    #[test]
    fn dockerfile_contains_rust_install() {
        assert!(DOCKERFILE.contains("rustup default stable"));
        assert!(DOCKERFILE.contains("rustup component add rustfmt clippy"));
    }

    #[test]
    fn dockerfile_contains_tauri_deps() {
        assert!(DOCKERFILE.contains("libwebkit2gtk-4.1-dev"));
        assert!(DOCKERFILE.contains("cargo install tauri-cli"));
    }

    #[test]
    fn dockerfile_contains_pnpm_setup() {
        assert!(DOCKERFILE.contains("corepack enable"));
        assert!(DOCKERFILE.contains("pnpm setup"));
    }

    #[test]
    fn dockerfile_sets_agent_user() {
        assert!(DOCKERFILE.contains("USER agent"));
        assert!(DOCKERFILE.contains("WORKDIR /home/agent"));
    }

    #[test]
    fn build_context_is_created_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = tmp.path();

        std::fs::write(ctx.join("Dockerfile"), DOCKERFILE).unwrap();
        let content = std::fs::read_to_string(ctx.join("Dockerfile")).unwrap();
        assert_eq!(content, DOCKERFILE);
    }

    #[test]
    fn build_context_includes_pn_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = tmp.path();

        // Create a fake pn binary to copy
        let fake_pn = ctx.join("pn_src");
        std::fs::write(&fake_pn, b"fake-pn-binary").unwrap();

        std::fs::copy(&fake_pn, ctx.join("pn")).unwrap();
        let content = std::fs::read(ctx.join("pn")).unwrap();
        assert_eq!(content, b"fake-pn-binary");
    }
}
