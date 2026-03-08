use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Metadata extracted from `cargo metadata`.
pub struct CargoMetadataInfo {
    /// Names of all packages in the workspace.
    pub workspace_packages: Vec<String>,
    /// The package whose manifest_path directory matches cwd, if any.
    pub current_package: Option<String>,
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
                    }
                }
            }
        }
    }

    Ok(CargoMetadataInfo {
        workspace_packages,
        current_package,
        target_dir: PathBuf::from(target_dir),
    })
}

/// Resolve the user's positional arguments into a package name and optional module path.
///
/// Resolution rules:
/// 1. `first_arg == "self"` → current package; second_arg becomes module
/// 2. `first_arg` contains `::` → split at first `::`:
///    - prefix `self` → current package + rest as module
///    - otherwise → prefix is package name, rest is module
/// 3. Two args → backward compat: first=package, second=module
/// 4. Single arg, no `::` → check workspace packages (with normalization); if not found, treat as self module
pub fn resolve_target(
    first_arg: &str,
    second_arg: Option<&str>,
    metadata: &CargoMetadataInfo,
) -> Result<ResolvedTarget> {
    // Case 1: explicit "self" keyword
    if first_arg == "self" {
        let pkg = current_package_or_error(metadata)?;
        let module = second_arg.map(|m| strip_self_prefix(m).to_string());
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

    // Case 3: two args provided → backward compat
    if let Some(module) = second_arg {
        return Ok(ResolvedTarget {
            package_name: first_arg.to_string(),
            module_path: Some(module.to_string()),
        });
    }

    // Case 4: single arg, no "::" → try workspace package first, then self module
    if let Some(pkg) = find_workspace_package(&metadata.workspace_packages, first_arg) {
        return Ok(ResolvedTarget {
            package_name: pkg,
            module_path: None,
        });
    }

    // Not a known package → treat as module of self
    let pkg = current_package_or_error(metadata)?;
    Ok(ResolvedTarget {
        package_name: pkg,
        module_path: Some(first_arg.to_string()),
    })
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
    fn test_single_arg_unknown_falls_back_to_self_module() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("cli", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, Some("cli".to_string()));
    }

    #[test]
    fn test_single_arg_fallback_no_current_package_errors() {
        let meta = test_metadata(&[], None);
        let result = resolve_target("cli", None, &meta);
        assert!(result.is_err());
    }

    #[test]
    fn test_hyphen_underscore_normalization() {
        let meta = test_metadata(&["my-crate"], Some("my-crate"));
        let resolved = resolve_target("my_crate", None, &meta).unwrap();
        assert_eq!(resolved.package_name, "my-crate");
        assert_eq!(resolved.module_path, None);
    }
}
