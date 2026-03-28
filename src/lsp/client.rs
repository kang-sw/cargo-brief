//! Client-side logic: ensure daemon is running, connect, send commands.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::protocol::{DaemonRequest, DaemonResponse, read_message, write_message};

/// Create a named pipe (FIFO) at `path`. Ignores `EEXIST` (idempotent).
pub(super) fn create_fifo(path: &Path, mode: libc::mode_t) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path =
        CString::new(path.as_os_str().as_bytes()).context("FIFO path contains null byte")?;
    // SAFETY: mkfifo is a standard POSIX call; c_path is valid and null-terminated.
    let ret = unsafe { libc::mkfifo(c_path.as_ptr(), mode) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EEXIST) {
            return Err(err).with_context(|| format!("mkfifo failed: {}", path.display()));
        }
    }
    Ok(())
}

/// Acquire an exclusive advisory lock on `file` (blocking).
pub(super) fn flock_exclusive(file: &File) -> Result<()> {
    // SAFETY: flock is a standard POSIX call; fd is valid while File is alive.
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error()).context("flock(LOCK_EX) failed");
    }
    Ok(())
}

/// Call `libc::poll()` with EINTR retry. Returns the poll result (>0 = ready, 0 = timeout).
pub(super) fn poll_retry(pfd: &mut libc::pollfd, timeout_ms: libc::c_int) -> Result<libc::c_int> {
    loop {
        // SAFETY: poll on a valid fd with a stack-allocated pollfd.
        let n = unsafe { libc::poll(pfd, 1, timeout_ms) };
        if n >= 0 {
            return Ok(n);
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EINTR) {
            return Err(err).context("poll() failed");
        }
        // EINTR — retry
    }
}

/// Toggle `O_NONBLOCK` on a file descriptor.
pub(super) fn set_nonblocking(file: &File, nonblock: bool) -> Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY: fcntl F_GETFL/F_SETFL are standard POSIX calls on a valid fd.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 {
            return Err(std::io::Error::last_os_error()).context("fcntl(F_GETFL) failed");
        }
        let new_flags = if nonblock {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        if libc::fcntl(fd, libc::F_SETFL, new_flags) == -1 {
            return Err(std::io::Error::last_os_error()).context("fcntl(F_SETFL) failed");
        }
    }
    Ok(())
}

/// Daemon directory for a workspace. Uses `<target_dir>/cargo-brief-lsp/<hash>`
/// so the FIFOs live inside the project's target directory (sandbox-friendly).
/// Canonicalizes the workspace root to avoid duplicate daemons from symlinks.
pub fn daemon_dir(target_dir: &Path, workspace_root: &Path) -> PathBuf {
    let canonical = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let hash = short_hash(&canonical);
    target_dir.join("cargo-brief-lsp").join(hash)
}

/// Ensure daemon is running. Returns the daemon directory path.
/// Liveness check: PID file alive + `lsp.req` FIFO exists (readiness invariant).
pub fn ensure_daemon(target_dir: &Path, workspace_root: &Path, verbose: bool) -> Result<PathBuf> {
    let dir = daemon_dir(target_dir, workspace_root);
    let pid_file = dir.join("lsp.pid");
    let req_fifo = dir.join("lsp.req");

    // Check if existing daemon is alive and ready (FIFOs exist)
    if req_fifo.exists()
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

    // Wait for FIFO to appear (daemon creates FIFOs after ra init)
    wait_for_daemon(&dir, Duration::from_secs(120), &mut child, &log_path)?;
    Ok(dir)
}

/// Send a request to the daemon via FIFO and return the response.
/// Uses `flock` on `lsp.lock` to serialize concurrent clients.
pub fn send_command(
    daemon_dir: &Path,
    request: DaemonRequest,
    timeout: Duration,
) -> Result<DaemonResponse> {
    let lock_path = daemon_dir.join("lsp.lock");
    let req_path = daemon_dir.join("lsp.req");
    let resp_path = daemon_dir.join("lsp.resp");

    // 1. Acquire exclusive lock
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .context("Failed to open lock file")?;
    flock_exclusive(&lock_file)?;

    // 2. Open req FIFO for writing (blocks until daemon has read-end — instant)
    let mut req_fd = OpenOptions::new()
        .write(true)
        .open(&req_path)
        .context("Failed to open request FIFO")?;

    // 3. Write request, then drop to signal we're done writing
    write_message(&mut req_fd, &request)?;
    drop(req_fd);

    // 4. Open resp FIFO for reading (non-blocking initially for drain + poll)
    let resp_fd = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&resp_path)
        .context("Failed to open response FIFO")?;

    // 4a. Drain stale data from resp FIFO (from a previously crashed client)
    let mut drain_buf = [0u8; 4096];
    loop {
        // SAFETY: read on a valid fd with a stack-allocated buffer.
        let n = unsafe {
            libc::read(
                resp_fd.as_raw_fd(),
                drain_buf.as_mut_ptr() as *mut libc::c_void,
                drain_buf.len(),
            )
        };
        if n <= 0 {
            break;
        }
    }

    // 5. Poll for response with timeout (EINTR-safe)
    let timeout_ms: libc::c_int = timeout.as_millis().try_into().unwrap_or(libc::c_int::MAX);
    let mut pfd = libc::pollfd {
        fd: resp_fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let n = poll_retry(&mut pfd, timeout_ms)?;
    if n == 0 {
        bail!(
            "Timed out waiting for daemon response ({}s)",
            timeout.as_secs()
        );
    }

    // 6. Switch to blocking and read the response
    set_nonblocking(&resp_fd, false)?;
    let mut resp_fd = resp_fd;
    let response: DaemonResponse = read_message(&mut resp_fd)?;

    // 7. flock auto-released on lock_fd drop
    Ok(response)
}

/// Remove daemon files (FIFOs, PID, lock, log) from a daemon directory.
pub(super) fn cleanup_daemon_files(dir: &Path) {
    for name in ["lsp.pid", "lsp.req", "lsp.resp", "lsp.lock", "lsp.log"] {
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

/// Wait for the daemon's `lsp.req` FIFO to appear (readiness signal).
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
    let req_fifo = daemon_dir.join("lsp.req");

    while start.elapsed() < timeout {
        if req_fifo.exists() {
            return Ok(());
        }

        // Check if daemon died before FIFOs appeared (try_wait reaps zombies)
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

    #[test]
    fn create_fifo_creates_pipe() {
        use std::os::unix::fs::FileTypeExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("test.fifo");
        create_fifo(&fifo, 0o600).unwrap();
        assert!(std::fs::metadata(&fifo).unwrap().file_type().is_fifo());
    }

    #[test]
    fn create_fifo_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("test.fifo");
        create_fifo(&fifo, 0o600).unwrap();
        create_fifo(&fifo, 0o600).unwrap(); // second call succeeds (EEXIST ignored)
    }

    #[test]
    fn flock_exclusive_blocks_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");
        let f1 = File::create(&path).unwrap();
        flock_exclusive(&f1).unwrap();

        // Try non-blocking lock — should fail with EWOULDBLOCK
        let f2 = File::open(&path).unwrap();
        let ret = unsafe { libc::flock(f2.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_ne!(ret, 0);
        let err = std::io::Error::last_os_error();
        assert_eq!(err.raw_os_error(), Some(libc::EWOULDBLOCK));
    }

    #[test]
    fn set_nonblocking_toggles_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.file");
        let f = File::create(&path).unwrap();

        set_nonblocking(&f, true).unwrap();
        let flags = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags & libc::O_NONBLOCK, 0);

        set_nonblocking(&f, false).unwrap();
        let flags = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETFL) };
        assert_eq!(flags & libc::O_NONBLOCK, 0);
    }
}
