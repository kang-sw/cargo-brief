use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Metadata extracted from `cargo metadata`.
pub struct CargoMetadataInfo {
    /// Names of all packages in the workspace.
    pub workspace_packages: Vec<String>,
    /// The package whose manifest_path directory matches cwd, if any.
    pub current_package: Option<String>,
    /// The manifest directory of the current package (for file path resolution).
    pub current_package_manifest_dir: Option<PathBuf>,
    /// The target directory for build artifacts.
    pub target_dir: PathBuf,
}

/// A resolved target for the pipeline.
#[derive(Debug)]
pub struct ResolvedTarget {
    pub package_name: String,
    pub module_path: Option<String>,
}

/// Load cargo metadata for the workspace.
pub fn load_cargo_metadata(manifest_path: Option<&str>) -> Result<CargoMetadataInfo> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version=1", "--no-deps"]);

    if let Some(manifest) = manifest_path {
        cmd.args(["--manifest-path", manifest]);
    }

    let output = cmd.output().context("Failed to run cargo metadata")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo metadata failed:\n{stderr}");
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse cargo metadata")?;

    let target_dir = metadata["target_directory"]
        .as_str()
        .context("No target_directory in cargo metadata")?;

    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let cwd_canonical = cwd.canonicalize().unwrap_or(cwd);

    let mut workspace_packages = Vec::new();
    let mut current_package = None;
    let mut current_package_manifest_dir = None;

    if let Some(packages) = metadata["packages"].as_array() {
        for pkg in packages {
            if let Some(name) = pkg["name"].as_str() {
                workspace_packages.push(name.to_string());

                // Check if this package's manifest directory matches cwd
                if let Some(manifest) = pkg["manifest_path"].as_str() {
                    let manifest_dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
                    let manifest_canonical = manifest_dir
                        .canonicalize()
                        .unwrap_or(manifest_dir.to_path_buf());
                    if manifest_canonical == cwd_canonical {
                        current_package = Some(name.to_string());
                        current_package_manifest_dir = Some(manifest_canonical);
                    }
                }
            }
        }
    }

    Ok(CargoMetadataInfo {
        workspace_packages,
        current_package,
        current_package_manifest_dir,
        target_dir: PathBuf::from(target_dir),
    })
}

/// Resolve the user's positional arguments into a package name and optional module path.
///
/// Resolution rules:
/// 1. `first_arg == "self"` → current package; second_arg becomes module (file path aware)
/// 2. `first_arg` contains `::` → split at first `::`:
///    - prefix `self` → current package + rest as module
///    - otherwise → prefix is package name, rest is module
/// 3. Two args → backward compat: first=package, second=module (file path aware)
/// 4. Single arg, no `::` → if file path, resolve to self module; else check workspace packages; else treat as package
pub fn resolve_target(
    first_arg: &str,
    second_arg: Option<&str>,
    metadata: &CargoMetadataInfo,
) -> Result<ResolvedTarget> {
    // Case 1: explicit "self" keyword
    if first_arg == "self" {
        let pkg = current_package_or_error(metadata)?;
        let module = match second_arg {
            Some(m) => maybe_file_to_module(strip_self_prefix(m), metadata)?,
            None => None,
        };
        return Ok(ResolvedTarget {
            package_name: pkg,
            module_path: module,
        });
    }

    // Case 2: contains "::" — split at first occurrence
    if let Some(idx) = first_arg.find("::") {
        let prefix = &first_arg[..idx];
        let rest = &first_arg[idx + 2..];
        let module = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };

        if prefix == "self" {
            let pkg = current_package_or_error(metadata)?;
            return Ok(ResolvedTarget {
                package_name: pkg,
                module_path: module,
            });
        } else {
            return Ok(ResolvedTarget {
                package_name: prefix.to_string(),
                module_path: module,
            });
        }
    }

    // Case 3: two args provided → backward compat (file path aware on module arg)
    if let Some(module) = second_arg {
        let module = maybe_file_to_module(module, metadata)?;
        return Ok(ResolvedTarget {
            package_name: first_arg.to_string(),
            module_path: module,
        });
    }

    // Case 4a: single arg that looks like a file path → resolve to self module
    if is_file_path(first_arg) {
        let pkg = current_package_or_error(metadata)?;
        let module = file_path_to_module_path(first_arg, metadata)?;
        return Ok(ResolvedTarget {
            package_name: pkg,
            module_path: module,
        });
    }

    // Case 4b: single arg, no "::" → try workspace package first, then self module
    if let Some(pkg) = find_workspace_package(&metadata.workspace_packages, first_arg) {
        return Ok(ResolvedTarget {
            package_name: pkg,
            module_path: None,
        });
    }

    // Not a known workspace package → treat as external package name.
    // Users should use `self::module` or file paths for self-module access.
    Ok(ResolvedTarget {
        package_name: first_arg.to_string(),
        module_path: None,
    })
}

/// If the input looks like a file path, convert it to a module path; otherwise return as-is.
fn maybe_file_to_module(input: &str, metadata: &CargoMetadataInfo) -> Result<Option<String>> {
    if is_file_path(input) {
        file_path_to_module_path(input, metadata)
    } else if input.is_empty() {
        Ok(None)
    } else {
        Ok(Some(input.to_string()))
    }
}

/// Detect whether a string looks like a file path rather than a module path.
/// File paths contain `/` or end with `.rs`; module paths use `::` separators.
fn is_file_path(s: &str) -> bool {
    s.contains('/') || s.ends_with(".rs")
}

/// Convert a file path to a module path.
///
/// Fallback order:
/// 1. Try as cwd-relative path
/// 2. Try as relative to the current package's `src/` directory
///
/// Then strip the `src/` prefix and convert: `.rs` → remove, `mod.rs` → parent,
/// `lib.rs` → None (crate root), `/` → `::`.
fn file_path_to_module_path(input: &str, metadata: &CargoMetadataInfo) -> Result<Option<String>> {
    let input_path = Path::new(input);

    // Try to resolve the file path
    let resolved = if input_path.is_file() {
        // cwd-relative path exists
        input_path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize path: {input}"))?
    } else if let Some(pkg_dir) = &metadata.current_package_manifest_dir {
        // Try relative to package's src/
        let src_relative = pkg_dir.join("src").join(input);
        if src_relative.is_file() {
            src_relative.canonicalize()?
        } else {
            // Try relative to package root
            let pkg_relative = pkg_dir.join(input);
            if pkg_relative.is_file() {
                pkg_relative.canonicalize()?
            } else {
                bail!(
                    "File not found: '{input}'\n\
                     Searched:\n  - ./{input}\n  - {}/src/{input}\n  - {}/{input}",
                    pkg_dir.display(),
                    pkg_dir.display()
                );
            }
        }
    } else {
        bail!(
            "File not found: '{input}'\n\
             (No current package directory for fallback search.)"
        );
    };

    // Find the package's src/ directory and make path relative to it
    let pkg_dir = metadata
        .current_package_manifest_dir
        .as_ref()
        .context("Cannot resolve file path without a current package")?;
    let src_dir = pkg_dir.join("src");
    let src_canonical = src_dir
        .canonicalize()
        .with_context(|| format!("Package src/ directory not found: {}", src_dir.display()))?;

    let relative = resolved.strip_prefix(&src_canonical).with_context(|| {
        format!(
            "File '{}' is not inside the package's src/ directory ({})",
            resolved.display(),
            src_canonical.display()
        )
    })?;

    path_components_to_module(relative)
}

/// Convert a path relative to `src/` into a module path.
fn path_components_to_module(relative: &Path) -> Result<Option<String>> {
    let file_name = relative
        .file_name()
        .and_then(|f| f.to_str())
        .context("Invalid file name")?;

    // lib.rs at root → crate root (no module path)
    if file_name == "lib.rs" && relative.parent().map_or(true, |p| p == Path::new("")) {
        return Ok(None);
    }

    let stem = relative
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid file stem")?;

    let mut parts: Vec<&str> = Vec::new();

    // Collect directory components
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(s) = component {
                parts.push(s.to_str().context("Non-UTF8 path component")?);
            }
        }
    }

    // mod.rs → use only parent directory path; other files → append stem
    if stem != "mod" {
        parts.push(stem);
    }

    if parts.is_empty() {
        // mod.rs at src root → crate root
        Ok(None)
    } else {
        Ok(Some(parts.join("::")))
    }
}

/// Strip a leading `self::` prefix if present.
fn strip_self_prefix(s: &str) -> &str {
    s.strip_prefix("self::").unwrap_or(s)
}

/// Get the current package or return a descriptive error.
fn current_package_or_error(metadata: &CargoMetadataInfo) -> Result<String> {
    metadata.current_package.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot resolve 'self': no package found for the current directory.\n\
             Are you in a package directory? (Virtual workspace roots have no package.)"
        )
    })
}

/// Find a package in the workspace, normalizing hyphens/underscores.
fn find_workspace_package(packages: &[String], query: &str) -> Option<String> {
    let normalized = query.replace('-', "_");
    packages
        .iter()
        .find(|p| p.replace('-', "_") == normalized)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_metadata(packages: &[&str], current: Option<&str>) -> CargoMetadataInfo {
        CargoMetadataInfo {
            workspace_packages: packages.iter().map(|s| s.to_string()).collect(),
            current_package: current.map(|s| s.to_string()),
            current_package_manifest_dir: None,
            target_dir: PathBuf::from("/tmp/target"),
        }
    }

    fn test_metadata_with_dir(
        packages: &[&str],
        current: Option<&str>,
        manifest_dir: &Path,
    ) -> CargoMetadataInfo {
        CargoMetadataInfo {
            workspace_packages: packages.iter().map(|s| s.to_string()).collect(),
            current_package: current.map(|s| s.to_string()),
            current_package_manifest_dir: Some(manifest_dir.to_path_buf()),
            target_dir: PathBuf::from("/tmp/target"),
        }
    }

    #[test]
    fn test_self_keyword_no_module() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("self", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_self_keyword_with_module() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("self", Some("foo::bar"), &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, Some("foo::bar".to_string()));
    }

    #[test]
    fn test_self_keyword_strips_self_prefix_in_module() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("self", Some("self::foo"), &meta).unwrap();
        assert_eq!(resolved.module_path, Some("foo".to_string()));
    }

    #[test]
    fn test_self_no_current_package_errors() {
        let meta = test_metadata(&["other"], None);
        let result = resolve_target("self", None, &meta);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("self"));
    }

    #[test]
    fn test_double_colon_self_prefix() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("self::cli", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, Some("cli".to_string()));
    }

    #[test]
    fn test_double_colon_package_prefix() {
        let meta = test_metadata(&["hecs", "my-crate"], Some("my-crate"));
        let resolved = resolve_target("hecs::world", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "hecs");
        assert_eq!(resolved.module_path, Some("world".to_string()));
    }

    #[test]
    fn test_double_colon_trailing_empty() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("self::", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_two_args_backward_compat() {
        let meta = test_metadata(&["hecs"], Some("my-crate"));
        let resolved = resolve_target("hecs", Some("world"), &meta).unwrap();
        assert_eq!(resolved.package_name, "hecs");
        assert_eq!(resolved.module_path, Some("world".to_string()));
    }

    #[test]
    fn test_single_arg_known_package() {
        let meta = test_metadata(&["hecs", "my-crate"], Some("my-crate"));
        let resolved = resolve_target("hecs", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "hecs");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_single_arg_unknown_resolves_as_package() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("cli", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "cli");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_single_arg_no_current_package_assumes_external() {
        let meta = test_metadata(&[], None);
        let resolved = resolve_target("hecs", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "hecs");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_hyphen_underscore_normalization() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("my_crate", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, None);
    }

    // === File path detection ===

    #[test]
    fn test_is_file_path_with_slash() {
        assert!(is_file_path("src/cli.rs"));
        assert!(is_file_path("foo/bar"));
    }

    #[test]
    fn test_is_file_path_with_rs_extension() {
        assert!(is_file_path("cli.rs"));
        assert!(is_file_path("model.rs"));
    }

    #[test]
    fn test_is_file_path_not_module() {
        assert!(!is_file_path("cli"));
        assert!(!is_file_path("foo::bar"));
    }

    // === path_components_to_module unit tests ===

    #[test]
    fn test_path_to_module_simple_file() {
        let result = path_components_to_module(Path::new("cli.rs")).unwrap();
        assert_eq!(result, Some("cli".to_string()));
    }

    #[test]
    fn test_path_to_module_nested_file() {
        let result = path_components_to_module(Path::new("model/item.rs")).unwrap();
        assert_eq!(result, Some("model::item".to_string()));
    }

    #[test]
    fn test_path_to_module_mod_rs() {
        let result = path_components_to_module(Path::new("model/mod.rs")).unwrap();
        assert_eq!(result, Some("model".to_string()));
    }

    #[test]
    fn test_path_to_module_lib_rs() {
        let result = path_components_to_module(Path::new("lib.rs")).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_path_to_module_deeply_nested() {
        let result = path_components_to_module(Path::new("a/b/c.rs")).unwrap();
        assert_eq!(result, Some("a::b::c".to_string()));
    }

    // === File path in resolve_target (integration with real filesystem) ===

    #[test]
    fn test_single_arg_file_path_cwd_relative() {
        // src/cli.rs exists relative to cwd (the project root)
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let resolved = resolve_target("src/cli.rs", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "cargo-brief");
        assert_eq!(resolved.module_path, Some("cli".to_string()));
    }

    #[test]
    fn test_single_arg_file_path_lib_rs() {
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let resolved = resolve_target("src/lib.rs", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "cargo-brief");
        assert_eq!(resolved.module_path, None);
    }

    #[test]
    fn test_self_with_file_path_module() {
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let resolved = resolve_target("self", Some("src/resolve.rs"), &meta).unwrap();
        assert_eq!(resolved.package_name, "cargo-brief");
        assert_eq!(resolved.module_path, Some("resolve".to_string()));
    }

    #[test]
    fn test_two_args_file_path_module() {
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let resolved = resolve_target("cargo-brief", Some("src/cli.rs"), &meta).unwrap();
        assert_eq!(resolved.package_name, "cargo-brief");
        assert_eq!(resolved.module_path, Some("cli".to_string()));
    }

    #[test]
    fn test_file_path_src_fallback() {
        // "cli.rs" doesn't exist at cwd, but does at src/cli.rs
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let resolved = resolve_target("cli.rs", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "cargo-brief");
        assert_eq!(resolved.module_path, Some("cli".to_string()));
    }

    #[test]
    fn test_file_path_not_found_errors() {
        let cwd = std::env::current_dir().unwrap();
        let meta = test_metadata_with_dir(&["cargo-brief"], Some("cargo-brief"), &cwd);
        let result = resolve_target("nonexistent.rs", None, &meta);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
