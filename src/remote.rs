use std::io::Write;

use anyhow::{Context, Result};
use tempfile::TempDir;

/// Parse a crate spec like "name@version" into (name, version_req).
///
/// - `"serde"` → `("serde", "*")`
/// - `"serde@1"` → `("serde", "1")`
/// - `"serde@1.0.200"` → `("serde", "=1.0.200")` (3-component = exact pin)
pub fn parse_crate_spec(spec: &str) -> (String, String) {
    match spec.split_once('@') {
        None => (spec.to_string(), "*".to_string()),
        Some((name, version)) => {
            let dots = version.chars().filter(|&c| c == '.').count();
            let version_req = if dots >= 2 {
                format!("={version}")
            } else {
                version.to_string()
            };
            (name.to_string(), version_req)
        }
    }
}

/// Create a temporary workspace with the given crate as a dependency.
/// Returns `TempDir` — the workspace is cleaned up when dropped.
pub fn create_temp_workspace(name: &str, version_req: &str) -> Result<TempDir> {
    let tmp = TempDir::new().context("Failed to create temp directory")?;

    let cargo_toml = format!(
        r#"[package]
name = "brief-tmp"
version = "0.0.0"
edition = "2021"

[dependencies]
{name} = "{version_req}"
"#
    );

    let manifest_path = tmp.path().join("Cargo.toml");
    std::fs::write(&manifest_path, cargo_toml)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    let src_dir = tmp.path().join("src");
    std::fs::create_dir(&src_dir)
        .with_context(|| format!("Failed to create {}", src_dir.display()))?;

    let lib_path = src_dir.join("lib.rs");
    let mut f =
        std::fs::File::create(&lib_path).context("Failed to create temp workspace src/lib.rs")?;
    f.write_all(b"").context("Failed to write empty lib.rs")?;

    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_name() {
        let (name, ver) = parse_crate_spec("serde");
        assert_eq!(name, "serde");
        assert_eq!(ver, "*");
    }

    #[test]
    fn parse_major_version() {
        let (name, ver) = parse_crate_spec("serde@1");
        assert_eq!(name, "serde");
        assert_eq!(ver, "1");
    }

    #[test]
    fn parse_major_minor() {
        let (name, ver) = parse_crate_spec("tokio@1.0");
        assert_eq!(name, "tokio");
        assert_eq!(ver, "1.0");
    }

    #[test]
    fn parse_exact_version() {
        let (name, ver) = parse_crate_spec("serde@1.0.200");
        assert_eq!(name, "serde");
        assert_eq!(ver, "=1.0.200");
    }

    #[test]
    fn create_workspace_produces_valid_layout() {
        let tmp = create_temp_workspace("serde", "*").unwrap();
        assert!(tmp.path().join("Cargo.toml").exists());
        assert!(tmp.path().join("src/lib.rs").exists());

        let content = std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap();
        assert!(content.contains("serde"));
    }
}
