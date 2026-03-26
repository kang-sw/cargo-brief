//! Client-side logic: ensure daemon is running, connect, send commands.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::protocol::{DaemonRequest, DaemonResponse, read_message, write_message};

/// Socket/PID directory for a workspace. Canonicalizes the root to avoid
/// duplicate daemons from symlinks or `..` path components.
pub fn daemon_dir(workspace_root: &Path) -> PathBuf {
    let canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let hash = short_hash(&canonical);
    runtime_dir().join("cargo-brief").join(hash)
}

/// Ensure daemon is running and return a connected UDS stream.
pub fn ensure_daemon(workspace_root: &Path, verbose: bool) -> Result<UnixStream> {
    let dir = daemon_dir(workspace_root);
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

    if verbose {
        eprintln!("[lsp] spawning daemon for {}", workspace_root.display());
    }
    spawn_daemon(workspace_root, &sock, &pid_file)?;

    // Wait for socket to become available (poll with backoff)
    wait_for_socket(&sock, Duration::from_secs(120))
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

/// Spawn the daemon via re-exec.
fn spawn_daemon(workspace_root: &Path, sock: &Path, pid: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    let ws_str = workspace_root
        .to_str()
        .context("Non-UTF8 workspace root path")?;
    let sock_str = sock.to_str().context("Non-UTF8 socket path")?;
    let pid_str = pid.to_str().context("Non-UTF8 pid file path")?;

    Command::new(exe)
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
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("Failed to spawn LSP daemon process")?;

    Ok(())
}

/// Wait for the socket file to appear and be connectable.
fn wait_for_socket(sock: &Path, timeout: Duration) -> Result<UnixStream> {
    let start = Instant::now();
    let mut interval = Duration::from_millis(50);

    while start.elapsed() < timeout {
        if let Some(stream) = try_connect(sock) {
            return Ok(stream);
        }
        std::thread::sleep(interval);
        // Exponential backoff up to 500ms
        interval = (interval * 2).min(Duration::from_millis(500));
    }

    bail!(
        "Timed out waiting for LSP daemon socket after {}s.\n\
         Socket path: {}",
        timeout.as_secs(),
        sock.display()
    )
}

/// Check if a process is alive via kill(pid, 0).
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(pid, 0) with signal 0 only checks process existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Get the runtime directory for daemon sockets.
fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
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
}
