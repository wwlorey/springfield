use std::io;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

const DOCKERFILE: &str = include_str!("../../../.docker/sandbox-templates/ralph/Dockerfile");

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn dockerfile_hash() -> String {
    sha256_hex(DOCKERFILE.as_bytes())
}

fn pn_hash(path: &str) -> Result<String, String> {
    let data =
        std::fs::read(path).map_err(|e| format!("failed to read pn binary at {path}: {e}"))?;
    Ok(sha256_hex(&data))
}

pub fn build_template() -> Result<(), String> {
    let pn_path = locate_pn()?;

    let tmp = tempfile::tempdir()
        .map_err(|e| format!("failed to create temporary build context: {e}"))?;
    let ctx = tmp.path();

    std::fs::write(ctx.join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile: {e}"))?;

    std::fs::copy(&pn_path, ctx.join("pn"))
        .map_err(|e| format!("failed to copy pn binary: {e}"))?;

    let pn_h = pn_hash(&pn_path)?;
    let df_h = dockerfile_hash();

    let status = Command::new("docker")
        .args([
            "build",
            "-t",
            "ralph-sandbox:latest",
            "--label",
            &format!("pn_hash={pn_h}"),
            "--label",
            &format!("dockerfile_hash={df_h}"),
            ".",
        ])
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

/// Returns `Some((pn_hash, dockerfile_hash))` if the image exists.
/// Either label may be `None` if the image was built without labels.
/// Returns `None` if the image does not exist.
fn inspect_template_labels() -> Option<(Option<String>, Option<String>)> {
    let output = Command::new("docker")
        .args([
            "image",
            "inspect",
            "--format",
            "{{index .Config.Labels \"pn_hash\"}}|{{index .Config.Labels \"dockerfile_hash\"}}",
            "ralph-sandbox:latest",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<&str> = raw.splitn(2, '|').collect();
    if parts.len() != 2 {
        return Some((None, None));
    }

    let parse = |v: &str| -> Option<String> {
        let v = v.trim();
        if v.is_empty() || v == "<no value>" {
            None
        } else {
            Some(v.to_string())
        }
    };

    Some((parse(parts[0]), parse(parts[1])))
}

fn check_staleness(img_pn_hash: Option<String>, img_df_hash: Option<String>) {
    if img_pn_hash.is_none() && img_df_hash.is_none() {
        eprintln!(
            "sgf: warning: ralph-sandbox:latest has no version labels. \
             Run 'sgf template build' to update."
        );
        return;
    }

    let mut reasons = Vec::new();

    let current_df = dockerfile_hash();
    if img_df_hash.as_deref() != Some(&current_df) {
        reasons.push("Dockerfile has changed");
    }

    if let Ok(pn_path) = locate_pn()
        && let Ok(current_pn) = pn_hash(&pn_path)
        && img_pn_hash.as_deref() != Some(&current_pn)
    {
        reasons.push("pn binary has changed");
    }
    // If pn can't be located during staleness check, skip silently

    if !reasons.is_empty() {
        eprintln!(
            "sgf: warning: ralph-sandbox:latest may be stale ({}). \
             Run 'sgf template build' to update.",
            reasons.join(", ")
        );
    }
}

pub fn ensure_template() -> io::Result<()> {
    match inspect_template_labels() {
        None => {
            eprintln!(
                "sgf: ralph-sandbox:latest not found, building template \
                 (this may take several minutes)..."
            );
            build_template().map_err(io::Error::other)?;
            eprintln!("sgf: template built successfully");
        }
        Some((pn_h, df_h)) => {
            check_staleness(pn_h, df_h);
        }
    }
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
    fn sha256_hex_deterministic() {
        let hash1 = sha256_hex(b"hello world");
        let hash2 = sha256_hex(b"hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
        assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sha256_hex_different_inputs() {
        let a = sha256_hex(b"hello");
        let b = sha256_hex(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn dockerfile_hash_stable() {
        let h1 = dockerfile_hash();
        let h2 = dockerfile_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn pn_hash_missing_file() {
        let result = pn_hash("/nonexistent/path/to/pn");
        assert!(result.is_err());
    }

    #[test]
    fn pn_hash_works_on_temp_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"fake-pn-binary").unwrap();
        let result = pn_hash(tmp.path().to_str().unwrap());
        assert!(result.is_ok());
        let hash = result.unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, sha256_hex(b"fake-pn-binary"));
    }

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
