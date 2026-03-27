//! LSP daemon process: spawns rust-analyzer, accepts FIFO clients, handles idle timeout.

use std::fs::{File, OpenOptions};
use std::io::BufRead;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::client::{create_fifo, poll_retry, set_nonblocking};
use super::protocol::{DaemonRequest, DaemonResponse, RaStatus, read_message, write_message};
use super::query;
use super::transport::RaTransport;
use super::watcher::{self, DebounceBuffer};

/// Default idle timeout: 10 minutes.
const IDLE_TIMEOUT_SECS: u64 = 600;

/// Entry point for the re-exec'd daemon process. Parses args manually.
pub fn run_daemon_from_args() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut workspace_root = None;
    let mut daemon_dir = None;

    let mut i = 2; // skip binary name and "__lsp-daemon"
    while i < args.len() {
        match args[i].as_str() {
            "--workspace-root" | "--daemon-dir" => {
                let flag = &args[i];
                i += 1;
                let value = args
                    .get(i)
                    .with_context(|| format!("Missing value for {flag}"))?;
                match flag.as_str() {
                    "--workspace-root" => workspace_root = Some(PathBuf::from(value)),
                    "--daemon-dir" => daemon_dir = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => bail!("Unknown daemon argument: {other}"),
        }
        i += 1;
    }

    let workspace_root = workspace_root.context("Missing --workspace-root")?;
    let daemon_dir = daemon_dir.context("Missing --daemon-dir")?;

    run_daemon(&workspace_root, &daemon_dir)
}

/// Discover the rust-analyzer binary path.
fn discover_ra_binary() -> Result<PathBuf> {
    // Try rustup first
    if let Ok(output) = Command::new("rustup")
        .args(["which", "rust-analyzer"])
        .output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    // Fall back to PATH
    if let Ok(output) = Command::new("which").arg("rust-analyzer").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    bail!(
        "rust-analyzer not found.\n\
         Install via: rustup component add rust-analyzer\n\
         Or ensure rust-analyzer is available on PATH."
    )
}

/// Send LSP `initialize` request and `initialized` notification.
fn send_initialize(transport: &mut RaTransport, workspace_root: &Path) -> Result<()> {
    let path_str = workspace_root.to_str().context("Non-UTF8 workspace root")?;
    // file:// URIs require three slashes for absolute paths: file:///path
    let root_uri = if path_str.starts_with('/') {
        format!("file://{path_str}")
    } else {
        format!("file:///{path_str}")
    };

    let params = serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {},
        "initializationOptions": {}
    });

    let response = transport.send_request_and_wait("initialize", params)?;

    // Verify we got a result
    if response.get("result").is_none() {
        bail!("LSP initialize response missing 'result' field");
    }

    // Send initialized notification
    transport.send_notification("initialized", serde_json::json!({}))?;

    Ok(())
}

/// Handle a single request and produce a response.
fn handle_request(
    request: &DaemonRequest,
    ra_status: RaStatus,
    start_time: Instant,
    shutdown: &mut bool,
    transport: &mut RaTransport,
    workspace_root: &Path,
) -> DaemonResponse {
    match request {
        DaemonRequest::Stop => {
            *shutdown = true;
            DaemonResponse::Ok {
                message: "stopping".to_string(),
            }
        }
        DaemonRequest::Status => DaemonResponse::Status {
            pid: std::process::id(),
            ra_status,
            uptime_secs: start_time.elapsed().as_secs(),
        },
        DaemonRequest::References { symbol, quiet } => {
            match query::handle_references(transport, workspace_root, symbol, *quiet) {
                Ok(output) => DaemonResponse::QueryResult { output },
                Err(e) => DaemonResponse::Error {
                    message: format!("{e}"),
                },
            }
        }
        DaemonRequest::BlastRadius {
            symbol,
            depth,
            quiet,
        } => match query::handle_blast_radius(transport, workspace_root, symbol, *depth, *quiet) {
            Ok(output) => DaemonResponse::QueryResult { output },
            Err(e) => DaemonResponse::Error {
                message: format!("{e}"),
            },
        },
        DaemonRequest::CallHierarchy {
            symbol,
            outgoing,
            quiet,
        } => {
            match query::handle_call_hierarchy(transport, workspace_root, symbol, *outgoing, *quiet)
            {
                Ok(output) => DaemonResponse::QueryResult { output },
                Err(e) => DaemonResponse::Error {
                    message: format!("{e}"),
                },
            }
        }
    }
}

/// Shutdown rust-analyzer gracefully via LSP shutdown/exit.
/// Bounded read loop: reads at most 10 messages waiting for shutdown response.
/// If ra is already dead, read_message() returns Err immediately (broken pipe).
fn shutdown_ra(transport: &mut RaTransport) {
    if let Ok(id) = transport.send_request("shutdown", serde_json::Value::Null) {
        for _ in 0..10 {
            match transport.read_message() {
                Ok(msg) if msg["id"].as_i64() == Some(id as i64) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    let _ = transport.send_notification("exit", serde_json::Value::Null);
}

/// Main daemon loop.
pub fn run_daemon(workspace_root: &Path, daemon_dir: &Path) -> Result<()> {
    let start_time = Instant::now();
    let pid_path = daemon_dir.join("lsp.pid");
    let req_path = daemon_dir.join("lsp.req");
    let resp_path = daemon_dir.join("lsp.resp");
    let lock_path = daemon_dir.join("lsp.lock");

    // 1. Write PID file early to prevent double-spawn race
    std::fs::write(&pid_path, std::process::id().to_string())
        .context("Failed to write PID file")?;

    // 2. Discover ra binary
    let ra_bin = discover_ra_binary()?;
    eprintln!("[lsp-daemon] using rust-analyzer: {}", ra_bin.display());

    // 3. Spawn ra subprocess
    let mut ra_child = Command::new(&ra_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(workspace_root)
        .spawn()
        .with_context(|| format!("Failed to spawn rust-analyzer: {}", ra_bin.display()))?;

    let ra_stdin = ra_child.stdin.take().context("No stdin on ra process")?;
    let ra_stdout = ra_child.stdout.take().context("No stdout on ra process")?;

    // Drain ra stderr in a background thread to prevent pipe blocking
    if let Some(stderr) = ra_child.stderr.take() {
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[ra-stderr] {line}");
            }
        });
    }

    let mut transport = RaTransport::new(ra_stdin, ra_stdout);

    // 4. LSP initialize
    let mut ra_status = RaStatus::Initializing;
    eprintln!("[lsp-daemon] sending LSP initialize...");
    match send_initialize(&mut transport, workspace_root) {
        Ok(()) => {
            ra_status = RaStatus::Ready;
            eprintln!("[lsp-daemon] rust-analyzer initialized");
        }
        Err(e) => {
            eprintln!("[lsp-daemon] initialize failed: {e}");
            // Continue running — ra might still become ready
        }
    }

    // 5. Start file watcher
    let (fs_rx, _watcher) = match watcher::start_watcher(workspace_root) {
        Ok((watcher, rx)) => {
            eprintln!("[lsp-daemon] file watcher started");
            (Some(rx), Some(watcher))
        }
        Err(e) => {
            eprintln!("[lsp-daemon] file watcher failed: {e}, continuing without");
            (None, None)
        }
    };
    let mut debounce_buf = DebounceBuffer::new();

    // 6. Create FIFOs and lock file (AFTER ra init — preserves readiness invariant)
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
    let mut resp_fd = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&resp_path)
        .context("Failed to open response FIFO")?;

    eprintln!(
        "[lsp-daemon] listening on FIFOs in {}",
        daemon_dir.display()
    );

    // 7. Main loop
    let idle_timeout = Duration::from_secs(
        std::env::var("CARGO_BRIEF_LSP_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(IDLE_TIMEOUT_SECS),
    );
    let mut last_activity = Instant::now();
    let mut shutdown = false;

    loop {
        // Poll lsp.req for incoming data (POLLIN), EINTR-safe
        let mut pfd = libc::pollfd {
            fd: req_fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let n = poll_retry(&mut pfd, 100)?;

        if n > 0 && (pfd.revents & libc::POLLIN) != 0 {
            // Client sent data — switch to blocking, read full message
            set_nonblocking(&req_fd, false)?;
            let request: DaemonRequest = match read_message(&mut &req_fd) {
                Ok(req) => req,
                Err(e) => {
                    eprintln!("[lsp-daemon] failed to read request: {e}");
                    set_nonblocking(&req_fd, true)?;
                    continue;
                }
            };
            set_nonblocking(&req_fd, true)?;

            // Process request
            let response = handle_request(
                &request,
                ra_status,
                start_time,
                &mut shutdown,
                &mut transport,
                workspace_root,
            );

            // Write response
            if let Err(e) = write_message(&mut resp_fd, &response) {
                eprintln!("[lsp-daemon] failed to write response: {e}");
            }

            last_activity = Instant::now();
            if shutdown {
                break;
            }
        } else {
            // Timeout (n == 0) — check idle
            if last_activity.elapsed() > idle_timeout {
                eprintln!("[lsp-daemon] idle timeout, shutting down");
                break;
            }
        }

        // Drain FS events
        if let Some(rx) = &fs_rx {
            while let Ok(event) = rx.try_recv() {
                debounce_buf.push(event);
            }
            if debounce_buf.should_flush() {
                let events = debounce_buf.drain();
                let params = watcher::build_did_change_notification(&events);
                if let Err(e) =
                    transport.send_notification("workspace/didChangeWatchedFiles", params)
                {
                    eprintln!("[lsp-daemon] failed to notify ra of file changes: {e}");
                }
            }
        }

        // Check if ra is still alive
        match ra_child.try_wait() {
            Ok(Some(status)) => {
                eprintln!("[lsp-daemon] rust-analyzer exited: {status}");
                ra_status = RaStatus::Stopped;
                break;
            }
            Ok(None) => {} // still running
            Err(e) => {
                eprintln!("[lsp-daemon] failed to check ra status: {e}");
            }
        }
    }

    // 8. Cleanup
    if ra_status != RaStatus::Stopped {
        shutdown_ra(&mut transport);
    }
    // Wait for ra to exit
    let _ = ra_child.wait();

    std::fs::remove_file(&pid_path).ok();
    std::fs::remove_file(&req_path).ok();
    std::fs::remove_file(&resp_path).ok();
    std::fs::remove_file(&lock_path).ok();
    std::fs::remove_file(daemon_dir.join("lsp.log")).ok();
    // Try to remove the parent directory (only succeeds if empty)
    std::fs::remove_dir(daemon_dir).ok();

    eprintln!("[lsp-daemon] shut down cleanly");
    Ok(())
}
