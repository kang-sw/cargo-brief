//! Windows IPC implementation using atomic-rename file protocol + LockFileEx.
//!
//! Protocol:
//! - Client writes `lsp.req.tmp`, renames to `lsp.req` (atomic).
//! - Daemon polls for `lsp.req`, reads + deletes it.
//! - Daemon writes `lsp.resp.tmp`, renames to `lsp.resp` (atomic).
//! - Client polls for `lsp.resp`, reads + deletes it.
//! - `LockFileEx` on `lsp.lock` serializes concurrent clients.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx};
use windows_sys::Win32::System::IO::OVERLAPPED;

use crate::lsp::protocol::{DaemonRequest, DaemonResponse, read_message, write_message};

/// Poll interval for file-based IPC (ms).
const POLL_INTERVAL_MS: u64 = 10;

// ── Locking helper ────────────────────────────────────────────────────

/// Acquire an exclusive lock on `file` using `LockFileEx`.
fn lock_exclusive(file: &File) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    let handle: HANDLE = file.as_raw_handle() as HANDLE;
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: LockFileEx is a standard Windows API; handle is valid while File lives.
    let ret = unsafe {
        LockFileEx(
            handle,
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if ret == 0 {
        return Err(std::io::Error::last_os_error()).context("LockFileEx failed");
    }
    Ok(())
}

// ── DaemonIpc ─────────────────────────────────────────────────────────

/// Daemon-side IPC handle. Windows uses file-based polling — no persistent handles.
pub(in crate::lsp) struct DaemonIpc {
    daemon_dir: PathBuf,
}

impl DaemonIpc {
    /// Create IPC endpoints (lock file + readiness marker).
    /// Called AFTER ra init — marker creation IS the readiness signal.
    pub fn setup(daemon_dir: &Path) -> Result<Self> {
        let lock_path = daemon_dir.join("lsp.lock");
        let ready_path = daemon_dir.join("lsp.ready");

        File::create(&lock_path).context("Failed to create lock file")?;
        File::create(&ready_path).context("Failed to create readiness marker")?;

        Ok(Self {
            daemon_dir: daemon_dir.to_path_buf(),
        })
    }

    /// Poll for an incoming client request. Returns `None` on timeout.
    pub fn poll_request(&mut self, timeout_ms: i32) -> Result<Option<DaemonRequest>> {
        let req_path = self.daemon_dir.join("lsp.req");
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(0) as u64);

        loop {
            if req_path.exists() {
                let mut file = File::open(&req_path).context("Failed to open request file")?;
                let request: DaemonRequest = read_message(&mut file)?;
                drop(file);
                fs::remove_file(&req_path).ok();
                return Ok(Some(request));
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    /// Send a response to the client via atomic rename.
    pub fn send_response(&mut self, response: &DaemonResponse) -> Result<()> {
        let tmp_path = self.daemon_dir.join("lsp.resp.tmp");
        let resp_path = self.daemon_dir.join("lsp.resp");

        let mut file = File::create(&tmp_path).context("Failed to create response tmp file")?;
        write_message(&mut file, response)?;
        drop(file);
        fs::rename(&tmp_path, &resp_path).context("Failed to rename response file")?;
        Ok(())
    }
}

// ── Free functions ────────────────────────────────────────────────────

/// Send a request to the daemon and return the response.
/// Uses `LockFileEx` on `lsp.lock` to serialize concurrent clients.
pub(in crate::lsp) fn send_command(
    daemon_dir: &Path,
    request: DaemonRequest,
    timeout: Duration,
) -> Result<DaemonResponse> {
    let lock_path = daemon_dir.join("lsp.lock");
    let req_tmp = daemon_dir.join("lsp.req.tmp");
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
    lock_exclusive(&lock_file)?;

    // 2. Drain stale response from a previously crashed client
    fs::remove_file(&resp_path).ok();

    // 3. Write request to tmp file, then atomic rename
    let mut req_file = File::create(&req_tmp).context("Failed to create request tmp file")?;
    write_message(&mut req_file, &request)?;
    drop(req_file);
    fs::rename(&req_tmp, &req_path).context("Failed to rename request file")?;

    // 3. Poll for response with timeout
    let now = Instant::now();
    let deadline = now
        .checked_add(timeout)
        .unwrap_or(now + Duration::from_secs(86400 * 365));
    loop {
        if resp_path.exists() {
            let mut file = File::open(&resp_path).context("Failed to open response file")?;
            let response: DaemonResponse = read_message(&mut file)?;
            drop(file);
            fs::remove_file(&resp_path).ok();
            return Ok(response);
        }

        if Instant::now() >= deadline {
            bail!(
                "Timed out waiting for daemon response ({}s)",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
    // lock auto-released on lock_file drop
}

/// Remove IPC-specific files. Does NOT remove pid/log.
pub(in crate::lsp) fn cleanup_ipc_files(dir: &Path) {
    for name in [
        "lsp.req",
        "lsp.resp",
        "lsp.lock",
        "lsp.req.tmp",
        "lsp.resp.tmp",
        "lsp.ready",
    ] {
        fs::remove_file(dir.join(name)).ok();
    }
}

/// Path whose existence signals daemon readiness. Clients poll this.
/// On Windows, a dedicated `lsp.ready` marker file.
pub(in crate::lsp) fn ready_indicator(dir: &Path) -> PathBuf {
    dir.join("lsp.ready")
}
