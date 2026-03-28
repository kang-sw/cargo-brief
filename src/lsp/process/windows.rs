use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};

/// Check if a process is alive via `OpenProcess(SYNCHRONIZE)`.
pub(in crate::lsp) fn process_alive(pid: u32) -> bool {
    // SAFETY: OpenProcess with SYNCHRONIZE is a standard Windows API call.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return false;
    }
    // SAFETY: handle is valid and non-null.
    unsafe { CloseHandle(handle) };
    true
}

/// `CREATE_NEW_PROCESS_GROUP` detaches the daemon from the parent's console group.
const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

/// Detach a daemon process from the parent's process group.
pub(in crate::lsp) fn configure_daemon_spawn(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

/// Look up a binary on PATH using `where.exe`.
pub(in crate::lsp) fn find_binary_on_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("where.exe").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    // `where.exe` may return multiple lines; take the first.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.lines().next()?.trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}
