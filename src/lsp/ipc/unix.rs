//! Unix IPC implementation using FIFO pair + flock serialization.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::lsp::protocol::{DaemonRequest, DaemonResponse, read_message, write_message};

// ── Low-level primitives ──────────────────────────────────────────────

/// Create a named pipe (FIFO) at `path`. Ignores `EEXIST` (idempotent).
fn create_fifo(path: &Path, mode: libc::mode_t) -> Result<()> {
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
fn flock_exclusive(file: &File) -> Result<()> {
    // SAFETY: flock is a standard POSIX call; fd is valid while File is alive.
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error()).context("flock(LOCK_EX) failed");
    }
    Ok(())
}

/// Call `libc::poll()` with EINTR retry. Returns the poll result (>0 = ready, 0 = timeout).
pub(in crate::lsp) fn poll_retry(
    pfd: &mut libc::pollfd,
    timeout_ms: libc::c_int,
) -> Result<libc::c_int> {
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
fn set_nonblocking(file: &File, nonblock: bool) -> Result<()> {
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

// ── DaemonIpc ─────────────────────────────────────────────────────────

/// Daemon-side IPC handle. Owns the FIFO file descriptors.
pub(in crate::lsp) struct DaemonIpc {
    req_fd: File,  // lsp.req FIFO, opened O_RDWR + O_NONBLOCK
    resp_fd: File, // lsp.resp FIFO, opened O_RDWR (blocking)
}

impl DaemonIpc {
    /// Create IPC endpoints (FIFOs + lock file).
    /// Called AFTER ra init — endpoint creation IS the readiness signal.
    /// Cleans up stale endpoint files before creating new ones.
    pub fn setup(daemon_dir: &Path) -> Result<Self> {
        let req_path = daemon_dir.join("lsp.req");
        let resp_path = daemon_dir.join("lsp.resp");
        let lock_path = daemon_dir.join("lsp.lock");

        // Clean stale FIFOs first
        std::fs::remove_file(&req_path).ok();
        std::fs::remove_file(&resp_path).ok();
        create_fifo(&req_path, 0o600).context("Failed to create request FIFO")?;
        create_fifo(&resp_path, 0o600).context("Failed to create response FIFO")?;
        File::create(&lock_path).context("Failed to create lock file")?;

        // Open FIFOs with O_RDWR — daemon keeps both open for its lifetime.
        // O_RDWR prevents open() from blocking and eliminates POLLHUP races.
        let req_fd = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&req_path)
            .context("Failed to open request FIFO")?;
        let resp_fd = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&resp_path)
            .context("Failed to open response FIFO")?;

        Ok(Self { req_fd, resp_fd })
    }

    /// Poll for an incoming client request. Returns `None` on timeout.
    pub fn poll_request(&mut self, timeout_ms: i32) -> Result<Option<DaemonRequest>> {
        let mut pfd = libc::pollfd {
            fd: self.req_fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let n = poll_retry(&mut pfd, timeout_ms)?;

        if n > 0 && (pfd.revents & libc::POLLIN) != 0 {
            // Client sent data — switch to blocking, read full message
            set_nonblocking(&self.req_fd, false)?;
            let request: DaemonRequest = match read_message(&mut &self.req_fd) {
                Ok(req) => req,
                Err(e) => {
                    set_nonblocking(&self.req_fd, true)?;
                    return Err(e).context("Failed to read request");
                }
            };
            set_nonblocking(&self.req_fd, true)?;
            Ok(Some(request))
        } else {
            Ok(None)
        }
    }

    /// Send a response to the client.
    pub fn send_response(&mut self, response: &DaemonResponse) -> Result<()> {
        write_message(&mut self.resp_fd, response)
    }
}

// ── Free functions ────────────────────────────────────────────────────

/// Send a request to the daemon via FIFO and return the response.
/// Uses `flock` on `lsp.lock` to serialize concurrent clients.
pub(in crate::lsp) fn send_command(
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

/// Remove IPC-specific files (req, resp, lock). Does NOT remove pid/log.
pub(in crate::lsp) fn cleanup_ipc_files(dir: &Path) {
    for name in ["lsp.req", "lsp.resp", "lsp.lock"] {
        std::fs::remove_file(dir.join(name)).ok();
    }
}

/// Path whose existence signals daemon readiness. Clients poll this.
/// On Unix, FIFO existence = ready.
pub(in crate::lsp) fn ready_indicator(dir: &Path) -> PathBuf {
    dir.join("lsp.req")
}

#[cfg(test)]
mod tests {
    use super::*;

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
