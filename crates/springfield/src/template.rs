use std::io;
use std::path::{Path, PathBuf};

use docker_ctx::docker_command;

use sha2::{Digest, Sha256};

const DOCKERFILE: &str = include_str!("../../../.docker/sandbox-templates/ralph/Dockerfile");

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn dockerfile_hash() -> String {
    sha256_hex(DOCKERFILE.as_bytes())
}

fn pensa_src_hash(pensa_dir: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let cargo_toml = pensa_dir.join("Cargo.toml");
    hasher.update(
        std::fs::read(&cargo_toml)
            .map_err(|e| format!("failed to read {}: {e}", cargo_toml.display()))?,
    );
    let mut src_files: Vec<PathBuf> = std::fs::read_dir(pensa_dir.join("src"))
        .map_err(|e| format!("failed to read pensa src dir: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    src_files.sort();
    for path in src_files {
        hasher.update(
            std::fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?,
        );
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn locate_pensa_crate() -> Result<PathBuf, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pensa_dir = manifest_dir.parent().unwrap().join("pensa");
    if !pensa_dir.join("Cargo.toml").exists() {
        return Err(format!("pensa crate not found at {}", pensa_dir.display()));
    }
    Ok(pensa_dir)
}

fn copy_pensa_source(pensa_dir: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("failed to create pensa-src dir: {e}"))?;

    let cargo_toml = std::fs::read_to_string(pensa_dir.join("Cargo.toml"))
        .map_err(|e| format!("failed to read pensa Cargo.toml: {e}"))?;
    let standalone = cargo_toml
        .replace("version.workspace = true", "version = \"0.1.0\"")
        .replace("edition.workspace = true", "edition = \"2024\"")
        .replace("license.workspace = true", "license = \"MIT\"");
    std::fs::write(dest.join("Cargo.toml"), standalone)
        .map_err(|e| format!("failed to write standalone Cargo.toml: {e}"))?;

    let src_dest = dest.join("src");
    std::fs::create_dir_all(&src_dest)
        .map_err(|e| format!("failed to create pensa-src/src dir: {e}"))?;
    for entry in std::fs::read_dir(pensa_dir.join("src"))
        .map_err(|e| format!("failed to read pensa src dir: {e}"))?
    {
        let entry = entry.map_err(|e| format!("failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            std::fs::copy(&path, src_dest.join(path.file_name().unwrap()))
                .map_err(|e| format!("failed to copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

pub fn build_template() -> Result<(), String> {
    let pensa_dir = locate_pensa_crate()?;

    let tmp = tempfile::tempdir()
        .map_err(|e| format!("failed to create temporary build context: {e}"))?;
    let ctx = tmp.path();

    std::fs::write(ctx.join("Dockerfile"), DOCKERFILE)
        .map_err(|e| format!("failed to write Dockerfile: {e}"))?;

    copy_pensa_source(&pensa_dir, &ctx.join("pensa-src"))?;

    let pn_h = pensa_src_hash(&pensa_dir)?;
    let df_h = dockerfile_hash();

    let status = docker_command()
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
    let output = docker_command()
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

    if let Ok(pensa_dir) = locate_pensa_crate()
        && let Ok(current_pn) = pensa_src_hash(&pensa_dir)
        && img_pn_hash.as_deref() != Some(&current_pn)
    {
        reasons.push("pensa source has changed");
    }

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
    fn pensa_src_hash_missing_dir() {
        let result = pensa_src_hash(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn pensa_src_hash_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(src.join("main.rs"), b"fn main() {}").unwrap();
        let h1 = pensa_src_hash(tmp.path()).unwrap();
        let h2 = pensa_src_hash(tmp.path()).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn locate_pensa_crate_finds_crate() {
        let result = locate_pensa_crate();
        assert!(result.is_ok());
        assert!(result.unwrap().join("Cargo.toml").exists());
    }

    #[test]
    fn copy_pensa_source_inlines_workspace_fields() {
        let pensa_dir = locate_pensa_crate().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("pensa-src");
        copy_pensa_source(&pensa_dir, &dest).unwrap();
        let cargo = std::fs::read_to_string(dest.join("Cargo.toml")).unwrap();
        assert!(!cargo.contains("workspace"));
        assert!(cargo.contains("version = \"0.1.0\""));
        assert!(dest.join("src").join("main.rs").exists());
    }

    #[test]
    fn dockerfile_is_embedded() {
        assert!(DOCKERFILE.contains("FROM docker/sandbox-templates:claude-code"));
        assert!(DOCKERFILE.contains("COPY --chown=agent:agent pensa-src"));
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
    fn build_context_includes_pensa_source() {
        let pensa_dir = locate_pensa_crate().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let ctx = tmp.path();

        std::fs::write(ctx.join("Dockerfile"), DOCKERFILE).unwrap();
        copy_pensa_source(&pensa_dir, &ctx.join("pensa-src")).unwrap();

        assert!(ctx.join("pensa-src").join("Cargo.toml").exists());
        assert!(ctx.join("pensa-src").join("src").join("main.rs").exists());
    }
}
