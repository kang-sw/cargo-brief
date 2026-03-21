use std::collections::HashSet;
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
    use_cache: bool,
) -> Result<PathBuf> {
    if use_cache {
        let base_name = crate_name.split('@').next().unwrap_or(crate_name);
        let json_name = base_name.replace('-', "_");
        let json_path = target_dir.join("doc").join(format!("{json_name}.json"));
        if json_path.exists() {
            if verbose {
                eprintln!("[cargo-brief] Using cached rustdoc JSON for '{crate_name}'");
            }
            return Ok(json_path);
        }
    }

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

/// Parse Cargo.lock to extract all resolved package names.
///
/// Returns a set of hyphenated package names (as they appear in Cargo.lock).
/// Returns an empty set on any error (missing file, malformed content).
pub fn load_lockfile_packages(manifest_path: Option<&str>) -> HashSet<String> {
    let lockfile_path = if let Some(manifest) = manifest_path {
        Path::new(manifest)
            .parent()
            .map(|p| p.join("Cargo.lock"))
            .unwrap_or_else(|| PathBuf::from("Cargo.lock"))
    } else {
        PathBuf::from("Cargo.lock")
    };

    let content = match std::fs::read_to_string(&lockfile_path) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };

    let mut packages = HashSet::new();
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            in_package = true;
            continue;
        }
        if in_package && trimmed.starts_with("name = \"") {
            if let Some(name) = trimmed
                .strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
            {
                packages.insert(name.to_string());
            }
            in_package = false;
        } else if trimmed.starts_with('[') {
            in_package = false;
        }
    }

    packages
}

/// Batch-generate rustdoc JSON for multiple crates via single `cargo doc`.
///
/// Returns names that succeeded (cached or newly generated). Crates whose
/// JSON already exists are counted as cached successes without invoking cargo.
pub fn batch_generate_rustdoc_json(
    crate_names: &[&str],
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
) -> Vec<String> {
    let mut succeeded = Vec::new();
    let mut to_generate = Vec::new();

    for &name in crate_names {
        let json_name = name.replace('-', "_");
        let json_path = target_dir.join("doc").join(format!("{json_name}.json"));
        if json_path.exists() {
            succeeded.push(name.to_string());
        } else {
            to_generate.push(name);
        }
    }

    if to_generate.is_empty() {
        return succeeded;
    }

    if verbose {
        eprintln!(
            "[cargo-brief] Batch generating rustdoc JSON for {} crate(s): {}",
            to_generate.len(),
            to_generate.join(", ")
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.arg(format!("+{toolchain}"));
    cmd.args(["doc", "--no-deps", "--lib"]);

    for name in &to_generate {
        cmd.args(["-p", name]);
    }

    if let Some(manifest) = manifest_path {
        cmd.args(["--manifest-path", manifest]);
    }

    cmd.env(
        "RUSTDOCFLAGS",
        "--output-format json -Z unstable-options --document-private-items",
    );

    if verbose {
        cmd.stderr(Stdio::inherit());
        let status = cmd.status();
        if let Err(e) = &status {
            eprintln!("warning: batch cargo doc failed to execute: {e}");
            return succeeded;
        }
        // Even on non-zero exit, some crates may have succeeded — check below
    } else {
        let output = cmd.output();
        match output {
            Err(e) => {
                eprintln!("warning: batch cargo doc failed to execute: {e}");
                return succeeded;
            }
            Ok(o) if !o.status.success() => {
                // Non-fatal: some crates may still have generated JSON
            }
            Ok(_) => {}
        }
    }

    // Check which JSONs got created
    for name in &to_generate {
        let json_name = name.replace('-', "_");
        let json_path = target_dir.join("doc").join(format!("{json_name}.json"));
        if json_path.exists() {
            succeeded.push(name.to_string());
        } else if verbose {
            eprintln!("warning: batch cargo doc did not produce JSON for '{name}'");
        }
    }

    succeeded
}
