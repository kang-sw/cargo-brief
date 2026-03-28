//! LSP daemon management for semantic code analysis via rust-analyzer.
//!
//! Provides `cargo brief lsp` subcommands: `touch`, `stop`, `status`, `references`,
//! `blast-radius`, `call-hierarchy`.
//! The daemon spawns rust-analyzer as a background process, communicates
//! via LSP over stdio, and accepts client queries via platform-specific IPC.

pub(crate) mod client;
pub mod daemon;
mod ipc;
mod process;
mod protocol;
mod query;
mod transport;
mod watcher;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::cli::{LspArgs, LspCommand, RemoteOpts};
use crate::resolve;

use client::{cleanup_daemon_files, daemon_dir, ensure_daemon};
use ipc::send_command;
use protocol::{DaemonRequest, DaemonResponse};

pub fn run_lsp_command(args: &LspArgs, remote: &RemoteOpts) -> Result<()> {
    if remote.crates {
        anyhow::bail!("LSP commands do not support remote crate mode (-C)");
    }

    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let td = &metadata.target_dir;
    let wr = &metadata.workspace_root;
    let verbose = args.global.verbose;

    match &args.command {
        LspCommand::Touch => cmd_touch(td, wr, verbose),
        LspCommand::Stop => cmd_stop(td, wr, verbose),
        LspCommand::Status => cmd_status(td, wr),
        LspCommand::References { symbol, quiet } => cmd_query(
            td,
            wr,
            verbose,
            DaemonRequest::References {
                symbol: symbol.clone(),
                quiet: *quiet,
            },
        ),
        LspCommand::BlastRadius {
            symbol,
            depth,
            quiet,
        } => cmd_query(
            td,
            wr,
            verbose,
            DaemonRequest::BlastRadius {
                symbol: symbol.clone(),
                depth: *depth,
                quiet: *quiet,
            },
        ),
        LspCommand::CallHierarchy {
            symbol,
            outgoing,
            quiet,
        } => cmd_query(
            td,
            wr,
            verbose,
            DaemonRequest::CallHierarchy {
                symbol: symbol.clone(),
                outgoing: *outgoing,
                quiet: *quiet,
            },
        ),
    }
}

/// Ensure daemon is running (start if needed).
fn cmd_touch(target_dir: &Path, workspace_root: &Path, verbose: bool) -> Result<()> {
    let dir = ensure_daemon(target_dir, workspace_root, verbose)?;

    let resp = send_command(&dir, DaemonRequest::Status, Duration::from_secs(5))?;
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
        DaemonResponse::QueryResult { .. } => {}
    }
    Ok(())
}

/// Stop the daemon.
fn cmd_stop(target_dir: &Path, workspace_root: &Path, verbose: bool) -> Result<()> {
    let dir = daemon_dir(target_dir, workspace_root);

    // Try sending stop — if readiness indicator doesn't exist, daemon is not running
    if ipc::ready_indicator(&dir).exists() {
        match send_command(&dir, DaemonRequest::Stop, Duration::from_secs(5)) {
            Ok(DaemonResponse::Ok { message }) => {
                eprintln!("[lsp] {message}");
            }
            Ok(_) => {}
            Err(e) => {
                if verbose {
                    eprintln!("[lsp] stop failed: {e}");
                }
            }
        }
    } else if verbose {
        eprintln!("[lsp] no daemon running");
    }

    // Clean up files
    cleanup_daemon_files(&dir);
    std::fs::remove_dir(&dir).ok();

    Ok(())
}

/// Client-side timeout for query commands: 120s to accommodate
/// 60s daemon-side indexing wait + 30s query execution + margin.
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);

/// Send a query request to the daemon and print the result.
fn cmd_query(
    target_dir: &Path,
    workspace_root: &Path,
    verbose: bool,
    request: DaemonRequest,
) -> Result<()> {
    let dir = ensure_daemon(target_dir, workspace_root, verbose)?;

    let resp = send_command(&dir, request, QUERY_TIMEOUT)?;
    match resp {
        DaemonResponse::QueryResult { output } => {
            print!("{output}");
            Ok(())
        }
        DaemonResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("Unexpected response from daemon"),
    }
}

/// Show daemon status.
fn cmd_status(target_dir: &Path, workspace_root: &Path) -> Result<()> {
    let dir = daemon_dir(target_dir, workspace_root);

    // Check if daemon is ready
    if !ipc::ready_indicator(&dir).exists() {
        println!("LSP daemon: not running");
        return Ok(());
    }

    match send_command(&dir, DaemonRequest::Status, Duration::from_secs(5)) {
        Ok(resp) => match resp {
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
                println!("  Dir:     {}", dir.display());
            }
            DaemonResponse::Error { message } => {
                println!("LSP daemon: error");
                println!("  {message}");
            }
            DaemonResponse::Ok { message } => {
                println!("LSP daemon: {message}");
            }
            DaemonResponse::QueryResult { .. } => {
                println!("LSP daemon: unexpected response");
            }
        },
        Err(_) => {
            println!("LSP daemon: not running");
        }
    }

    Ok(())
}
