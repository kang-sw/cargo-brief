use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Invoke `cargo +nightly rustdoc` and return the path to the generated JSON file.
pub fn generate_rustdoc_json(
    crate_name: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    document_private_items: bool,
) -> Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg(format!("+{toolchain}"));
    cmd.args(["rustdoc", "-p", crate_name]);

    if let Some(manifest) = manifest_path {
        cmd.args(["--manifest-path", manifest]);
    }

    cmd.arg("--");
    cmd.args(["--output-format", "json", "-Z", "unstable-options"]);

    if document_private_items {
        cmd.arg("--document-private-items");
    }

    let output = cmd.output().with_context(|| {
        format!(
            "Failed to execute `cargo +{toolchain} rustdoc`. \
             Is the '{toolchain}' toolchain installed? Try: rustup toolchain install {toolchain}"
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("toolchain") && stderr.contains("is not installed") {
            bail!(
                "The '{toolchain}' toolchain is not installed.\n\
                 Install it with: rustup toolchain install {toolchain}"
            );
        }
        if stderr.contains("did not match any packages")
            || stderr.contains("package(s) `")
            || stderr.contains("no packages match")
        {
            bail!(
                "Package '{crate_name}' not found in the workspace.\n\
                 Check the package name and ensure it exists in the workspace.\n\
                 Original error:\n{stderr}"
            );
        }
        bail!("cargo rustdoc failed:\n{stderr}");
    }

    // Find the generated JSON file in the target directory
    let target_dir = find_target_dir(manifest_path)?;
    let json_name = crate_name.replace('-', "_");
    let json_path = target_dir.join("doc").join(format!("{json_name}.json"));

    if !json_path.exists() {
        bail!(
            "Expected rustdoc JSON at {} but file not found",
            json_path.display()
        );
    }

    Ok(json_path)
}

/// Parse a rustdoc JSON file into the `rustdoc_types::Crate` structure.
pub fn parse_rustdoc_json(path: &Path) -> Result<rustdoc_types::Crate> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let krate: rustdoc_types::Crate =
        serde_json::from_str(&content).context("Failed to parse rustdoc JSON")?;
    Ok(krate)
}

/// Determine the cargo target directory.
fn find_target_dir(manifest_path: Option<&str>) -> Result<PathBuf> {
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

    Ok(PathBuf::from(target_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_target_dir() {
        let target = find_target_dir(None).unwrap();
        assert!(target.ends_with("target") || target.to_string_lossy().contains("target"));
    }
}
