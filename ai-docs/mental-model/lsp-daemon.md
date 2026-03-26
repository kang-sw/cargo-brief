# LSP Daemon

## Entry Points
- `src/lsp/mod.rs` — `run_lsp_command()`: validates remote mode is off, loads metadata, dispatches touch/stop/status.
- `src/lsp/daemon.rs` — `run_daemon_from_args()`: called from `main.rs` early-exit path before clap parsing; `run_daemon()` is the main daemon loop.
- `src/lsp/client.rs` — `ensure_daemon()`, `daemon_dir()`: client-side entry points for spawning and locating daemons.
- `src/lsp/watcher.rs` — `start_watcher()`: sets up notify-based FS watcher; `DebounceBuffer`: 300ms batching; `build_did_change_notification()`: LSP params construction.

## Module Contracts
- `run_lsp_command()` guarantees: calls `resolve::load_cargo_metadata()` directly — does NOT build a `PipelineContext`, does NOT use `PipelineContext` anywhere. This is by design. Returns `Result<()>`, not `Result<String>` — it prints directly to stderr/stdout.
- `daemon_dir(workspace_root)` guarantees: canonicalizes the workspace root path before hashing. Two clients with symlinked paths pointing to the same directory will share the same daemon. The hash is FNV-1a 64-bit, hex-encoded to 16 chars. Directory is `$XDG_RUNTIME_DIR/cargo-brief/<hash>` (falls back to `$TMPDIR/cargo-brief/<hash>`).
- `ensure_daemon()` guarantees: first pings existing socket (returns immediately if live); checks stale PID file via `kill(pid, 0)`; spawns daemon via re-exec with `__lsp-daemon` prefix args; polls socket up to 120 seconds with exponential backoff (50ms → 500ms). The 120-second wait is synchronous — the calling process blocks.
- `run_daemon()` guarantees: (1) writes PID file before binding socket; (2) LSP initialize completes before the UDS socket is bound and accepting — clients that connect will always find an initialized (or failed) ra instance, never one mid-handshake; (3) file watcher is started after LSP initialize and before the UDS bind — watcher failure is non-fatal (daemon continues without FS watching); (4) idle timeout defaults to 600s, overridable via `CARGO_BRIEF_LSP_TIMEOUT` env var; (5) cleans up socket + PID files on exit (both normal and idle timeout; but NOT on panic or kill -9).
- `handle_client()` guarantees: handles exactly one request per connection — no persistent per-client state. The UDS framing is 4-byte LE length-prefix + JSON body, max 1 MiB per message.
- `RaTransport` guarantees: `send_request_and_wait()` skips notifications (messages with no `"id"` field) and reads up to 10,000 messages before giving up. ra sends many progress notifications during initialization; this is the primary use of the skip logic.
- The entire `lsp` module is `#[cfg(unix)]` — it does not compile on non-Unix. `BriefCommand::Lsp` and `cli::LspArgs` are NOT `#[cfg(unix)]` gated (they compile everywhere), but `lib.rs::run_lsp_command`, `pub mod lsp`, and the two references in `main.rs` (early-exit line and dispatch arm) are NOT gated either. As a result, the current code does not build for non-Unix targets.

## Coupling
- `main.rs` early-exit: `args[1] == "__lsp-daemon"` is checked BEFORE clap parsing. This means the `__lsp-daemon` sentinel must always be in position 1 (not 0 and not 2+). `spawn_daemon()` passes it as the first argument after the binary name — this positional contract must not be broken.
- `resolve::CargoMetadataInfo.workspace_root` was added to support this module. All callers of `load_cargo_metadata()` receive this field; it is safe to ignore in existing code, but adding it to the struct means test helpers in `resolve.rs` must initialize it (they do — set to `PathBuf::from("/tmp")`).
- `daemon_dir()` uses the CANONICAL workspace root path as hash input. If `canonicalize()` fails (e.g., path does not exist yet), the raw path is used as fallback — two clients may hash to different dirs if one succeeds at canonicalization and the other does not.
- PID file is written before the socket is bound. A crash between PID write and socket bind leaves a PID file that points to a dead process. `ensure_daemon()` detects this via `process_alive()` and cleans up — but only if the PID is a valid u32 and the file is readable. A corrupted PID file blocks daemon restart until manually removed.
- `start_watcher()` spawns an OS-level thread internally (notify `RecommendedWatcher` uses a background thread). The `RecommendedWatcher` handle is stored as `_watcher: Option<RecommendedWatcher>` in `run_daemon()` stack frame — dropping it on daemon exit stops the OS thread and closes the channel. `fs_rx: Option<Receiver<FileEvent>>` bridges FS events to the main loop via `try_recv()` on each iteration.

## Extension Points & Change Recipes
- **Add a new daemon command** (e.g., `Query`): Add variant to `DaemonRequest`/`DaemonResponse` in `protocol.rs`, add match arm in `daemon.rs::handle_client()`, add the client-side call in `client.rs` or `mod.rs`. The UDS protocol is JSON-serialized — adding a new enum variant is backward-compatible only if the daemon and client versions match (no versioning mechanism exists yet). Note: file-change events arrive via `fs_rx` (an `mpsc::Receiver` drained on each main-loop iteration), not via UDS client connections — `handle_client()` is only called for UDS connections and has no visibility into FS events.
- **Add a new `LspCommand` subcommand**: Add variant to `cli::LspCommand`, add match arm in `lsp::mod::run_lsp_command()`. Touch `cli.rs`, `lsp/mod.rs`. No changes to `daemon.rs` needed unless the command requires a new `DaemonRequest`.
- **Change idle timeout default**: Change `IDLE_TIMEOUT_SECS` in `daemon.rs`. The env var `CARGO_BRIEF_LSP_TIMEOUT` already overrides it at runtime.
- **Change debounce window**: Change `DEBOUNCE_MS` in `watcher.rs`. The value is a `u128` compared against `elapsed().as_millis()` — the window is measured from the first event in a batch, not from the most recent event.

## Common Mistakes
- Adding a new `DaemonRequest` variant without a matching arm in `daemon.rs::handle_client()` — the match is exhaustive, so this is a compile error, not silent. But sending a new request to an OLD daemon silently fails with a JSON deserialization error on the daemon side (daemon reads malformed message, logs error, closes connection).
- The `lsp` module output goes to stderr (via `eprintln!`), not stdout — `run_lsp_command()` returns `Result<()>`, and the dispatch arm in `main.rs` does not print anything. Asserting on stdout in tests for LSP commands will always get empty output.
- Calling `run_lsp_command()` on Windows — the function itself is `#[cfg(unix)]` and won't compile. The `BriefCommand::Lsp` dispatch arm in `main.rs` references `cargo_brief::run_lsp_command` which is also `#[cfg(unix)]`. On non-Unix, this arm needs to be guarded or the build will fail.
- `send_request_and_wait()` discards notifications (no `"id"` field) silently. If ra sends an error as a notification rather than a response (which is non-standard but possible), it will be dropped, and the 10,000-message loop will eventually time out with a generic "Timed out" error rather than the underlying ra error.
- `shutdown_ra()` waits for the shutdown response with a bounded 10-message read. If ra sends many pending notifications before acknowledging shutdown, the function may miss the response and call `exit` anyway — this is acceptable (ra handles exit regardless of shutdown ACK).

## Technical Debt
- No versioning in the UDS protocol. Old daemon + new client (or vice versa) will see JSON deserialization errors — the daemon logs the error and drops the connection, the client gets an `anyhow::Error` from `read_message`. No diagnostic indicates a version mismatch.
- `ra_status` in `daemon.rs` is updated only at initialization and ra-exit — there is no mechanism to detect if ra enters a broken state mid-session (e.g., ra becomes unresponsive without exiting). The status will report `Ready` indefinitely.
- `run_daemon()` does not redirect its own stderr to a log file — all `eprintln!` output (including `[ra-stderr]` lines) goes to the process stderr, which was set to `Stdio::null()` by `spawn_daemon()`. These logs are silently discarded in production. Only visible when starting the daemon manually or in tests.
- No capability negotiation in `send_initialize()` — `"capabilities": {}` is sent. If ra requires specific client capabilities for a future query type, the initialize call will need updating.
- The UDS message size limit (1 MiB) is checked only on read, not on write. A client sending a pathologically large request will be rejected by the daemon but the client's `write_message()` will succeed silently.
