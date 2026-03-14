use std::io::Write;
use std::path::{Path, PathBuf};

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

/// A workspace directory that is either a persistent cache dir or a temporary dir.
pub enum WorkspaceDir {
    Cached(PathBuf),
    Temp(TempDir),
}

impl WorkspaceDir {
    pub fn path(&self) -> &Path {
        match self {
            WorkspaceDir::Cached(p) => p,
            WorkspaceDir::Temp(t) => t.path(),
        }
    }
}

/// Resolve the cache root directory.
///
/// Priority: `$CARGO_BRIEF_CACHE_DIR` > `$XDG_CACHE_HOME/cargo-brief/crates` > `$HOME/.cache/cargo-brief/crates`
fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_BRIEF_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("cargo-brief/crates");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cache/cargo-brief/crates")
}

/// Convert a crate spec into a filesystem-safe directory name.
///
/// `@` → `@` (already safe), other special chars replaced with `_`.
fn sanitize_spec(spec: &str) -> String {
    spec.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '@' => c,
            _ => '_',
        })
        .collect()
}

/// Write the workspace Cargo.toml and src/lib.rs into the given directory.
fn write_workspace_files(
    dir: &Path,
    name: &str,
    version_req: &str,
    features: Option<&str>,
) -> Result<()> {
    let dep_value = match features {
        Some(f) => {
            let feat_list: Vec<&str> = f.split(',').map(|s| s.trim()).collect();
            let feat_str = feat_list
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ version = \"{version_req}\", features = [{feat_str}] }}")
        }
        None => format!("\"{version_req}\""),
    };

    let cargo_toml = format!(
        r#"[package]
name = "brief-tmp"
version = "0.0.0"
edition = "2021"

[dependencies]
{name} = {dep_value}
"#
    );

    let manifest_path = dir.join("Cargo.toml");
    std::fs::write(&manifest_path, cargo_toml)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    let src_dir = dir.join("src");
    if !src_dir.exists() {
        std::fs::create_dir_all(&src_dir)
            .with_context(|| format!("Failed to create {}", src_dir.display()))?;
    }

    let lib_path = src_dir.join("lib.rs");
    let mut f =
        std::fs::File::create(&lib_path).context("Failed to create workspace src/lib.rs")?;
    f.write_all(b"").context("Failed to write empty lib.rs")?;

    Ok(())
}

/// Resolve a workspace directory for a remote crate spec.
///
/// When `no_cache` is true, returns a `TempDir` (cleaned up on drop).
/// Otherwise, returns a persistent cache directory under `cache_dir()/sanitize_spec(spec)`.
pub fn resolve_workspace(
    spec: &str,
    features: Option<&str>,
    no_cache: bool,
) -> Result<WorkspaceDir> {
    let (name, version_req) = parse_crate_spec(spec);

    if no_cache {
        let tmp = TempDir::new().context("Failed to create temp directory")?;
        write_workspace_files(tmp.path(), &name, &version_req, features)?;
        return Ok(WorkspaceDir::Temp(tmp));
    }

    let dir = cache_dir().join(sanitize_spec(spec));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;
    write_workspace_files(&dir, &name, &version_req, features)?;
    Ok(WorkspaceDir::Cached(dir))
}

/// Create a temporary workspace with the given crate as a dependency.
/// Returns `TempDir` — the workspace is cleaned up when dropped.
pub fn create_temp_workspace(name: &str, version_req: &str) -> Result<TempDir> {
    let tmp = TempDir::new().context("Failed to create temp directory")?;
    write_workspace_files(tmp.path(), name, version_req, None)?;
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

    #[test]
    fn sanitize_spec_basic() {
        assert_eq!(sanitize_spec("serde"), "serde");
        assert_eq!(sanitize_spec("tokio@1"), "tokio@1");
        assert_eq!(sanitize_spec("serde@1.0.200"), "serde@1.0.200");
    }

    #[test]
    fn cache_dir_env_override() {
        let original = std::env::var("CARGO_BRIEF_CACHE_DIR").ok();
        // SAFETY: test-only env manipulation, tests run serially for this var
        unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", "/tmp/test-cache") };
        assert_eq!(cache_dir(), PathBuf::from("/tmp/test-cache"));
        match original {
            Some(v) => unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", v) },
            None => unsafe { std::env::remove_var("CARGO_BRIEF_CACHE_DIR") },
        }
    }

    #[test]
    fn write_workspace_with_features() {
        let tmp = tempfile::tempdir().unwrap();
        write_workspace_files(tmp.path(), "tokio", "1", Some("rt,net,macros")).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap();
        assert!(content.contains("features"));
        assert!(content.contains("\"rt\""));
        assert!(content.contains("\"net\""));
        assert!(content.contains("\"macros\""));
    }

    #[test]
    fn resolve_workspace_no_cache() {
        let ws = resolve_workspace("serde", None, true).unwrap();
        assert!(matches!(ws, WorkspaceDir::Temp(_)));
        assert!(ws.path().join("Cargo.toml").exists());
        assert!(ws.path().join("src/lib.rs").exists());
    }

    #[test]
    fn resolve_workspace_cached() {
        let test_dir = tempfile::tempdir().unwrap();
        let original = std::env::var("CARGO_BRIEF_CACHE_DIR").ok();
        // SAFETY: test-only env manipulation, tests run serially for this var
        unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", test_dir.path()) };

        let ws = resolve_workspace("serde", None, false).unwrap();
        assert!(matches!(ws, WorkspaceDir::Cached(_)));
        assert!(ws.path().join("Cargo.toml").exists());
        assert!(ws.path().join("src/lib.rs").exists());

        // Second call reuses the same directory (idempotent)
        let ws2 = resolve_workspace("serde", None, false).unwrap();
        assert_eq!(ws.path(), ws2.path());

        match original {
            Some(v) => unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", v) },
            None => unsafe { std::env::remove_var("CARGO_BRIEF_CACHE_DIR") },
        }
    }
}
