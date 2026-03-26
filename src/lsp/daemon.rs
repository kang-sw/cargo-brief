//! LSP daemon process: spawns rust-analyzer, accepts UDS clients, handles idle timeout.

use std::io::BufRead;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::protocol::{DaemonRequest, DaemonResponse, RaStatus, read_message, write_message};
use super::transport::RaTransport;

/// Default idle timeout: 10 minutes.
const IDLE_TIMEOUT_SECS: u64 = 600;

/// Entry point for the re-exec'd daemon process. Parses args manually.
pub fn run_daemon_from_args() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let mut workspace_root = None;
    let mut socket = None;
    let mut pid_file = None;

    let mut i = 2; // skip binary name and "__lsp-daemon"
    while i < args.len() {
        match args[i].as_str() {
            "--workspace-root" | "--socket" | "--pid-file" => {
                let flag = &args[i];
                i += 1;
                let value = args
                    .get(i)
                    .with_context(|| format!("Missing value for {flag}"))?;
                match flag.as_str() {
                    "--workspace-root" => workspace_root = Some(PathBuf::from(value)),
                    "--socket" => socket = Some(PathBuf::from(value)),
                    "--pid-file" => pid_file = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => bail!("Unknown daemon argument: {other}"),
        }
        i += 1;
    }

    let workspace_root = workspace_root.context("Missing --workspace-root")?;
    let socket = socket.context("Missing --socket")?;
    let pid_file = pid_file.context("Missing --pid-file")?;

    run_daemon(&workspace_root, &socket, &pid_file)
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

/// Handle a single client connection on the UDS.
fn handle_client(
    mut stream: std::os::unix::net::UnixStream,
    ra_status: RaStatus,
    start_time: Instant,
    shutdown: &mut bool,
) -> Result<()> {
    // Set a read timeout to avoid blocking forever on malformed clients
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let request: DaemonRequest = read_message(&mut stream)?;

    let response = match request {
        DaemonRequest::Ping => DaemonResponse::Ok {
            message: "pong".to_string(),
        },
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
    };

    write_message(&mut stream, &response)?;
    Ok(())
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
pub fn run_daemon(workspace_root: &Path, socket_path: &Path, pid_path: &Path) -> Result<()> {
    let start_time = Instant::now();

    // Clean up stale socket
    if socket_path.exists() {
        std::fs::remove_file(socket_path).ok();
    }

    // 1. Write PID file early to prevent double-spawn race
    std::fs::write(pid_path, std::process::id().to_string()).context("Failed to write PID file")?;

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

    // 5. Bind UDS listener
    let listener = UnixListener::bind(socket_path).context("Failed to bind UDS listener socket")?;
    listener
        .set_nonblocking(true)
        .context("Failed to set listener non-blocking")?;

    eprintln!("[lsp-daemon] listening on {}", socket_path.display());

    // 6. Main loop
    let idle_timeout = Duration::from_secs(
        std::env::var("CARGO_BRIEF_LSP_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(IDLE_TIMEOUT_SECS),
    );
    let mut last_activity = Instant::now();
    let mut shutdown = false;

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = handle_client(stream, ra_status, start_time, &mut shutdown) {
                    eprintln!("[lsp-daemon] client error: {e}");
                }
                last_activity = Instant::now();
                if shutdown {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last_activity.elapsed() > idle_timeout {
                    eprintln!("[lsp-daemon] idle timeout, shutting down");
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("[lsp-daemon] accept error: {e}");
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

    // 7. Cleanup
    if ra_status != RaStatus::Stopped {
        shutdown_ra(&mut transport);
    }
    // Wait for ra to exit
    let _ = ra_child.wait();

    std::fs::remove_file(pid_path).ok();
    std::fs::remove_file(socket_path).ok();
    // Try to remove the parent directory (only succeeds if empty)
    if let Some(parent) = socket_path.parent() {
        std::fs::remove_dir(parent).ok();
    }

    eprintln!("[lsp-daemon] shut down cleanly");
    Ok(())
}
