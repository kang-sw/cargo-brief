# LSP Daemon: Track Indexing Status via `$/progress`

## Context

- Ticket: `260327-feat-lsp-indexing-status` (Phase 1 + Phase 2)
- The daemon sets `RaStatus::Ready` right after LSP `initialize` completes,
  but ra continues indexing in the background. During this window,
  `workspace/symbol` returns empty results → false "Symbol not found" errors.
- Observed on an 8-member workspace (bevy/tokio deps): ~10s+ indexing after
  `initialize`. Confirmed by testing: `sleep 10` before first query resolves it.
- Root cause: ra stdout is never polled between requests. `$/progress`
  notifications accumulate in the pipe buffer, unread.

### Key architectural constraints (from research)

- **Single-threaded daemon.** Main loop polls only `req_fd` (FIFO). Ra stdout
  is wrapped in `BufReader<ChildStdout>` inside `RaTransport` — never polled.
- **`send_request_and_wait`** skips all notifications (no `"id"` field). This
  is where `$/progress` messages get silently dropped today.
- **`ChildStdout` implements `AsRawFd`** — we can extract the raw fd for
  `poll()` without restructuring transport. But we must be careful: the
  `BufReader` may have buffered data that `poll()` won't see.
- **`handle_request()` is called synchronously** in the main loop. During a
  query, the loop is blocked — we can drain ra stdout there.

### Design decisions

1. **Poll-then-read pattern for ra stdout.** Keep stdout **always blocking**.
   Before each drain attempt, `poll()` the raw fd (0ms timeout). If `POLLIN`,
   call `read_message()` (blocking, but data is known available). Repeat until
   `poll()` returns 0. Also check `BufReader::buffer().is_empty()` — if the
   internal buffer is non-empty, skip the poll and read directly. Note:
   `BufReader::buffer()` only returns data already read into the buffer by
   prior operations; it does NOT trigger a fill. So this check catches leftover
   data from prior reads but is not a reliable substitute for poll. The
   poll-then-read is the primary mechanism; the buffer check is a minor
   optimization that avoids a syscall when data is already buffered.
2. **Track progress tokens with a `HashSet<String>`.** `$/progress` begin adds
   token, end removes it. Tokens may be JSON integers or strings — normalize
   to `String` via `Value::to_string()` (integers become `"1"`, strings stay
   `"\"token\""`). `RaStatus::Indexing` when set is non-empty.
   `RaStatus::Ready` when set empties after having been non-empty.
3. **Fallback for trivial workspaces.** If ra sends no `$/progress` at all,
   the daemon stays `Initializing` forever. Add a fallback: if no progress
   token has ever been seen and uptime exceeds 10s, treat as `Ready`. This
   handles tiny workspaces where ra skips progress reporting.
4. **Query-time wait (Phase 2).** When `ra_status != Ready` at query time,
   enter a blocking drain loop on ra stdout until status becomes `Ready` or
   timeout (60s default, env-configurable via `CARGO_BRIEF_LSP_READY_TIMEOUT`).
5. **Handle `window/workDoneProgress/create` everywhere.** Ra sends these as
   server-initiated requests (they have an `"id"` field). Must be answered
   both in the main-loop drain and inside `send_request_and_wait`. Add
   detection in `send_request_and_wait` so it replies and continues instead
   of silently dropping the request.

### Rejected alternatives

- **Two-fd poll (req_fd + ra_stdout_fd):** BufReader buffering makes raw fd poll
  unreliable. Would need to drop BufReader or use raw reads everywhere, breaking
  the transport abstraction.
- **Background thread for ra stdout:** Adds complexity (mutex/channel for
  `ra_status`, thread lifecycle). The single-threaded model works; we just need
  to read more often.
- **Client-side retry:** Client can't distinguish "not indexed yet" from "symbol
  doesn't exist" without daemon-side state.

## Relevant Files

- `src/lsp/daemon.rs` — main loop, `RaStatus` transitions, `handle_request()`
- `src/lsp/transport.rs` — `RaTransport`, `send_request_and_wait()`, message
  reading. Will gain `stdout_raw_fd()`, `has_buffered_data()`,
  `send_raw_response()`, and `window/workDoneProgress/create` handling
  inside `send_request_and_wait()`.
- `src/lsp/protocol.rs` — `RaStatus` enum, `DaemonResponse::Status`
- `src/lsp/client.rs` — `send_command()` timeout values
- `src/lsp/mod.rs` — `cmd_query()` timeout, status display formatting
- `src/lsp/query.rs` — `handle_references/blast_radius/call_hierarchy` —
  no changes needed (wait happens before dispatch)

## Conventions (verified from code)

- `RaStatus` is `Copy + PartialEq + Serialize + Deserialize`. Used by value.
- Main loop uses `libc::poll` via `poll_retry()` wrapper (EINTR-safe).
- `set_nonblocking()` in `client.rs` uses `libc::fcntl` `F_GETFL`/`F_SETFL`.
  Can be reused or moved to a shared location.
- Error messages use `eprintln!("[lsp-daemon] ...")` prefix.
- Transport methods return `Result<serde_json::Value>`.
- Timeout env var pattern: `CARGO_BRIEF_LSP_TIMEOUT` parsed with
  `.ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT)`.

## Implementation Steps

### Phase 1: Ra stdout polling + indexing state tracking

#### Step 1: Add `RaStatus::Indexing` variant

**File:** `src/lsp/protocol.rs`

Add `Indexing` between `Initializing` and `Ready` in the `RaStatus` enum.
Update `Display` impl: `Indexing => "indexing"`.

- Delegation: main

#### Step 2: Add helper methods to `RaTransport`

**File:** `src/lsp/transport.rs`

Add three methods plus modify `send_request_and_wait`:

```rust
use std::os::unix::io::{AsRawFd, RawFd};

/// Return the raw fd of ra's stdout for use with poll().
/// Note: transport.rs is inside the cfg(unix) lsp module, so unix imports are safe.
pub fn stdout_raw_fd(&self) -> RawFd {
    self.stdout.get_ref().as_raw_fd()
}

/// Check if BufReader has leftover data from a prior read.
/// This is NOT a reliable "is data available" check — it only catches
/// buffered leftovers. Use poll() as the primary mechanism.
pub fn has_buffered_data(&self) -> bool {
    !self.stdout.buffer().is_empty()
}

/// Send an LSP response to a server-initiated request (e.g.
/// window/workDoneProgress/create). Named `send_raw_response` to
/// distinguish from client-initiated request/response — here WE are
/// responding to a request FROM the server.
pub fn send_raw_response(&mut self, id: serde_json::Value, result: serde_json::Value) -> Result<()>
```

Modify `send_request_and_wait` to handle `window/workDoneProgress/create`:
currently, server-initiated requests (which have an `"id"` field AND a
`"method"` field) are silently skipped. Add detection: if the message has
both `"id"` and `"method"`, it's a server request — reply with
`send_raw_response(id, null)` and continue the loop. This ensures ra gets
responses to its progress token creation requests during active queries.

```rust
// In the existing notification-skip loop inside send_request_and_wait:
if msg.get("id").is_none() {
    continue; // notification
}
// NEW: server-initiated request (has both "id" and "method")
if msg.get("method").is_some() {
    // Reply to server request (e.g. window/workDoneProgress/create)
    if let Some(id) = msg.get("id").cloned() {
        let _ = self.send_raw_response(id, serde_json::json!(null));
    }
    continue;
}
// existing: check if msg["id"] matches our request id
```

- Delegation: main

Note: `read_message()` is already `pub` — daemon.rs can call it directly.

#### Step 3: Add progress tracking to daemon main loop

**File:** `src/lsp/daemon.rs`

Changes:

1. **Remove `Ready` assignment after `send_initialize()`.** Currently L233
   sets `ra_status = RaStatus::Ready` on success — delete this line. Leave
   `ra_status` at `Initializing` until progress tracking promotes it.

2. Add state:

```rust
let mut active_progress: HashSet<String> = HashSet::new();
let mut had_progress = false;
```

3. Add helper function (pure — no IO, testable):

```rust
fn process_ra_notification(
    msg: &serde_json::Value,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
) -> Option<RaStatus>
```

Handles `$/progress` notifications only (caller handles server-initiated
requests separately):
- `method == "$/progress"` with `params.token` and `params.value.kind`:
  - `"begin"` → insert `token.to_string()` (normalizes int/string), set
    `*had_progress = true`
  - `"end"` → remove `token.to_string()`
  - `"report"` → no-op
- Return value:
  - `had_progress && active_progress.is_empty()` → `Some(Ready)`
  - `!active_progress.is_empty()` → `Some(Indexing)`
  - Otherwise → `None`
- Non-`$/progress` notifications → `None`

4. Extract a reusable drain helper (used in both main loop and
   `wait_for_ready`):

```rust
/// Drain all available ra stdout messages. Updates ra_status and replies
/// to server-initiated requests. Returns true if any messages were read.
fn drain_ra_messages(
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
    start_time: Instant,  // for fallback timeout
) -> Result<bool>
```

Inside this function:
```rust
let mut any_read = false;
loop {
    // Check BufReader internal buffer first (minor optimization)
    if !transport.has_buffered_data() {
        let mut pfd = libc::pollfd { fd: transport.stdout_raw_fd(), events: POLLIN, revents: 0 };
        let n = poll_retry(&mut pfd, 0)?;
        if n == 0 { break; }
    }
    match transport.read_message() {
        Ok(msg) => {
            any_read = true;
            // Progress status update
            if let Some(new_status) = process_ra_notification(&msg, active_progress, had_progress) {
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
        Err(e) => {
            eprintln!("[lsp-daemon] ra stdout read error: {e}");
            break;
        }
    }
}
// Fallback: no progress tokens ever seen, uptime > 10s → assume Ready
if !*had_progress && *ra_status == RaStatus::Initializing && start_time.elapsed().as_secs() > 10 {
    eprintln!("[lsp-daemon] ra status: ready (no progress reported, fallback)");
    *ra_status = RaStatus::Ready;
}
Ok(any_read)
```

5. Call `drain_ra_messages()` in the main loop, after the idle-timeout
   check and before FS event drain. Placement: same location as the
   existing `ra_child.try_wait()` check area.

Note: `cmd_status` display needs no code change — `RaStatus::Display`
handles `Indexing` from step 1, and `mod.rs` uses `{ra_status}` formatting.

- Delegation: main
- Depends on: step 1, step 2

### Phase 2: Query-time wait-for-ready

#### Step 4: Add wait-for-ready before query dispatch

**File:** `src/lsp/daemon.rs`

```rust
fn wait_for_ready(
    transport: &mut RaTransport,
    ra_status: &mut RaStatus,
    active_progress: &mut HashSet<String>,
    had_progress: &mut bool,
    start_time: Instant,
    timeout: Duration,
) -> Result<()>
```

Implementation: blocking loop that polls ra stdout with 500ms timeout,
reads messages via the same poll-then-read pattern as `drain_ra_messages`
(check `has_buffered_data()` first, then `poll()`), processes progress
notifications, and checks wall-clock elapsed against `timeout`.

- If `*ra_status == Ready` → return `Ok(())`.
- If timeout → `bail!("rust-analyzer is still indexing (waited {}s). Try again shortly.", ...)`.
- Default timeout: 60s. Env var: `CARGO_BRIEF_LSP_READY_TIMEOUT`.

Integrate into the main loop between reading the FIFO request and calling
`handle_request()`:

```rust
let is_query = matches!(request, DaemonRequest::References{..} | DaemonRequest::BlastRadius{..} | DaemonRequest::CallHierarchy{..});
if is_query && ra_status != RaStatus::Ready {
    if let Err(e) = wait_for_ready(&mut transport, &mut ra_status, &mut active_progress, &mut had_progress, start_time, ready_timeout) {
        let response = DaemonResponse::Error { message: format!("{e}") };
        write_message(&mut resp_fd, &response)?;
        set_nonblocking(&req_fd, true)?;
        continue;
    }
}
```

- Delegation: main
- Depends on: step 3

#### Step 5: Increase client-side timeout for first query

**File:** `src/lsp/mod.rs`

`cmd_query()` currently uses `Duration::from_secs(30)` for `send_command`.
Increase to 120s to accommodate: 60s daemon-side indexing wait + 30s query
execution + margin. Use a constant:
`const QUERY_TIMEOUT: Duration = Duration::from_secs(120);`.

- Delegation: main
- Depends on: step 4

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `protocol.rs` (RaStatus) | Post-impl | Trivial enum addition, existing roundtrip tests cover serialization |
| `transport.rs` (`send_raw_response`, `send_request_and_wait` changes) | Post-impl | Thin wrappers; `send_request_and_wait` changes are structural |
| `daemon.rs` (`process_ra_notification`) | TDD | Pure function, easy to test with crafted JSON |
| `daemon.rs` (`drain_ra_messages`, `wait_for_ready`) | Manual | Requires live ra process; test via stub workspace |
| Integration (full flow) | Manual | Use the existing `target/lsp-test-ws` stub workspace |

### TDD: `process_ra_notification`

**Stub scope:** Function takes `(&Value, &mut HashSet<String>, &mut bool)`,
returns `Option<RaStatus>`.

**Exemplar cases (main agent):**
- `$/progress` begin → inserts token, returns `Some(Indexing)`
- `$/progress` end (last token) → removes token, returns `Some(Ready)`
- `$/progress` end (not last) → removes token, returns `None` (still indexing)
- `$/progress` report → returns `None`
- Non-progress notification → returns `None`
- `$/progress` begin when `had_progress` already true → still `Some(Indexing)`

**Population cases (delegable to haiku):**
- Multiple begin/end sequences (two full cycles)
- Unknown progress token in end (no-op, returns `None`)
- Token as integer (JSON number) → normalized via `to_string()`
- Token as string → normalized via `to_string()`
- Mixed: begin with int token, end with same int token → properly removed

### Post-impl: RaStatus serialization

- Add `Indexing` to existing `roundtrip_response` test in `protocol.rs`

### Manual verification

Note: `target/lsp-test-ws` is a developer-local stub workspace created
during debugging. If it doesn't exist, create it per the steps documented
in the session where the issue was discovered.

1. `cd target/lsp-test-ws && cargo brief lsp stop`
2. `cargo brief lsp status` → "not running"
3. `cargo brief lsp references apply_damage` → daemon starts, waits for
   indexing, then returns results (no manual sleep needed)
4. `cargo brief lsp status` → "ra: ready"
5. `lsp.log` shows `ra status: indexing` → `ra status: ready` transitions
6. Repeat with `blast-radius` and `call-hierarchy`

## Success Criteria

- `cargo brief lsp status` reports `indexing` during ra startup, `ready` after
- Queries during indexing block until ready (up to 60s), then succeed
- Queries after indexing completes work immediately (no regression)
- No false "Symbol not found" on first query after daemon start
- `lsp.log` shows `[lsp-daemon] ra status: indexing` and `ra status: ready`
  transitions
