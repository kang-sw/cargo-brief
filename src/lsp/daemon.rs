//! LSP daemon process: spawns rust-analyzer, accepts FIFO clients, handles idle timeout.

use std::collections::HashSet;
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
        "capabilities": {
            "window": { "workDoneProgress": true }
        },
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

/// Process a single ra notification for `$/progress` tracking.
/// Returns `Some(new_status)` when the indexing state changes.
fn process_ra_notification(
    msg: &serde_json::Value,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
) -> Option<RaStatus> {
    let method = msg.get("method")?.as_str()?;
    if method != "$/progress" {
        return None;
    }

    let params = msg.get("params")?;
    let token = params.get("token")?;
    let kind = params.get("value")?.get("kind")?.as_str()?;

    let token_key = token.to_string();

    match kind {
        "begin" => {
            active_progress.insert(token_key);
            *had_progress = true;
            Some(RaStatus::Indexing)
        }
        "end" => {
            active_progress.remove(&token_key);
            if *had_progress && active_progress.is_empty() {
                Some(RaStatus::Ready)
            } else {
                None
            }
        }
        _ => None, // "report" or unknown
    }
}

/// Uptime threshold (seconds) before assuming Ready if no `$/progress` seen.
const NO_PROGRESS_FALLBACK_SECS: u64 = 10;

/// Process one ra message: update progress state and reply to server requests.
fn handle_ra_message(
    msg: &serde_json::Value,
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
) {
    if let Some(new_status) = process_ra_notification(msg, active_progress, had_progress) {
        if *ra_status != new_status {
            eprintln!("[lsp-daemon] ra status: {new_status}");
            *ra_status = new_status;
        }
    }
    // Reply to server-initiated requests (e.g. window/workDoneProgress/create)
    if msg.get("id").is_some() && msg.get("method").is_some() {
        if let Some(id) = msg.get("id").cloned() {
            let _ = transport.send_raw_response(id, serde_json::json!(null));
        }
    }
}

/// Check fallback: no progress tokens ever seen, uptime > threshold → assume Ready.
fn check_no_progress_fallback(ra_status: &mut RaStatus, had_progress: bool, start_time: Instant) {
    if !had_progress
        && *ra_status == RaStatus::Initializing
        && start_time.elapsed().as_secs() > NO_PROGRESS_FALLBACK_SECS
    {
        eprintln!("[lsp-daemon] ra status: ready (no progress reported, fallback)");
        *ra_status = RaStatus::Ready;
    }
}

/// Drain all available ra stdout messages. Updates ra_status and replies
/// to server-initiated requests. Returns true if any messages were read.
fn drain_ra_messages(
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
    start_time: Instant,
) -> Result<bool> {
    let mut any_read = false;
    loop {
        // Check BufReader internal buffer first (minor optimization)
        if !transport.has_buffered_data() {
            let mut pfd = libc::pollfd {
                fd: transport.stdout_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let n = poll_retry(&mut pfd, 0)?;
            if n == 0 {
                break;
            }
        }
        match transport.read_message() {
            Ok(msg) => {
                any_read = true;
                handle_ra_message(&msg, transport, ra_status, active_progress, had_progress);
            }
            Err(e) => {
                eprintln!("[lsp-daemon] ra stdout read error: {e}");
                break;
            }
        }
    }
    check_no_progress_fallback(ra_status, *had_progress, start_time);
    Ok(any_read)
}

/// Default timeout for waiting for ra to finish indexing before a query.
const READY_TIMEOUT_SECS: u64 = 60;

/// After reaching Ready, keep draining for this long to catch re-indexing cycles.
/// ra goes through multiple begin/end phases (config load, workspace load,
/// proc-macro expansion, symbol indexing). Each time status returns to Ready,
/// the settle timer resets. Only when Ready persists for the full settle period
/// without any new progress begin do we consider indexing truly complete.
const SETTLE_MS: u64 = 5000;

/// Block until ra finishes indexing (status becomes Ready) or timeout.
/// After each Ready transition, continues draining for SETTLE_MS to catch
/// ra re-entering Indexing (common during multi-phase startup).
fn wait_for_ready(
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
    start_time: Instant,
    timeout: Duration,
) -> Result<()> {
    if *ra_status == RaStatus::Ready {
        return Ok(());
    }

    let wait_start = Instant::now();
    // Tracks when we last entered Ready. None = not Ready yet.
    let mut ready_since: Option<Instant> = None;
    eprintln!("[lsp-daemon] waiting for rust-analyzer to finish indexing...");

    loop {
        if wait_start.elapsed() > timeout {
            bail!(
                "rust-analyzer is still indexing (waited {}s). Try again shortly.",
                timeout.as_secs()
            );
        }

        // Track Ready/Indexing transitions
        if *ra_status == RaStatus::Ready {
            if ready_since.is_none() {
                ready_since = Some(Instant::now());
            }
            // Settle period elapsed — indexing is truly done
            if ready_since.is_some_and(|t| t.elapsed().as_millis() >= SETTLE_MS as u128) {
                return Ok(());
            }
        } else if ready_since.is_some() {
            // Back to Indexing — reset settle timer
            ready_since = None;
        }

        // Poll ra stdout — shorter interval during settle for responsiveness
        let poll_timeout = if ready_since.is_some() { 100 } else { 500 };
        if !transport.has_buffered_data() {
            let mut pfd = libc::pollfd {
                fd: transport.stdout_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let n = poll_retry(&mut pfd, poll_timeout)?;
            if n == 0 {
                check_no_progress_fallback(ra_status, *had_progress, start_time);
                continue;
            }
        }

        match transport.read_message() {
            Ok(msg) => {
                handle_ra_message(&msg, transport, ra_status, active_progress, had_progress);
            }
            Err(e) => {
                eprintln!("[lsp-daemon] ra stdout read error during wait: {e}");
                bail!("rust-analyzer stdout closed during indexing wait");
            }
        }
    }
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
    let mut active_progress: HashSet<String> = HashSet::new();
    let mut had_progress = false;

    eprintln!("[lsp-daemon] sending LSP initialize...");
    match send_initialize(&mut transport, workspace_root) {
        Ok(()) => {
            eprintln!("[lsp-daemon] rust-analyzer initialized, waiting for indexing...");
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
    let ready_timeout = Duration::from_secs(
        std::env::var("CARGO_BRIEF_LSP_READY_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(READY_TIMEOUT_SECS),
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

            // Wait for ra to finish indexing before query dispatch
            let is_query = matches!(
                request,
                DaemonRequest::References { .. }
                    | DaemonRequest::BlastRadius { .. }
                    | DaemonRequest::CallHierarchy { .. }
            );
            if is_query && ra_status != RaStatus::Ready {
                if let Err(e) = wait_for_ready(
                    &mut transport,
                    &mut ra_status,
                    &mut active_progress,
                    &mut had_progress,
                    start_time,
                    ready_timeout,
                ) {
                    let response = DaemonResponse::Error {
                        message: format!("{e}"),
                    };
                    if let Err(we) = write_message(&mut resp_fd, &response) {
                        eprintln!("[lsp-daemon] failed to write error response: {we}");
                    }
                    continue;
                }
            }

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

        // Drain ra stdout (progress notifications, server requests)
        if ra_status != RaStatus::Stopped {
            if let Err(e) = drain_ra_messages(
                &mut transport,
                &mut ra_status,
                &mut active_progress,
                &mut had_progress,
                start_time,
            ) {
                eprintln!("[lsp-daemon] drain error: {e}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn progress_begin(token: serde_json::Value) -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": token, "value": { "kind": "begin", "title": "Indexing" } }
        })
    }

    fn progress_end(token: serde_json::Value) -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": token, "value": { "kind": "end" } }
        })
    }

    fn progress_report(token: serde_json::Value) -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": token, "value": { "kind": "report", "percentage": 50 } }
        })
    }

    #[test]
    fn progress_begin_returns_indexing() {
        let mut set = HashSet::new();
        let mut had = false;
        let result = process_ra_notification(&progress_begin(json!("tok1")), &mut set, &mut had);
        assert_eq!(result, Some(RaStatus::Indexing));
        assert!(had);
        assert!(set.contains(&json!("tok1").to_string()));
    }

    #[test]
    fn progress_end_last_token_returns_ready() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!("tok1")), &mut set, &mut had);
        let result = process_ra_notification(&progress_end(json!("tok1")), &mut set, &mut had);
        assert_eq!(result, Some(RaStatus::Ready));
        assert!(set.is_empty());
    }

    #[test]
    fn progress_end_not_last_returns_none() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!("a")), &mut set, &mut had);
        process_ra_notification(&progress_begin(json!("b")), &mut set, &mut had);
        let result = process_ra_notification(&progress_end(json!("a")), &mut set, &mut had);
        assert_eq!(result, None); // "b" still active
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn progress_report_returns_none() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!("tok1")), &mut set, &mut had);
        let result = process_ra_notification(&progress_report(json!("tok1")), &mut set, &mut had);
        assert_eq!(result, None);
    }

    #[test]
    fn non_progress_notification_returns_none() {
        let mut set = HashSet::new();
        let mut had = false;
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {}
        });
        assert_eq!(process_ra_notification(&msg, &mut set, &mut had), None);
    }

    #[test]
    fn progress_begin_when_had_progress_already_true() {
        let mut set = HashSet::new();
        let mut had = true; // already true from prior cycle
        let result = process_ra_notification(&progress_begin(json!("tok2")), &mut set, &mut had);
        assert_eq!(result, Some(RaStatus::Indexing));
    }

    #[test]
    fn two_full_begin_end_cycles() {
        let mut set = HashSet::new();
        let mut had = false;
        // Cycle 1
        process_ra_notification(&progress_begin(json!("a")), &mut set, &mut had);
        let r = process_ra_notification(&progress_end(json!("a")), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Ready));
        // Cycle 2
        let r = process_ra_notification(&progress_begin(json!("b")), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Indexing));
        let r = process_ra_notification(&progress_end(json!("b")), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Ready));
    }

    #[test]
    fn unknown_token_in_end_is_noop() {
        let mut set = HashSet::new();
        let mut had = false;
        // End without prior begin — had_progress is false, so no Ready
        let r = process_ra_notification(&progress_end(json!("unknown")), &mut set, &mut had);
        assert_eq!(r, None);
    }

    #[test]
    fn integer_token_normalized() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!(1)), &mut set, &mut had);
        assert!(set.contains("1"));
        let r = process_ra_notification(&progress_end(json!(1)), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Ready));
    }

    #[test]
    fn string_token_normalized() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!("hello")), &mut set, &mut had);
        // String tokens include quotes in to_string()
        assert!(set.contains(&json!("hello").to_string()));
        let r = process_ra_notification(&progress_end(json!("hello")), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Ready));
    }

    #[test]
    fn mixed_int_string_tokens() {
        let mut set = HashSet::new();
        let mut had = false;
        process_ra_notification(&progress_begin(json!(42)), &mut set, &mut had);
        process_ra_notification(&progress_begin(json!("foo")), &mut set, &mut had);
        assert_eq!(set.len(), 2);
        process_ra_notification(&progress_end(json!(42)), &mut set, &mut had);
        assert_eq!(set.len(), 1);
        let r = process_ra_notification(&progress_end(json!("foo")), &mut set, &mut had);
        assert_eq!(r, Some(RaStatus::Ready));
    }
}
