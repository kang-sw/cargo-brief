# LSP Daemon Bootstrap

## Context

Ticket `260326-feat-lsp-daemon-bootstrap`: Add a persistent rust-analyzer
daemon managed by cargo-brief for ground-truth semantic code analysis.

This is Phase 1 of the LSP integration epic (`260326-feat-lsp-daemon`).
Scope: daemon infrastructure, UDS communication, ra spawn + LSP init,
`touch`/`stop`/`status` CLI commands. Query commands and file watcher
are separate tickets.

### Key decisions from discussion

- **One daemon per Cargo workspace root** (hash of canonical path → socket dir)
- **All `lsp` commands auto-touch** (ensure daemon running before query)
- **Fully synchronous daemon** — single-threaded, non-blocking UDS accept
  with poll loop. No async runtime. One client at a time is fine since
  cargo-brief invocations are sequential in practice.
- **Socket location**: `$XDG_RUNTIME_DIR/cargo-brief/{hash}/` (fallback:
  `$TMPDIR/cargo-brief/{hash}/`). Avoids polluting workspace.
- **Idle timeout**: 10 minutes default, reset on each client interaction.
- **ra discovery**: `rustup which rust-analyzer`, fallback `rust-analyzer` on PATH.
- **LSP transport**: Hand-rolled Content-Length framing (~100 lines).
  Use `lsp-types` crate for typed request/response structures.

### Rejected alternatives

- **Async runtime (tokio/mio)**: Overkill. Daemon handles one client at a
  time; a poll loop with `set_nonblocking` is sufficient.
- **`lsp-server` crate**: Designed for server implementations, not clients.
  Transport framing is simple enough to hand-roll.
- **Feature-gating**: Deferred. `lsp-types` is a proc-macro-free types crate
  (~200KB). If binary size becomes a concern, add a cargo feature later.

## Relevant Files

### Existing (to modify)

- `src/cli.rs` — Add `LspArgs`, `LspCommand` enum. Pattern: follow `TsArgs`/`CodeArgs`
  (flat fields, `GlobalArgs` flattened, `manifest_path` option).
  `BriefCommand` enum at line 73 gets new `Lsp(LspArgs)` variant.
- `src/main.rs` — Add dispatch arm in match block (lines 10-38).
  LSP is special: `touch`/`stop`/`status` don't return output strings,
  so dispatch calls `run_lsp_command()` directly (no `print!`).
- `src/lib.rs` — Add `pub mod lsp;` declaration (line 12). Add
  `pub fn run_lsp_command(args: &LspArgs, remote: &RemoteOpts) -> Result<()>`
  thin wrapper. Note: returns `()` not `String` — side-effect oriented.
- `src/resolve.rs` — Add `workspace_root: PathBuf` field to
  `CargoMetadataInfo` (line 7-19). Extract from `metadata["workspace_root"]`
  in `load_cargo_metadata()`. This is needed to key the daemon socket path.
- `Cargo.toml` — Add `lsp-types` dependency.

### New files (to create)

- `src/lsp/mod.rs` — Module entry. Re-exports `run_lsp_command()`.
  Public interface for the lsp module.
- `src/lsp/daemon.rs` — Daemon process entry point and main loop.
  Spawns ra, accepts UDS clients, handles idle timeout.
- `src/lsp/client.rs` — Client-side logic: `ensure_daemon()` (check PID,
  spawn if needed, connect), `send_request()`, `receive_response()`.
- `src/lsp/protocol.rs` — UDS message types (serde): `DaemonRequest`,
  `DaemonResponse`. Framing: length-prefixed JSON over UDS.
- `src/lsp/transport.rs` — LSP JSON-RPC framing: write Content-Length
  header + JSON to ra stdin, read Content-Length + JSON from ra stdout.
  Request ID tracking.

## Conventions (verified from code)

### CLI args pattern (from TsArgs, CodeArgs)

```rust
#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    #[command(subcommand)]
    pub command: LspCommand,

    #[command(flatten)]
    pub global: GlobalArgs,     // --toolchain, --verbose

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,
}
```

`LspCommand` is a nested `#[derive(Subcommand)]` enum — clap handles
this natively (already 2 levels deep: `cargo → brief → api/search/...`).

### main.rs dispatch pattern

```rust
BriefCommand::Lsp(args) => {
    cargo_brief::run_lsp_command(args, &remote)?;
}
```

No `print!` — LSP commands produce stderr progress + side effects,
not stdout content (except `status` which prints to stdout).
Precedent: `BriefCommand::Clean` already returns `Result<()>` (no String).

### Module structure convention

`src/lsp/mod.rs` contains doc comments + public re-exports only.
Implementation in sibling files. Files split at ~300 lines.

### Error handling

- `anyhow::Result` with `.with_context()` at each step
- Actionable error messages (e.g., "rust-analyzer not found. Install via: rustup component add rust-analyzer")

## Implementation Steps

### Step 1: Add `workspace_root` to CargoMetadataInfo

**File:** `src/resolve.rs`

Add `workspace_root: PathBuf` to the `CargoMetadataInfo` struct (after
`target_dir`). Extract it in `load_cargo_metadata()` from
`metadata["workspace_root"]` — the same pattern as `target_directory`
extraction.

Also update the two test helper constructors in `resolve.rs` tests
(`test_metadata` and `test_metadata_with_dir`) — they use struct literal
syntax and will fail to compile without the new field.

### Step 2: Add CLI types

**File:** `src/cli.rs`

Add after `CleanArgs`:

```rust
/// Arguments for the `lsp` subcommand.
#[derive(Args, Debug, Clone)]
pub struct LspArgs {
    #[command(subcommand)]
    pub command: LspCommand,

    #[command(flatten)]
    pub global: GlobalArgs,

    /// Path to Cargo.toml
    #[arg(long, help_heading = "Local Workspace")]
    pub manifest_path: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum LspCommand {
    /// Ensure LSP daemon is running (pre-warm rust-analyzer)
    Touch,
    /// Stop the LSP daemon
    Stop,
    /// Show LSP daemon status
    Status,
}
```

Add `Lsp(LspArgs)` variant to `BriefCommand` enum.
Add `LspArgs` to the imports in `lib.rs` line 26-28.

### Step 3: Add dispatch in main.rs + lib.rs entry point

**File:** `src/main.rs` — Add match arm:
```rust
BriefCommand::Lsp(args) => {
    cargo_brief::run_lsp_command(args, &remote)?;
}
```

**File:** `src/lib.rs` — Add:
```rust
pub mod lsp;

pub fn run_lsp_command(args: &cli::LspArgs, remote: &RemoteOpts) -> Result<()> {
    lsp::run_lsp_command(args, remote)
}
```

### Step 4: Create UDS protocol types

**File:** `src/lsp/protocol.rs`

Define message types for client ↔ daemon communication:

```rust
#[derive(Serialize, Deserialize)]
pub enum DaemonRequest {
    Ping,
    Stop,
    Status,
    // Future: Query { method: String, params: serde_json::Value }
}

#[derive(Serialize, Deserialize)]
pub enum DaemonResponse {
    Ok { message: String },
    Status { pid: u32, ra_status: RaStatus, uptime_secs: u64 },
    Error { message: String },
}

#[derive(Serialize, Deserialize)]
pub enum RaStatus {
    Initializing,
    Ready,
    Stopped,
}
```

Framing functions:
- `write_message(stream: &mut UnixStream, msg: &impl Serialize) -> Result<()>`
  — write 4-byte LE length prefix + JSON bytes
- `read_message<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T>`
  — read 4-byte length, read N bytes, deserialize

### Step 5: Create LSP transport layer

**File:** `src/lsp/transport.rs`

JSON-RPC over stdio to rust-analyzer:

```rust
pub struct RaTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i32,
}

impl RaTransport {
    pub fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<i32>;
    pub fn read_message(&mut self) -> Result<serde_json::Value>;
    pub fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<()>;
}
```

Content-Length framing: write `Content-Length: N\r\n\r\n{json}`, read
by parsing header lines until empty line, then read N bytes.

Add `send_request_and_wait(&mut self, method, params) -> Result<Value>`
that loops on `read_message()`, skipping notifications (messages without
`id` field — e.g., `window/logMessage`, `$/progress` from ra) until a
response with matching `id` is received.

### Step 6: Create daemon process

**File:** `src/lsp/daemon.rs`

Core structure:

```rust
pub fn run_daemon(workspace_root: &Path, socket_path: &Path, pid_path: &Path) -> Result<()> {
    // 1. Discover ra binary
    let ra_bin = discover_ra_binary()?;

    // 2. Spawn ra subprocess
    let mut ra_child = Command::new(&ra_bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())  // or inherit for debugging
        .spawn()?;
    let mut transport = RaTransport::new(ra_child.stdin.take()..., ra_child.stdout.take()...);

    // 3. LSP initialize
    send_initialize(&mut transport, workspace_root)?;
    // Wait for initialized response

    // 4. Write PID file
    std::fs::write(pid_path, std::process::id().to_string())?;

    // 5. Bind UDS listener
    let listener = UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;

    // 6. Main loop
    let mut last_activity = Instant::now();
    let mut shutdown = false;
    let idle_timeout = Duration::from_secs(600); // 10 min

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = handle_client(stream, &mut transport, &mut shutdown) {
                    eprintln!("[lsp-daemon] client error: {e}");
                }
                last_activity = Instant::now();
                if shutdown { break; }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last_activity.elapsed() > idle_timeout {
                    break; // idle shutdown
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => eprintln!("[lsp-daemon] accept error: {e}"),
        }

        // Check if ra is still alive
        if let Some(status) = ra_child.try_wait()? {
            eprintln!("[lsp-daemon] rust-analyzer exited: {status}");
            break;
        }
    }

    // 7. Cleanup
    shutdown_ra(&mut transport);
    std::fs::remove_file(pid_path).ok();
    std::fs::remove_file(socket_path).ok();
    Ok(())
}
```

`discover_ra_binary()`: Try `rustup which rust-analyzer` first, fall back
to `which rust-analyzer` on PATH. Actionable error if neither found.

`send_initialize()`: Send LSP `initialize` request with
`InitializeParams { root_uri, capabilities, ... }`. Wait for response.
Then send `initialized` notification.

`handle_client(stream, transport, shutdown: &mut bool)`: Read
`DaemonRequest` from UDS stream, dispatch:
- `Ping` → respond `Ok`
- `Stop` → respond `Ok`, set `*shutdown = true`
- `Status` → respond with pid, ra status, uptime

Client errors (broken pipe, malformed JSON) are logged and do NOT
propagate — daemon stays alive. Only `Stop` or idle timeout terminates.

### Step 7: Create client module

**File:** `src/lsp/client.rs`

```rust
/// Socket/PID directory for a workspace.
pub fn daemon_dir(workspace_root: &Path) -> PathBuf {
    let hash = short_hash(workspace_root);
    runtime_dir().join("cargo-brief").join(hash)
}

/// Ensure daemon is running, return connected UDS stream.
pub fn ensure_daemon(workspace_root: &Path, verbose: bool) -> Result<UnixStream> {
    let dir = daemon_dir(workspace_root);
    let sock = dir.join("lsp.sock");
    let pid_file = dir.join("lsp.pid");

    // Try connecting to existing daemon
    if let Some(stream) = try_connect(&sock) {
        return Ok(stream);
    }

    // Spawn daemon process
    std::fs::create_dir_all(&dir)?;
    spawn_daemon(workspace_root, &sock, &pid_file, verbose)?;

    // Wait for socket to become available (poll with backoff)
    wait_for_socket(&sock, Duration::from_secs(120))
}

fn try_connect(sock: &Path) -> Option<UnixStream> {
    let stream = UnixStream::connect(sock).ok()?;
    // Send Ping, verify response
    ...
}

fn spawn_daemon(workspace_root: &Path, sock: &Path, pid: &Path, verbose: bool) -> Result<()> {
    // Re-exec cargo-brief with internal __daemon flag
    // Detach from parent process group
    use std::os::unix::process::CommandExt;
    Command::new(std::env::current_exe()?)
        .args(["__lsp-daemon",
               "--workspace-root", workspace_root.to_str().unwrap(),
               "--socket", sock.to_str().unwrap(),
               "--pid-file", pid.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())  // or log file
        .stderr(Stdio::inherit())  // forward progress to parent stderr
        .process_group(0)  // detach from parent process group (survives terminal close)
        .spawn()?;
    Ok(())
}

fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

fn short_hash(path: &Path) -> String {
    // FNV-1a 64-bit hash of canonical path bytes, hex-encoded.
    // Must be deterministic across Rust versions (NOT DefaultHasher).
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
```

### Step 8: Wire module entry point

**File:** `src/lsp/mod.rs`

```rust
//! LSP daemon management for semantic code analysis via rust-analyzer.
//!
//! Provides `cargo brief lsp` subcommands: `touch`, `stop`, `status`.
//! The daemon spawns rust-analyzer as a background process, communicates
//! via LSP over stdio, and accepts client queries via Unix domain socket.

mod client;
mod daemon;
mod protocol;
mod transport;

use anyhow::{Result, Context};
use crate::cli::{LspArgs, LspCommand, RemoteOpts};
use crate::resolve;

pub fn run_lsp_command(args: &LspArgs, remote: &RemoteOpts) -> Result<()> {
    if remote.crates {
        anyhow::bail!("LSP commands do not support remote crate mode (-C)");
    }

    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    match &args.command {
        LspCommand::Touch => cmd_touch(&metadata.workspace_root, args.global.verbose),
        LspCommand::Stop => cmd_stop(&metadata.workspace_root),
        LspCommand::Status => cmd_status(&metadata.workspace_root),
    }
}
```

### Step 9: Internal daemon entry point in main.rs

The daemon process is launched by re-exec'ing `cargo-brief` with a hidden
`__lsp-daemon` argument. In `main.rs`, detect this before clap parsing:

```rust
fn main() -> anyhow::Result<()> {
    // Hidden daemon entry point (not a clap subcommand).
    // Check only args[1] to avoid false positives from workspace paths.
    if std::env::args().nth(1).as_deref() == Some("__lsp-daemon") {
        return cargo_brief::lsp::daemon::run_daemon_from_args();
    }

    // Normal clap dispatch...
}
```

This avoids exposing `__lsp-daemon` in help text while keeping the
re-exec pattern simple.

`run_daemon_from_args()` parses `--workspace-root`, `--socket`,
`--pid-file` from remaining args (manual parsing or a small clap struct
hidden from the main CLI), then delegates to `run_daemon()`.

### Step 10: Add lsp-types dependency

**File:** `Cargo.toml`

```toml
lsp-types = "0.97"
```

Use for `InitializeParams`, `InitializeResult`, `ServerCapabilities`,
and future request/response types. Version 0.97 matches LSP 3.17.

### Step 11: Integration testing

LSP tests require a live `rust-analyzer` binary and are process-level
(not in-process String capture like other pipelines). Use a separate
test file `tests/lsp_integration.rs`:

- `lsp_touch_and_stop`: Start daemon via `touch`, verify `status` shows
  running, `stop` terminates. Skip if ra not installed.
- `lsp_double_touch`: Second `touch` is no-op (same PID).
- `lsp_idle_timeout`: Start daemon with short timeout (env override),
  verify auto-exit. (May be flaky — consider unit-testing the idle logic
  instead.)

Mark all LSP tests with `#[ignore]` by default (require ra).
Run explicitly with `cargo test -- --ignored lsp`.

Helper: `fn ra_available() -> bool` checks `rustup which rust-analyzer`
or `which rust-analyzer`. Tests call this and `return` early if false.

## Testing Strategy

- **Unit tests** in `lsp/protocol.rs`: Framing roundtrip (write + read message).
- **Unit tests** in `lsp/transport.rs`: Content-Length framing parse/write.
- **Integration tests** in `tests/lsp_integration.rs`: `touch` → `status` →
  `stop` lifecycle (requires rust-analyzer binary; `#[ignore]` gated).
  Separate from `tests/integration.rs` because LSP tests are process-level
  (spawn daemon, check PID files, etc.), not in-process String capture.
- **Manual verification**: `cargo brief lsp touch` on this project,
  observe ra startup on stderr, `lsp status` shows ready, idle timeout
  triggers after 10min.

## Success Criteria

1. `cargo brief lsp touch` spawns daemon + ra, prints status on stderr
2. `cargo brief lsp status` shows PID, ra state (initializing/ready), uptime
3. `cargo brief lsp stop` gracefully terminates daemon + ra, cleans PID/socket
4. Second `touch` detects existing daemon (no re-spawn)
5. Idle timeout (10min) auto-terminates daemon
6. Missing ra binary → actionable error message
7. Daemon crash → next command detects stale PID, cleans up, re-spawns
