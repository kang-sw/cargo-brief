# LSP Daemon File Watcher Integration

## Context
- **Ticket:** `260326-feat-lsp-file-watcher` (sub-ticket of `260326-feat-lsp-daemon`)
- **Prerequisite:** `260326-feat-lsp-daemon-bootstrap` (done) — daemon process, UDS,
  PID, ra spawn, LSP initialize/shutdown, touch/stop/status commands
- **Goal:** Add filesystem watching to the LSP daemon so rust-analyzer's VFS stays
  current without manual refresh. File changes between queries are automatically
  reflected in subsequent analysis.
- **Scope:** ~200-300 lines. New file `src/lsp/watcher.rs` + daemon.rs integration.

### Key decisions
- **Threading model:** The daemon loop is synchronous polling (non-blocking UDS accept
  + 100ms sleep on WouldBlock). `notify` crate runs its own thread internally; we
  use `std::sync::mpsc::channel` to bridge FS events into the main loop via `try_recv()`.
  No async/tokio needed — this fits the existing architecture cleanly.
- **Debounce in main loop:** Rather than using `notify-debouncer-*` crates, debounce
  in the daemon's poll loop. Collect events into a buffer; flush to ra when buffer
  is non-empty and oldest event is >300ms old. The 100ms polling interval gives
  sufficient resolution. This avoids an extra dependency.
- **`notify` version:** v6.x (stable). `RecommendedWatcher` with recursive mode.
  The ticket specifies v6+.
- **Platform dep gating:** `notify` is added under `[target.'cfg(unix)'.dependencies]`
  since the `lsp` module is `#[cfg(unix)]`. This prevents compiling `notify` on
  non-Unix targets where it would be dead code.
- **`RaTransport` access:** `run_daemon()` owns `transport` as a local variable. The
  main loop calls `transport.send_notification(...)` directly for FS events — no need
  to pass `transport` to `handle_client()`.
- **Rejected alternative:** async refactor with tokio `select!`. Overkill for the
  current synchronous design; adds significant complexity for no benefit at this
  scale.

## Relevant Files
- `src/lsp/daemon.rs` — main loop (`run_daemon()`), owns `RaTransport`, will call
  watcher setup and FS event polling
- `src/lsp/watcher.rs` — **NEW** file. Watcher setup, event filtering, debounce
  buffer, LSP notification construction
- `src/lsp/mod.rs` — add `mod watcher;` declaration
- `Cargo.toml` — add `notify` dependency
- `ai-docs/mental-model/lsp-daemon.md` — update after implementation

## Conventions (verified from code)
- The `lsp` module uses `eprintln!("[lsp-daemon] ...")` for all logging (daemon stderr
  is silenced in production via `Stdio::null()` in `spawn_daemon()`)
- `RaTransport::send_notification(method, params)` sends a JSON-RPC notification with
  no response expected — exactly what `workspace/didChangeWatchedFiles` needs
- The main loop polls with non-blocking accept + 100ms sleep on WouldBlock; FS event
  checking fits naturally in this gap
- LSP file change types: Created=1, Changed=2, Deleted=3 (protocol spec)
- `run_daemon()` constructs file:// URIs for workspace root using the pattern
  `format!("file://{path_str}")` for absolute paths (see `send_initialize()`)
- Module entry pattern: `src/lsp/mod.rs` re-exports public items and contains the
  `run_lsp_command()` dispatcher

## Implementation Steps

1. **Add `notify` dependency to `Cargo.toml`**
   - Add `notify = "6"` under `[target.'cfg(unix)'.dependencies]`
   - Run `cargo check` to verify resolution
   - Delegation: main
   - Depends on: none

2. **Add `mod watcher;` to `src/lsp/mod.rs`**
   - Add `mod watcher;` (private — accessible from sibling `daemon.rs` via
     `super::watcher::*`)
   - Delegation: main
   - Depends on: step 3 (watcher.rs must exist)

3. **Create `src/lsp/watcher.rs` — watcher setup + event translation**
   - Public function: `start_watcher(workspace_root: &Path) -> Result<(RecommendedWatcher, Receiver<FileEvent>)>`
     - Creates `mpsc::channel::<FileEvent>()`
     - Creates `RecommendedWatcher` with closure callback that filters, translates,
       and sends individual `FileEvent`s through the channel
     - Adds recursive watch on `workspace_root` via `watcher.watch(root, RecursiveMode::Recursive)`
     - Returns watcher handle (must be kept alive) and receiver
   - `FileEvent` struct: `{ uri: String, change_type: u32 }` (pre-translated to LSP
     format). Derives `Debug, Clone`.
   - Event filtering in the callback (closure receives `notify::Result<Event>`):
     - On `Err(e)`: log `eprintln!("[lsp-daemon] watch error: {e}")`, return
     - For each path in `event.paths`:
       - Reject: any component is `target` or starts with `.`
       - Accept: extension is `rs`, or file name is `Cargo.toml` or `Cargo.lock`
       - Skip otherwise
     - Translate `notify::EventKind`:
       - `Create(_)` → 1
       - `Modify(_)` → 2 (match all `ModifyKind` variants including `Any` —
         macOS FSEvents often produces `Any` for editor saves)
       - `Remove(_)` → 3
       - Other (`Access`, `Other`) → skip
     - Path → URI: `format!("file://{}", path.display())` (Unix absolute paths
       start with `/`, so this produces `file:///abs/path` which is correct)
     - Send each `FileEvent` individually via `tx.send(event).ok()` (ignore
       send failures — receiver may have been dropped during shutdown)
   - Delegation: main
   - Depends on: step 1

4. **Add debounce logic — `DebounceBuffer` in `watcher.rs`**
   - Struct: `DebounceBuffer { events: Vec<FileEvent>, first_event_time: Option<Instant> }`
   - `new() -> Self` — empty buffer
   - `push(&mut self, event: FileEvent)` — appends event, sets `first_event_time`
     if `None`
   - `should_flush(&self) -> bool` — true if `first_event_time` is `Some` and
     elapsed >300ms
   - `drain(&mut self) -> Vec<FileEvent>` — dedup by URI (keep last occurrence per
     URI via `HashMap` insertion order; iterate from back to preserve latest
     change_type), resets `first_event_time` to `None`, returns deduped events.
     Ordering note: events arrive via mpsc channel (FIFO), so insertion order is
     chronological. "Last occurrence" = latest change_type for that URI.
   - Delegation: main
   - Depends on: step 3

5. **Build LSP notification — `build_did_change_notification()` in `watcher.rs`**
   - Public function: `build_did_change_notification(events: &[FileEvent]) -> serde_json::Value`
   - Constructs `workspace/didChangeWatchedFiles` params:
     ```json
     { "changes": [ { "uri": "file:///...", "type": 1 }, ... ] }
     ```
   - Delegation: main
   - Depends on: step 3

6. **Integrate watcher into `daemon.rs` main loop**
   - After LSP initialize (step 4 in current daemon code), before binding UDS
     listener: attempt `watcher::start_watcher(workspace_root)`.
   - On success: store `_watcher` handle (kept alive by ownership) and `fs_rx:
     Receiver<FileEvent>`. Create `debounce_buf = DebounceBuffer::new()`.
   - On failure: log `eprintln!("[lsp-daemon] file watcher failed: {e}, continuing
     without")`, set `fs_rx = None`. The daemon continues to function without
     auto-refresh.
   - Type: `fs_rx: Option<Receiver<FileEvent>>`. The integration code uses
     `if let Some(rx) = &fs_rx` to guard channel access.
   - In the main loop, at the bottom of the loop body (after `match listener.accept()`
     block, before the ra liveness check). This runs on every iteration — both
     after client connections and after WouldBlock sleeps. The debounce buffer
     handles timing; running the drain on every iteration is harmless and ensures
     FS events are collected promptly:
     ```rust
     // Drain FS events
     if let Some(rx) = &fs_rx {
         while let Ok(event) = rx.try_recv() {
             debounce_buf.push(event);
         }
         if debounce_buf.should_flush() {
             let events = debounce_buf.drain();
             let params = watcher::build_did_change_notification(&events);
             if let Err(e) = transport.send_notification(
                 "workspace/didChangeWatchedFiles", params
             ) {
                 eprintln!("[lsp-daemon] failed to notify ra of file changes: {e}");
             }
         }
     }
     ```
   - Log watcher start: `eprintln!("[lsp-daemon] file watcher started")`
   - Delegation: main
   - Depends on: steps 2, 3, 4, 5

7. **Update `ai-docs/mental-model/lsp-daemon.md`**
   - Add watcher to Extension Points
   - Add watcher coupling notes (watcher thread ↔ main loop channel)
   - Update the entry points section with watcher.rs
   - Delegation: main
   - Depends on: step 5

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `watcher.rs` (event filtering) | Post-impl unit tests | Event filtering is pure logic; easy to test with synthetic `notify::Event` objects |
| `watcher.rs` (debounce buffer) | TDD | `DebounceBuffer` is a self-contained data structure; test push/flush/dedup behavior |
| `daemon.rs` (integration) | Manual | Requires running daemon + ra + real filesystem; integration test infrastructure for LSP is out of scope |

### TDD module: `DebounceBuffer`
- **Stub scope:** `FileEvent { uri: String, change_type: u32 }`,
  `DebounceBuffer::new()`, `push(FileEvent)`, `should_flush() -> bool`,
  `drain() -> Vec<FileEvent>`
- **Exemplar cases (main agent):**
  - Empty buffer: `should_flush()` returns false, `drain()` returns empty vec
  - Single event: not flushed immediately, flushed after 300ms
  - Multiple events for same URI: dedup keeps latest change_type
  - Events for different URIs: all preserved
  - Create then Delete for same URI: result is Delete (change_type=3)
  - After drain, buffer is empty: `should_flush()` returns false
- **Population cases:** N/A — small test surface, no delegation needed

### Post-impl module: event filtering
- **Key scenarios:**
  - `.rs` file create event → accepted
  - `Cargo.toml` modify event → accepted
  - `target/debug/foo.rs` event → rejected
  - `.git/HEAD` event → rejected
  - Access/metadata event → rejected
  - Non-`.rs` file event → rejected

### Manual module: daemon integration
- **Verification method:**
  1. `cargo brief lsp touch` (start daemon)
  2. Edit a `.rs` file in the workspace
  3. `cargo brief lsp status` (verify daemon still running)
  4. (Query commands in next ticket will validate ra actually received the notification)

## Success Criteria
- `notify` dependency compiles and watcher starts without error on macOS/Linux
- Daemon startup log shows `[lsp-daemon] file watcher started` (visible when running
  daemon manually; watcher startup is separate from file edits)
- `target/` directory writes do NOT trigger notifications to ra (unit test covers this)
- Daemon still shuts down cleanly with file watcher running (watcher dropped on exit)
- Watcher failure does not prevent daemon from starting (graceful degradation)
- DebounceBuffer unit tests pass (dedup, timing, edge cases)
- Event filtering unit tests pass (accept/reject patterns)
- No new compilation warnings
- `cargo test` passes (no existing tests broken)
- `cargo clippy` clean
- **Remaining scope:** query commands (`260326-feat-lsp-query-commands`) are the
  next sub-ticket — they will validate that ra actually receives and processes
  the file change notifications end-to-end
