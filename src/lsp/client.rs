//! Client-side logic: ensure daemon is running, connect, send commands.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// Daemon directory for a workspace. Uses `<target_dir>/cargo-brief-lsp/<hash>`
/// so the IPC files live inside the project's target directory (sandbox-friendly).
/// Canonicalizes the workspace root to avoid duplicate daemons from symlinks.
pub fn daemon_dir(target_dir: &Path, workspace_root: &Path) -> PathBuf {
    let canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let hash = short_hash(&canonical);
    target_dir.join("cargo-brief-lsp").join(hash)
}

/// Ensure daemon is running. Returns the daemon directory path.
/// Liveness check: PID file alive + readiness indicator exists.
pub fn ensure_daemon(target_dir: &Path, workspace_root: &Path, verbose: bool) -> Result<PathBuf> {
    let dir = daemon_dir(target_dir, workspace_root);
    let pid_file = dir.join("lsp.pid");
    let ready = super::ipc::ready_indicator(&dir);

    // Check if existing daemon is alive and ready
    if ready.exists()
        && pid_file.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && super::process::process_alive(pid)
    {
        if verbose {
            eprintln!("[lsp] daemon already running (PID {pid})");
        }
        return Ok(dir);
    }

    // Check for stale PID file
    if pid_file.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && !super::process::process_alive(pid)
    {
        if verbose {
            eprintln!("[lsp] cleaning up stale daemon (PID {pid})");
        }
        cleanup_daemon_files(&dir);
    }

    // Spawn daemon process
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create daemon dir: {}", dir.display()))?;

    let log_path = dir.join("lsp.log");

    if verbose {
        eprintln!("[lsp] spawning daemon for {}", workspace_root.display());
    }
    let mut child = spawn_daemon(workspace_root, &dir, &log_path)?;

    // Wait for readiness indicator to appear (daemon creates it after ra init)
    wait_for_daemon(&dir, Duration::from_secs(120), &mut child, &log_path)?;
    Ok(dir)
}

/// Remove all daemon files (IPC files + PID + log) from a daemon directory.
pub(super) fn cleanup_daemon_files(dir: &Path) {
    super::ipc::cleanup_ipc_files(dir);
    for name in ["lsp.pid", "lsp.log"] {
        std::fs::remove_file(dir.join(name)).ok();
    }
}

/// Spawn the daemon via re-exec. Returns the Child handle.
fn spawn_daemon(workspace_root: &Path, daemon_dir: &Path, log_path: &Path) -> Result<Child> {
    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    let ws_str = workspace_root
        .to_str()
        .context("Non-UTF8 workspace root path")?;
    let dir_str = daemon_dir.to_str().context("Non-UTF8 daemon dir path")?;

    let log_file = File::create(log_path)
        .with_context(|| format!("Failed to create daemon log: {}", log_path.display()))?;

    let mut cmd = Command::new(exe);
    cmd.args([
        "__lsp-daemon",
        "--workspace-root",
        ws_str,
        "--daemon-dir",
        dir_str,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::from(log_file));

    super::process::configure_daemon_spawn(&mut cmd);

    let child = cmd.spawn().context("Failed to spawn LSP daemon process")?;

    Ok(child)
}

/// Wait for the daemon's readiness indicator to appear.
/// Uses `child.try_wait()` each iteration for fast failure detection.
fn wait_for_daemon(
    daemon_dir: &Path,
    timeout: Duration,
    child: &mut Child,
    log_path: &Path,
) -> Result<()> {
    let start = Instant::now();
    let mut interval = Duration::from_millis(50);
    let pid = child.id();
    let ready = super::ipc::ready_indicator(daemon_dir);

    while start.elapsed() < timeout {
        if ready.exists() {
            return Ok(());
        }

        // Check if daemon died before readiness
        if let Ok(Some(_status)) = child.try_wait() {
            let tail = read_log_tail(log_path, 20);
            let log_section = if tail.is_empty() {
                "(no log output)".to_string()
            } else {
                tail
            };
            bail!(
                "LSP daemon (PID {pid}) died during startup.\n\
                 Daemon log:\n{log_section}"
            );
        }

        std::thread::sleep(interval);
        // Exponential backoff up to 500ms
        interval = (interval * 2).min(Duration::from_millis(500));
    }

    let tail = read_log_tail(log_path, 20);
    let log_section = if tail.is_empty() {
        "(no log output)".to_string()
    } else {
        tail
    };
    bail!(
        "Timed out waiting for LSP daemon after {}s.\n\
         Daemon dir: {}\n\
         Daemon log:\n{log_section}",
        timeout.as_secs(),
        daemon_dir.display()
    )
}

/// Read the last `max_lines` lines from a file. Returns empty string on any error.
fn read_log_tail(path: &Path, max_lines: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// FNV-1a 64-bit hash of a path, hex-encoded. Deterministic across Rust versions.
fn short_hash(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_deterministic() {
        let h1 = short_hash(Path::new("/home/user/project"));
        let h2 = short_hash(Path::new("/home/user/project"));
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_differs_for_different_paths() {
        let h1 = short_hash(Path::new("/home/user/project-a"));
        let h2 = short_hash(Path::new("/home/user/project-b"));
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_is_16_hex_chars() {
        let h = short_hash(Path::new("/some/path"));
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn log_tail_more_than_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");
        let content: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, &content).unwrap();
        let tail = read_log_tail(&path, 20);
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), 20);
        assert_eq!(lines[0], "line 11");
        assert_eq!(lines[19], "line 30");
    }

    #[test]
    fn log_tail_fewer_than_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");
        std::fs::write(&path, "line 1\nline 2\nline 3\n").unwrap();
        let tail = read_log_tail(&path, 20);
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line 1");
    }

    #[test]
    fn log_tail_nonexistent_file() {
        let tail = read_log_tail(Path::new("/nonexistent/file.log"), 20);
        assert!(tail.is_empty());
    }

    #[test]
    fn log_tail_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.log");
        std::fs::write(&path, "").unwrap();
        let tail = read_log_tail(&path, 20);
        assert!(tail.is_empty());
    }
}
