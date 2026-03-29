use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// Check if a process is alive via kill(pid, 0).
pub(in crate::lsp) fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(pid, 0) with signal 0 only checks process existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Detach daemon into a new session so it survives parent shell exit.
pub(in crate::lsp) fn configure_daemon_spawn(cmd: &mut Command) {
    // SAFETY: setsid() is async-signal-safe (POSIX). Creates a new session
    // so the daemon is not killed by SIGHUP when the parent terminal closes.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Look up a binary on PATH using `which`.
pub(in crate::lsp) fn find_binary_on_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}
