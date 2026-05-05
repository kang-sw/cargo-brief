use std::collections::{HashMap, HashSet};
use std::env;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

/// Package names and versions from Cargo.lock.
///
/// Tracks all package names for validation, plus version lists for
/// disambiguating multi-version crates.
pub struct LockfilePackages {
    names: HashSet<String>,
    /// name -> sorted versions (ascending semver). Only populated when 2+ versions exist.
    multi_versions: HashMap<String, Vec<String>>,
}

impl LockfilePackages {
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Resolve a crate name to its cargo spec.
    ///
    /// Returns `Some("name@latest")` if multiple versions exist,
    /// `Some("name")` if single version, `None` if not found.
    /// Tries underscore->hyphen fallback internally.
    pub fn resolve_spec(&self, name: &str) -> Option<String> {
        if let Some(spec) = self.resolve_spec_exact(name) {
            return Some(spec);
        }
        let hyphenated = name.replace('_', "-");
        if hyphenated != name {
            return self.resolve_spec_exact(&hyphenated);
        }
        None
    }

    fn resolve_spec_exact(&self, name: &str) -> Option<String> {
        if !self.names.contains(name) {
            return None;
        }
        if let Some(versions) = self.multi_versions.get(name) {
            // versions are pre-sorted ascending by semver — last is highest
            let highest = versions.last().unwrap();
            Some(format!("{name}@{highest}"))
        } else {
            Some(name.to_string())
        }
    }
}

/// Guard: runs the toolchain check at most once per process.
static TOOLCHAIN_CHECKED: AtomicBool = AtomicBool::new(false);

static LIB_TARGET_NAME_CACHE: OnceLock<Mutex<HashMap<String, HashMap<String, String>>>> =
    OnceLock::new();

/// Pre-check that the required rustup toolchain is available.
///
/// Uses `rustup which rustdoc --toolchain {toolchain}` (~10ms) to detect.
/// When the toolchain is missing and stderr is a TTY, prompts the user to
/// install it interactively (reading from `/dev/tty` on Unix, `CONIN$` on
/// Windows). In non-TTY mode, bails with an actionable error message.
///
/// The check runs at most once per process via an `AtomicBool` guard.
fn ensure_toolchain_available(toolchain: &str) -> Result<()> {
    if TOOLCHAIN_CHECKED.load(Ordering::Relaxed) {
        return Ok(());
    }

    let result = Command::new("rustup")
        .args(["which", "rustdoc", "--toolchain", toolchain])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match result {
        Ok(status) if status.success() => {
            TOOLCHAIN_CHECKED.store(true, Ordering::Relaxed);
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "rustup is not installed. Install it from https://rustup.rs/ \
                 then run: rustup toolchain install {toolchain}"
            );
        }
        _ => {} // toolchain missing — fall through to prompt/error
    }

    if std::io::stderr().is_terminal() {
        eprintln!("[cargo-brief] The '{toolchain}' toolchain is required but not installed.");
        eprint!("[cargo-brief] Install it now? [y/N] ");

        let response = read_tty_line();
        if !response
            .as_ref()
            .is_ok_and(|s| matches!(s.trim(), "y" | "Y"))
        {
            bail!(
                "The '{toolchain}' toolchain is not installed.\n\
                 Install it with: rustup toolchain install {toolchain}"
            );
        }

        eprintln!("[cargo-brief] Installing '{toolchain}' toolchain...");
        let install_status = Command::new("rustup")
            .args(["toolchain", "install", toolchain])
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .status()
            .context("Failed to run `rustup toolchain install`")?;

        if !install_status.success() {
            bail!(
                "Failed to install the '{toolchain}' toolchain.\n\
                 Try manually: rustup toolchain install {toolchain}"
            );
        }

        TOOLCHAIN_CHECKED.store(true, Ordering::Relaxed);
        Ok(())
    } else {
        bail!(
            "The '{toolchain}' toolchain is not installed.\n\
             Install it with: rustup toolchain install {toolchain}"
        );
    }
}

/// Read a single line from the controlling terminal, bypassing stdin.
fn read_tty_line() -> std::io::Result<String> {
    #[cfg(unix)]
    const TTY_PATH: &str = "/dev/tty";
    #[cfg(windows)]
    const TTY_PATH: &str = "CONIN$";
    #[cfg(not(any(unix, windows)))]
    return Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "TTY input not supported on this platform",
    ));

    #[cfg(any(unix, windows))]
    {
        use std::io::BufRead;
        let tty = std::fs::File::open(TTY_PATH)?;
        let mut reader = std::io::BufReader::new(tty);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        Ok(line)
    }
}

/// Find the rustdoc JSON file for a package in a doc directory.
///
/// Tries the package-name-based path first (`pkg_name → underscored → .json`).
/// If not found, queries `cargo metadata` to discover the actual lib target name.
/// This handles crates where `[lib] name` differs from the package name
/// (e.g. `rustls-webpki` generates `webpki.json`, not `rustls_webpki.json`).
pub fn find_lib_json_path(
    base_name: &str,
    manifest_path: Option<&str>,
    doc_dir: &Path,
) -> Option<PathBuf> {
    let expected_stem = base_name.replace('-', "_");
    let expected = doc_dir.join(format!("{expected_stem}.json"));
    if expected.exists() {
        return Some(expected);
    }
    let lib_name = query_lib_target_name(base_name, manifest_path)?;
    if lib_name == expected_stem {
        return None;
    }
    let alt = doc_dir.join(format!("{lib_name}.json"));
    alt.exists().then_some(alt)
}

fn manifest_cache_key(manifest_path: Option<&str>) -> String {
    if let Some(manifest) = manifest_path {
        return Path::new(manifest)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(manifest))
            .to_string_lossy()
            .into_owned();
    }

    env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .into_owned()
}

/// Run `cargo metadata` to find lib/proc-macro target names for all packages.
///
/// Runs without `--no-deps` so that external crates (e.g. crates.io deps in an
/// isolated temp workspace) are also visible in the packages list.
fn load_lib_target_names(manifest_path: Option<&str>) -> Option<HashMap<String, String>> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version=1"]);
    if let Some(m) = manifest_path {
        cmd.args(["--manifest-path", m]);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let meta: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let packages = meta.get("packages")?.as_array()?;

    let mut names = HashMap::new();
    for pkg in packages {
        let package_name = pkg["name"].as_str()?;
        for target in pkg["targets"].as_array()? {
            let kinds = target["kind"].as_array()?;
            let is_lib = kinds
                .iter()
                .any(|k| matches!(k.as_str(), Some("lib") | Some("proc-macro")));
            if is_lib {
                let package_key = package_name.replace('-', "_");
                let target_name = target["name"].as_str()?.replace('-', "_");
                names.insert(package_key, target_name);
                break;
            }
        }
    }
    Some(names)
}

fn query_lib_target_name(package_name: &str, manifest_path: Option<&str>) -> Option<String> {
    let norm = package_name.replace('-', "_");
    let cache_key = manifest_cache_key(manifest_path);
    let cache = LIB_TARGET_NAME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(guard) = cache.lock()
        && let Some(cached) = guard.get(&cache_key)
    {
        return cached.get(&norm).cloned();
    }

    let names = load_lib_target_names(manifest_path)?;
    let target_name = names.get(&norm).cloned();
    if let Ok(mut guard) = cache.lock() {
        guard.insert(cache_key, names);
    }
    target_name
}

fn describe_lib_json_fallback(
    base_name: &str,
    manifest_path: Option<&str>,
    doc_dir: &Path,
) -> String {
    let expected_stem = base_name.replace('-', "_");
    let Some(lib_name) = query_lib_target_name(base_name, manifest_path) else {
        return format!("cargo metadata did not resolve a lib/proc-macro target for '{base_name}'");
    };
    if lib_name == expected_stem {
        return format!("cargo metadata resolved '{base_name}' to the expected target name");
    }
    format!(
        "cargo metadata resolved '{base_name}' to lib target '{lib_name}', but {} was also missing",
        doc_dir.join(format!("{lib_name}.json")).display()
    )
}

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
        let doc_dir = target_dir.join("doc");
        if let Some(json_path) = find_lib_json_path(base_name, manifest_path, &doc_dir) {
            if verbose {
                eprintln!("[cargo-brief] Using cached rustdoc JSON for '{crate_name}'");
            }
            return Ok(json_path);
        }
    }

    ensure_toolchain_available(toolchain)?;

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
                // Auto-retry: parse candidate specs, pick highest version
                if !crate_name.contains('@') {
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

                    if let Some(best) = pick_highest_version_spec(&specs) {
                        return generate_rustdoc_json(
                            best,
                            toolchain,
                            manifest_path,
                            document_private_items,
                            target_dir,
                            verbose,
                            use_cache,
                        );
                    }
                }

                // Fallback: bail with user-facing message
                bail!(
                    "Multiple versions of '{crate_name}' exist and auto-resolution failed. \
                     Use `<name>@<version>` to disambiguate (e.g. `{crate_name}@1.0.0`)."
                );
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

    // Find the generated JSON file. Strip `@version` suffix first — cargo uses it
    // for disambiguation but the output file is named by the lib target name.
    // Use find_lib_json_path to handle crates where [lib] name != package name.
    let base_name = crate_name.split('@').next().unwrap_or(crate_name);
    let doc_dir = target_dir.join("doc");
    find_lib_json_path(base_name, manifest_path, &doc_dir).with_context(|| {
        let expected_name = base_name.replace('-', "_");
        let fallback = describe_lib_json_fallback(base_name, manifest_path, &doc_dir);
        format!(
            "Expected rustdoc JSON at {} but file not found; {fallback}",
            doc_dir.join(format!("{expected_name}.json")).display(),
        )
    })
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

/// Parse Cargo.lock to extract all resolved package names and versions.
///
/// Returns a `LockfilePackages` with hyphenated package names (as they appear
/// in Cargo.lock) and multi-version tracking for disambiguation.
/// Returns an empty struct on any error (missing file, malformed content).
pub fn load_lockfile_packages(manifest_path: Option<&str>) -> LockfilePackages {
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
        Err(_) => {
            return LockfilePackages {
                names: HashSet::new(),
                multi_versions: HashMap::new(),
            };
        }
    };

    let mut names = HashSet::new();
    let mut all_versions: HashMap<String, Vec<String>> = HashMap::new();
    let mut current_name: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            current_name = None;
            continue;
        }
        if trimmed.starts_with("name = \"") {
            current_name = trimmed
                .strip_prefix("name = \"")
                .and_then(|s| s.strip_suffix('"'))
                .map(|s| s.to_string());
            if let Some(ref name) = current_name {
                names.insert(name.clone());
            }
        } else if trimmed.starts_with("version = \"")
            && let Some(ref name) = current_name
            && let Some(ver) = trimmed
                .strip_prefix("version = \"")
                .and_then(|s| s.strip_suffix('"'))
        {
            all_versions
                .entry(name.clone())
                .or_default()
                .push(ver.to_string());
        } else if trimmed.starts_with('[') {
            current_name = None;
        }
    }

    // Only retain entries with 2+ versions
    let multi_versions: HashMap<String, Vec<String>> = all_versions
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(name, mut versions)| {
            // Sort by semver ascending; fall back to string sort
            versions.sort_by(
                |a, b| match (semver::Version::parse(a), semver::Version::parse(b)) {
                    (Ok(va), Ok(vb)) => va.cmp(&vb),
                    _ => a.cmp(b),
                },
            );
            (name, versions)
        })
        .collect();

    LockfilePackages {
        names,
        multi_versions,
    }
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
        let base = name.split('@').next().unwrap_or(name);
        let doc_dir = target_dir.join("doc");
        if find_lib_json_path(base, manifest_path, &doc_dir).is_some() {
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
        let base = name.split('@').next().unwrap_or(name);
        let doc_dir = target_dir.join("doc");
        if find_lib_json_path(base, manifest_path, &doc_dir).is_some() {
            succeeded.push(name.to_string());
        } else if verbose {
            eprintln!("warning: batch cargo doc did not produce JSON for '{name}'");
        }
    }

    succeeded
}

/// Pick the spec with the highest semver version from a list like `["foo@1.0.0", "foo@2.0.0"]`.
fn pick_highest_version_spec<'a>(specs: &[&'a str]) -> Option<&'a str> {
    specs
        .iter()
        .filter_map(|&s| {
            let ver_str = s.split_once('@')?.1;
            let ver = semver::Version::parse(ver_str).ok()?;
            Some((ver, s))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, s)| s)
}
