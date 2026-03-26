//! LSP daemon management for semantic code analysis via rust-analyzer.
//!
//! Provides `cargo brief lsp` subcommands: `touch`, `stop`, `status`.
//! The daemon spawns rust-analyzer as a background process, communicates
//! via LSP over stdio, and accepts client queries via Unix domain socket.

pub mod client;
pub mod daemon;
mod protocol;
mod transport;

use anyhow::{Context, Result};

use crate::cli::{LspArgs, LspCommand, RemoteOpts};
use crate::resolve;

use client::{daemon_dir, ensure_daemon, send_command};
use protocol::{DaemonRequest, DaemonResponse};

pub fn run_lsp_command(args: &LspArgs, remote: &RemoteOpts) -> Result<()> {
    if remote.crates {
        anyhow::bail!("LSP commands do not support remote crate mode (-C)");
    }

    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    match &args.command {
        LspCommand::Touch => cmd_touch(&metadata.workspace_root, args.global.verbose),
        LspCommand::Stop => cmd_stop(&metadata.workspace_root, args.global.verbose),
        LspCommand::Status => cmd_status(&metadata.workspace_root),
    }
}

/// Ensure daemon is running (start if needed).
fn cmd_touch(workspace_root: &std::path::Path, verbose: bool) -> Result<()> {
    let mut stream = ensure_daemon(workspace_root, verbose)?;

    // Query status to report to user
    let resp = send_command(&mut stream, DaemonRequest::Status)?;
    match resp {
        DaemonResponse::Status {
            pid,
            ra_status,
            uptime_secs,
        } => {
            eprintln!("[lsp] daemon running (PID {pid}, ra: {ra_status}, uptime: {uptime_secs}s)");
        }
        DaemonResponse::Ok { message } => {
            eprintln!("[lsp] {message}");
        }
        DaemonResponse::Error { message } => {
            eprintln!("[lsp] daemon error: {message}");
        }
    }
    Ok(())
}

/// Stop the daemon.
fn cmd_stop(workspace_root: &std::path::Path, verbose: bool) -> Result<()> {
    let dir = daemon_dir(workspace_root);
    let sock = dir.join("lsp.sock");

    // Try to connect and send stop command
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&sock) {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();

        if let Ok(()) = protocol::write_message(&mut stream, &DaemonRequest::Stop)
            && let Ok(DaemonResponse::Ok { message }) =
                protocol::read_message::<DaemonResponse>(&mut stream)
        {
            eprintln!("[lsp] {message}");
        }
    } else if verbose {
        eprintln!("[lsp] no daemon running");
    }

    // Clean up socket/pid files in case daemon didn't clean up
    let pid_file = dir.join("lsp.pid");
    std::fs::remove_file(&sock).ok();
    std::fs::remove_file(&pid_file).ok();
    std::fs::remove_dir(&dir).ok();

    Ok(())
}

/// Show daemon status.
fn cmd_status(workspace_root: &std::path::Path) -> Result<()> {
    let dir = daemon_dir(workspace_root);
    let sock = dir.join("lsp.sock");

    match std::os::unix::net::UnixStream::connect(&sock) {
        Ok(mut stream) => {
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();

            let resp = send_command(&mut stream, DaemonRequest::Status)?;
            match resp {
                DaemonResponse::Status {
                    pid,
                    ra_status,
                    uptime_secs,
                } => {
                    let minutes = uptime_secs / 60;
                    let seconds = uptime_secs % 60;
                    println!("LSP daemon: running");
                    println!("  PID:     {pid}");
                    println!("  RA:      {ra_status}");
                    println!("  Uptime:  {minutes}m {seconds}s");
                    println!("  Socket:  {}", sock.display());
                }
                DaemonResponse::Error { message } => {
                    println!("LSP daemon: error");
                    println!("  {message}");
                }
                DaemonResponse::Ok { message } => {
                    println!("LSP daemon: {message}");
                }
            }
        }
        Err(_) => {
            println!("LSP daemon: not running");
        }
    }

    Ok(())
}
