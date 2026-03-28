//! Platform-abstracted process management for the LSP daemon.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{configure_daemon_spawn, find_binary_on_path, process_alive};
#[cfg(windows)]
pub(super) use windows::{configure_daemon_spawn, find_binary_on_path, process_alive};
