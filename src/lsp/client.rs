//! Client-side logic: ensure daemon is running, connect, send commands.

use std::fs::File;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::protocol::{DaemonRequest, DaemonResponse, read_message, write_message};

/// Socket/PID directory for a workspace. Uses `<target_dir>/cargo-brief-lsp/<hash>`
/// so the socket lives inside the project's target directory (sandbox-friendly).
/// Canonicalizes the workspace root to avoid duplicate daemons from symlinks.
pub fn daemon_dir(target_dir: &Path, workspace_root: &Path) -> PathBuf {
    let canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let hash = short_hash(&canonical);
    target_dir.join("cargo-brief-lsp").join(hash)
}

/// Ensure daemon is running and return a connected UDS stream.
pub fn ensure_daemon(
    target_dir: &Path,
    workspace_root: &Path,
    verbose: bool,
) -> Result<UnixStream> {
    let dir = daemon_dir(target_dir, workspace_root);
    let sock = dir.join("lsp.sock");
    let pid_file = dir.join("lsp.pid");

    // Try connecting to existing daemon
    if let Some(stream) = try_connect(&sock) {
        if verbose {
            eprintln!("[lsp] connected to existing daemon");
        }
        return Ok(stream);
    }

    // Check for stale PID file
    if pid_file.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && !process_alive(pid)
    {
        if verbose {
            eprintln!("[lsp] cleaning up stale daemon (PID {pid})");
        }
        std::fs::remove_file(&pid_file).ok();
        std::fs::remove_file(&sock).ok();
    }

    // Spawn daemon process
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create daemon dir: {}", dir.display()))?;

    let log_path = dir.join("lsp.log");

    if verbose {
        eprintln!("[lsp] spawning daemon for {}", workspace_root.display());
    }
    let mut child = spawn_daemon(workspace_root, &sock, &pid_file, &log_path)?;

    // Wait for socket to become available (poll with backoff)
    wait_for_socket(&sock, Duration::from_secs(120), &mut child, &log_path)
}

/// Send a request to the daemon and return the response.
pub fn send_command(stream: &mut UnixStream, request: DaemonRequest) -> Result<DaemonResponse> {
    write_message(stream, &request)?;
    read_message(stream)
}

/// Try connecting to an existing daemon. Returns None if connection fails.
fn try_connect(sock: &Path) -> Option<UnixStream> {
    let mut stream = UnixStream::connect(sock).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    // Verify with a ping
    write_message(&mut stream, &DaemonRequest::Ping).ok()?;
    let resp: DaemonResponse = read_message(&mut stream).ok()?;

    match resp {
        DaemonResponse::Ok { .. } => Some(stream),
        _ => None,
    }
}

/// Spawn the daemon via re-exec. Returns the Child handle (caller must hold it
/// to avoid zombie processes — `try_wait()` is used for death detection).
fn spawn_daemon(workspace_root: &Path, sock: &Path, pid: &Path, log_path: &Path) -> Result<Child> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    let ws_str = workspace_root
        .to_str()
        .context("Non-UTF8 workspace root path")?;
    let sock_str = sock.to_str().context("Non-UTF8 socket path")?;
    let pid_str = pid.to_str().context("Non-UTF8 pid file path")?;

    let log_file = File::create(log_path)
        .with_context(|| format!("Failed to create daemon log: {}", log_path.display()))?;

    let child = Command::new(exe)
        .args([
            "__lsp-daemon",
            "--workspace-root",
            ws_str,
            "--socket",
            sock_str,
            "--pid-file",
            pid_str,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .process_group(0)
        .spawn()
        .context("Failed to spawn LSP daemon process")?;

    Ok(child)
}

/// Wait for the socket file to appear and be connectable.
/// Uses `child.try_wait()` each iteration for fast failure detection (also reaps
/// zombies — `kill(pid, 0)` alone cannot detect exited children of this process).
fn wait_for_socket(
    sock: &Path,
    timeout: Duration,
    child: &mut Child,
    log_path: &Path,
) -> Result<UnixStream> {
    let start = Instant::now();
    let mut interval = Duration::from_millis(50);
    let pid = child.id();

    while start.elapsed() < timeout {
        if let Some(stream) = try_connect(sock) {
            return Ok(stream);
        }

        // Check if daemon died before we could connect (try_wait reaps zombies)
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
        "Timed out waiting for LSP daemon socket after {}s.\n\
         Socket path: {}\n\
         Daemon log:\n{log_section}",
        timeout.as_secs(),
        sock.display()
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

/// Check if a process is alive via kill(pid, 0).
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(pid, 0) with signal 0 only checks process existence.
    unsafe { libc::kill(pid, 0) == 0 }
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
