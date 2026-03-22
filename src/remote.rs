use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
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

/// Build a version-normalized cache directory name.
///
/// Format: `name[version]` or `name[version]+feat1+feat2` (features alpha-sorted).
pub fn cache_dir_name(name: &str, version: &str, features: Option<&str>) -> String {
    let mut result = format!("{name}[{version}]");
    if let Some(feats) = features {
        let mut feat_list: Vec<&str> = feats.split(',').map(|s| s.trim()).collect();
        feat_list.sort();
        for f in &feat_list {
            result.push('+');
            result.push_str(f);
        }
    }
    result
}

/// Find the newest non-yanked version matching `version_req` from a crates.io API JSON response.
///
/// The `versions` array in the response is assumed to be newest-first.
pub fn find_matching_version(json_str: &str, version_req: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let versions = parsed.get("versions")?.as_array()?;
    let req = semver::VersionReq::parse(version_req).ok()?;

    for entry in versions {
        let yanked = entry
            .get("yanked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if yanked {
            continue;
        }
        let num = entry.get("num")?.as_str()?;
        if let Ok(ver) = semver::Version::parse(num)
            && req.matches(&ver)
        {
            return Some(num.to_string());
        }
    }
    None
}

/// Resolve the exact version of a crate from crates.io, with 24h file cache.
///
/// - Exact specs (starting with `=`) skip the API call entirely.
/// - Cached API responses at `CACHE_DIR/versions/{name}.json` are reused if < 24h old.
/// - On API failure, stale cache is used with a stderr warning.
/// - If no cache exists and network fails, returns an error.
pub fn fetch_resolved_version(name: &str, version_req: &str) -> Result<String> {
    // Exact spec shortcut: =1.0.200 → 1.0.200 (no network needed)
    if let Some(exact) = version_req.strip_prefix('=') {
        return Ok(exact.to_string());
    }

    let cache_path = cache_dir().join("versions").join(format!("{name}.json"));

    // Try fresh cache first (< 24h)
    if let Some(cached) = read_version_cache(&cache_path, false)
        && let Some(ver) = find_matching_version(&cached, version_req)
    {
        return Ok(ver);
    }

    // API call
    match fetch_crates_io_api(name) {
        Ok(resp) => {
            // Write cache (best-effort)
            let _ = std::fs::create_dir_all(cache_path.parent().unwrap());
            let _ = std::fs::write(&cache_path, &resp);

            find_matching_version(&resp, version_req).with_context(|| {
                format!("No version of '{name}' matches requirement '{version_req}'")
            })
        }
        Err(api_err) => {
            // Offline fallback: use stale cache if available
            if let Some(stale) = read_version_cache(&cache_path, true)
                && let Some(ver) = find_matching_version(&stale, version_req)
            {
                eprintln!(
                    "Warning: using stale version cache for '{name}' (API unavailable: {api_err})"
                );
                return Ok(ver);
            }
            bail!(
                "Cannot resolve version for '{name}': {api_err}. \
                 Try specifying an exact version (e.g., {name}@1.0.0) or check your internet connection."
            )
        }
    }
}

/// Read version cache file, optionally ignoring staleness.
fn read_version_cache(path: &Path, allow_stale: bool) -> Option<String> {
    let meta = path.metadata().ok()?;
    if !allow_stale {
        let age = meta.modified().ok()?.elapsed().ok()?;
        if age > Duration::from_secs(86400) {
            return None;
        }
    }
    std::fs::read_to_string(path).ok()
}

/// Call the crates.io REST API for crate metadata.
fn fetch_crates_io_api(name: &str) -> Result<String> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let resp = ureq::get(&url)
        .set(
            "User-Agent",
            "cargo-brief (https://github.com/kang-sw/cargo-brief)",
        )
        .call()
        .with_context(|| format!("HTTP request to crates.io failed for '{name}'"))?
        .into_string()
        .context("Failed to read crates.io response body")?;
    Ok(resp)
}

/// Write the workspace Cargo.toml and src/lib.rs into the given directory.
fn write_workspace_files(
    dir: &Path,
    name: &str,
    version_req: &str,
    features: Option<&str>,
    no_default_features: bool,
) -> Result<()> {
    let needs_table = features.is_some() || no_default_features;
    let dep_value = if needs_table {
        let mut parts = vec![format!("version = \"{version_req}\"")];
        if no_default_features {
            parts.push("default-features = false".to_string());
        }
        if let Some(f) = features {
            let feat_str = f
                .split(',')
                .map(|s| format!("\"{}\"", s.trim()))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("features = [{feat_str}]"));
        }
        format!("{{ {} }}", parts.join(", "))
    } else {
        format!("\"{version_req}\"")
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
/// Returns `(WorkspaceDir, Option<resolved_version>)`.
/// When `no_cache` is true, returns a `TempDir` (version resolution is best-effort).
/// Otherwise, resolves the exact version via crates.io API and uses a normalized cache dir.
pub fn resolve_workspace(
    spec: &str,
    features: Option<&str>,
    no_default_features: bool,
    no_cache: bool,
) -> Result<(WorkspaceDir, Option<String>)> {
    let (name, version_req) = parse_crate_spec(spec);

    if no_cache {
        // Best-effort version resolution for --no-cache (used for header display)
        let resolved = fetch_resolved_version(&name, &version_req).ok();
        let actual_req = resolved
            .as_deref()
            .map(|v| format!("={v}"))
            .unwrap_or(version_req);
        let tmp = TempDir::new().context("Failed to create temp directory")?;
        write_workspace_files(
            tmp.path(),
            &name,
            &actual_req,
            features,
            no_default_features,
        )?;
        return Ok((WorkspaceDir::Temp(tmp), resolved));
    }

    let resolved = fetch_resolved_version(&name, &version_req)?;
    let dir_name = cache_dir_name(&name, &resolved, features);
    let dir = cache_dir().join(dir_name);

    if !dir.join("Cargo.toml").exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;
        write_workspace_files(
            &dir,
            &name,
            &format!("={resolved}"),
            features,
            no_default_features,
        )?;
    }

    Ok((WorkspaceDir::Cached(dir), Some(resolved.clone())))
}

/// Read the resolved version of a crate from Cargo.lock in a workspace directory.
///
/// Parses the lockfile with a simple line-based scanner (no TOML dep needed).
/// Returns None if the crate is not found or the lockfile doesn't exist.
pub fn resolve_crate_version(workspace_dir: &Path, crate_name: &str) -> Option<String> {
    let lockfile = workspace_dir.join("Cargo.lock");
    let content = std::fs::read_to_string(lockfile).ok()?;

    let mut in_target_package = false;
    for line in content.lines() {
        if line.starts_with("[[package]]") {
            in_target_package = false;
            continue;
        }
        if in_target_package {
            if let Some(version) = line.strip_prefix("version = \"") {
                return version.strip_suffix('"').map(|v| v.to_string());
            }
        }
        // Match name = "<crate_name>" (underscore-normalized)
        if let Some(name) = line.strip_prefix("name = \"") {
            if let Some(name) = name.strip_suffix('"') {
                if name == crate_name || name.replace('-', "_") == crate_name.replace('-', "_") {
                    in_target_package = true;
                }
            }
        }
    }

    None
}

/// Clean cached workspace(s). Empty spec = all, otherwise glob-match on crate name prefix.
pub fn clean_cache(spec: &str) -> Result<()> {
    if spec.is_empty() {
        let dir = cache_dir();
        if dir.exists() {
            let size = dir_size(&dir);
            std::fs::remove_dir_all(&dir)?;
            eprintln!("Removed {} ({} MB)", dir.display(), size / 1_000_000);
        } else {
            eprintln!("No cache found at {}", dir.display());
        }
        return Ok(());
    }

    let (name, _) = parse_crate_spec(spec);
    let base = cache_dir();
    let prefix = format!("{name}[");
    let mut found = false;

    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(&prefix) && entry.path().is_dir() {
                let size = dir_size(&entry.path());
                std::fs::remove_dir_all(entry.path())?;
                eprintln!(
                    "Removed {} ({} MB)",
                    entry.path().display(),
                    size / 1_000_000
                );
                found = true;
            }
        }
    }

    // Also remove version cache
    let version_cache = base.join("versions").join(format!("{name}.json"));
    if version_cache.exists() {
        std::fs::remove_file(&version_cache)?;
        eprintln!("Removed version cache for '{name}'");
        found = true;
    }

    if !found {
        eprintln!("No cache found for '{name}'");
    }

    Ok(())
}

/// Recursively compute directory size in bytes.
fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| {
            let meta = e.metadata().ok();
            if e.path().is_dir() {
                dir_size(&e.path())
            } else {
                meta.map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Create a temporary workspace with the given crate as a dependency.
/// Returns `TempDir` — the workspace is cleaned up when dropped.
pub fn create_temp_workspace(name: &str, version_req: &str) -> Result<TempDir> {
    let tmp = TempDir::new().context("Failed to create temp directory")?;
    write_workspace_files(tmp.path(), name, version_req, None, false)?;
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

    /// Guard that serializes tests manipulating CARGO_BRIEF_CACHE_DIR.
    /// Prevents parallel tests from stomping each other's env var.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Set CARGO_BRIEF_CACHE_DIR for the duration of a closure, restoring afterwards.
    fn with_cache_dir<F: FnOnce()>(path: &std::path::Path, f: F) {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("CARGO_BRIEF_CACHE_DIR").ok();
        unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", path) };
        f();
        match original {
            Some(v) => unsafe { std::env::set_var("CARGO_BRIEF_CACHE_DIR", v) },
            None => unsafe { std::env::remove_var("CARGO_BRIEF_CACHE_DIR") },
        }
    }

    #[test]
    fn cache_dir_env_override() {
        let tmp = PathBuf::from("/tmp/test-cache");
        with_cache_dir(&tmp, || {
            assert_eq!(cache_dir(), tmp);
        });
    }

    #[test]
    fn write_workspace_with_features() {
        let tmp = tempfile::tempdir().unwrap();
        write_workspace_files(tmp.path(), "tokio", "1", Some("rt,net,macros"), false).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap();
        assert!(content.contains("features"));
        assert!(content.contains("\"rt\""));
        assert!(content.contains("\"net\""));
        assert!(content.contains("\"macros\""));
    }

    #[test]
    fn resolve_workspace_no_cache() {
        let (ws, _resolved) = resolve_workspace("serde", None, false, true).unwrap();
        assert!(matches!(ws, WorkspaceDir::Temp(_)));
        assert!(ws.path().join("Cargo.toml").exists());
        assert!(ws.path().join("src/lib.rs").exists());
    }

    #[test]
    fn resolve_workspace_cached() {
        let test_dir = tempfile::tempdir().unwrap();
        with_cache_dir(test_dir.path(), || {
            let (ws, resolved) = resolve_workspace("serde@1.0.200", None, false, false).unwrap();
            assert!(matches!(ws, WorkspaceDir::Cached(_)));
            assert!(ws.path().join("Cargo.toml").exists());
            assert!(ws.path().join("src/lib.rs").exists());
            assert_eq!(resolved, Some("1.0.200".to_string()));

            // Verify dir name uses normalized format
            let dir_name = ws.path().file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(dir_name, "serde[1.0.200]");

            // Second call reuses the same directory (idempotent)
            let (ws2, _) = resolve_workspace("serde@1.0.200", None, false, false).unwrap();
            assert_eq!(ws.path(), ws2.path());
        });
    }

    #[test]
    fn test_cache_dir_name() {
        assert_eq!(cache_dir_name("hecs", "0.14.2", None), "hecs[0.14.2]");
        assert_eq!(cache_dir_name("serde", "1.0.200", None), "serde[1.0.200]");
    }

    #[test]
    fn test_cache_dir_name_feature_sorting() {
        assert_eq!(
            cache_dir_name("bevy", "0.18.1", Some("bevy_winit,default")),
            "bevy[0.18.1]+bevy_winit+default"
        );
        // Features are alpha-sorted
        assert_eq!(
            cache_dir_name("tokio", "1.44.1", Some("net,rt,macros")),
            "tokio[1.44.1]+macros+net+rt"
        );
    }

    #[test]
    fn test_find_matching_version_exact() {
        let json = r#"{"versions": [
            {"num": "1.0.3", "yanked": false},
            {"num": "1.0.2", "yanked": false},
            {"num": "1.0.1", "yanked": false}
        ]}"#;
        assert_eq!(
            find_matching_version(json, "=1.0.2"),
            Some("1.0.2".to_string())
        );
    }

    #[test]
    fn test_find_matching_version_range() {
        let json = r#"{"versions": [
            {"num": "2.0.0", "yanked": false},
            {"num": "1.5.0", "yanked": false},
            {"num": "1.4.0", "yanked": false},
            {"num": "0.9.0", "yanked": false}
        ]}"#;
        // "1" matches >=1.0.0, <2.0.0 → newest is 1.5.0
        assert_eq!(find_matching_version(json, "1"), Some("1.5.0".to_string()));
    }

    #[test]
    fn test_find_matching_version_wildcard() {
        let json = r#"{"versions": [
            {"num": "3.0.0", "yanked": false},
            {"num": "2.0.0", "yanked": false}
        ]}"#;
        assert_eq!(find_matching_version(json, "*"), Some("3.0.0".to_string()));
    }

    #[test]
    fn test_find_matching_version_skips_yanked() {
        let json = r#"{"versions": [
            {"num": "1.2.0", "yanked": true},
            {"num": "1.1.0", "yanked": false}
        ]}"#;
        assert_eq!(find_matching_version(json, "*"), Some("1.1.0".to_string()));
    }

    #[test]
    fn test_find_matching_version_no_match() {
        let json = r#"{"versions": [
            {"num": "0.9.0", "yanked": false}
        ]}"#;
        assert_eq!(find_matching_version(json, "1"), None);
    }

    #[test]
    fn test_find_matching_version_minor_range() {
        let json = r#"{"versions": [
            {"num": "0.19.0", "yanked": false},
            {"num": "0.18.3", "yanked": false},
            {"num": "0.18.1", "yanked": false},
            {"num": "0.17.0", "yanked": false}
        ]}"#;
        // "0.18" matches >=0.18.0, <0.19.0
        assert_eq!(
            find_matching_version(json, "0.18"),
            Some("0.18.3".to_string())
        );
    }

    #[test]
    fn test_fetch_resolved_version_exact_shortcut() {
        // Exact specs skip the network entirely
        let ver = fetch_resolved_version("anything", "=1.2.3").unwrap();
        assert_eq!(ver, "1.2.3");
    }

    #[test]
    fn test_clean_cache_glob_matching() {
        let test_dir = tempfile::tempdir().unwrap();
        with_cache_dir(test_dir.path(), || {
            // Seed fake cache dirs
            std::fs::create_dir_all(test_dir.path().join("serde[1.0.200]")).unwrap();
            std::fs::create_dir_all(test_dir.path().join("serde[1.0.228]")).unwrap();
            std::fs::create_dir_all(test_dir.path().join("tokio[1.50.0]")).unwrap();

            // Seed version cache files
            let versions_dir = test_dir.path().join("versions");
            std::fs::create_dir_all(&versions_dir).unwrap();
            std::fs::write(versions_dir.join("serde.json"), "{}").unwrap();
            std::fs::write(versions_dir.join("tokio.json"), "{}").unwrap();

            clean_cache("serde").unwrap();

            // serde dirs and version cache removed
            assert!(!test_dir.path().join("serde[1.0.200]").exists());
            assert!(!test_dir.path().join("serde[1.0.228]").exists());
            assert!(!versions_dir.join("serde.json").exists());

            // tokio untouched
            assert!(test_dir.path().join("tokio[1.50.0]").exists());
            assert!(versions_dir.join("tokio.json").exists());
        });
    }

    #[test]
    fn test_clean_cache_empty_spec_removes_all() {
        let test_dir = tempfile::tempdir().unwrap();
        with_cache_dir(test_dir.path(), || {
            // Seed fake cache dirs
            std::fs::create_dir_all(test_dir.path().join("serde[1.0.200]")).unwrap();
            std::fs::create_dir_all(test_dir.path().join("tokio[1.50.0]")).unwrap();
            std::fs::create_dir_all(test_dir.path().join("versions")).unwrap();
            std::fs::write(test_dir.path().join("versions/serde.json"), "{}").unwrap();

            clean_cache("").unwrap();

            // Entire cache dir removed
            assert!(!test_dir.path().exists());
        });
    }
}
