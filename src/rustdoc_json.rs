use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Invoke `cargo +nightly rustdoc` and return the path to the generated JSON file.
///
/// When `verbose` is true, cargo's stderr (compilation progress) is streamed to
/// the terminal in real time via `Stdio::inherit()`.
pub fn generate_rustdoc_json(
    crate_name: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    document_private_items: bool,
    target_dir: &Path,
    verbose: bool,
) -> Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg(format!("+{toolchain}"));
    cmd.args(["rustdoc", "-p", crate_name, "--lib"]);

    if let Some(manifest) = manifest_path {
        cmd.args(["--manifest-path", manifest]);
    }

    cmd.arg("--");
    cmd.args(["--output-format", "json", "-Z", "unstable-options"]);

    if document_private_items {
        cmd.arg("--document-private-items");
    }

    if verbose {
        // Stream cargo's stderr (Compiling/Checking progress) to terminal
        cmd.stderr(Stdio::inherit());
        let status = cmd.status().with_context(|| {
            format!(
                "Failed to execute `cargo +{toolchain} rustdoc`. \
                 Is the '{toolchain}' toolchain installed? Try: rustup toolchain install {toolchain}"
            )
        })?;
        if !status.success() {
            bail!("cargo rustdoc failed for '{crate_name}' (see output above)");
        }
    } else {
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
            if stderr.contains("is ambiguous") {
                let specs: Vec<&str> = stderr
                    .lines()
                    .filter_map(|l| {
                        let trimmed = l.trim();
                        if trimmed.contains('@') && !trimmed.contains(' ') {
                            Some(trimmed)
                        } else {
                            None
                        }
                    })
                    .collect();

                let suggestion = if specs.is_empty() {
                    format!(
                        "Multiple versions of '{crate_name}' exist. \
                         Use `<name>@<version>` to disambiguate (e.g. `{crate_name}@1.0.0`)."
                    )
                } else {
                    format!(
                        "Multiple versions of '{crate_name}' exist. \
                         Specify one of:\n  {}\n\
                         Example: cargo brief {}",
                        specs.join("\n  "),
                        specs[0],
                    )
                };

                bail!("{suggestion}");
            }
            if stderr.contains("did not match any packages")
                || stderr.contains("package(s) `")
                || stderr.contains("no packages match")
            {
                bail!(
                    "Package '{crate_name}' not found in the workspace.\n\
                     Check the package name and ensure it exists in the workspace.\n\
                     TIP: If it's an optional or unresolved dependency, try:\n\
                       cargo brief --crates {crate_name} --features <features>\n\
                     Original error:\n{stderr}"
                );
            }
            bail!("cargo rustdoc failed:\n{stderr}");
        }
    }

    // Find the generated JSON file in the target directory
    // Strip `@version` suffix — cargo uses it for disambiguation but the output file
    // is always named by the bare crate name.
    let base_name = crate_name.split('@').next().unwrap_or(crate_name);
    let json_name = base_name.replace('-', "_");
    let json_path = target_dir.join("doc").join(format!("{json_name}.json"));

    if !json_path.exists() {
        bail!(
            "Expected rustdoc JSON at {} but file not found",
            json_path.display()
        );
    }

    Ok(json_path)
}

/// Skip `cargo rustdoc` if the JSON already exists in `target/doc/`.
/// For remote (--crates) pipeline only — versions are locked so output is stable.
pub fn generate_rustdoc_json_cached(
    crate_name: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    document_private_items: bool,
    target_dir: &Path,
    verbose: bool,
) -> Result<PathBuf> {
    let base_name = crate_name.split('@').next().unwrap_or(crate_name);
    let json_name = base_name.replace('-', "_");
    let json_path = target_dir.join("doc").join(format!("{json_name}.json"));
    if json_path.exists() {
        if verbose {
            eprintln!("[cargo-brief] Using cached rustdoc JSON for '{crate_name}'");
        }
        return Ok(json_path);
    }
    generate_rustdoc_json(
        crate_name,
        toolchain,
        manifest_path,
        document_private_items,
        target_dir,
        verbose,
    )
}

/// Parse rustdoc JSON with bincode caching. If a `.bin` file exists and is
/// newer than the `.json`, deserialize from bincode (5-10x faster). Otherwise
/// parse JSON and write the `.bin` cache for next time.
pub fn parse_rustdoc_json_cached(path: &Path) -> Result<rustdoc_types::Crate> {
    let bin_path = path.with_extension("bin");

    if bin_path.exists()
        && let (Ok(bin_meta), Ok(json_meta)) = (bin_path.metadata(), path.metadata())
        && bin_meta.modified()? >= json_meta.modified()?
    {
        let bytes = std::fs::read(&bin_path)?;
        if let Ok(krate) = bincode::deserialize(&bytes) {
            return Ok(krate);
        }
        // Corrupted .bin — fall through to JSON parse
    }

    let krate = parse_rustdoc_json(path)?;

    // Best-effort write of bincode cache
    if let Ok(bytes) = bincode::serialize(&krate) {
        let _ = std::fs::write(&bin_path, bytes);
    }

    Ok(krate)
}

/// Parse a rustdoc JSON file into the `rustdoc_types::Crate` structure.
pub fn parse_rustdoc_json(path: &Path) -> Result<rustdoc_types::Crate> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let krate: rustdoc_types::Crate =
        serde_json::from_str(&content).context("Failed to parse rustdoc JSON")?;
    Ok(krate)
}
