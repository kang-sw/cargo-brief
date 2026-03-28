//! Platform-abstracted IPC for client ↔ daemon communication.
//!
//! Unix: FIFO pair + `flock` serialization (unchanged from original).
//! Windows: Atomic-rename file protocol + `LockFileEx` serialization.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{DaemonIpc, cleanup_ipc_files, ready_indicator, send_command};
#[cfg(windows)]
pub(super) use windows::{DaemonIpc, cleanup_ipc_files, ready_indicator, send_command};

// Re-export poll_retry for daemon.rs ra-stdout polling
// (Unix-only, will be replaced in Phase 3 with transport abstraction)
#[cfg(unix)]
pub(super) use unix::poll_retry;
