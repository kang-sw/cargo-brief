---
title: LSP Daemon
summary: cargo-brief's LSP daemon — lifecycle management (touch/stop/status), query commands (references, blast-radius, call-hierarchy), symbol resolution strategy, RA indexing readiness, idle timeout, filesystem watcher, and IPC mechanism.
features:
  - Subcommand Overview
  - `lsp touch`
  - `lsp stop`
  - `lsp status`
  - `lsp references`
  - `lsp blast-radius`
  - `lsp call-hierarchy`
  - Symbol Resolution
  - Daemon Lifecycle
  - Daemon Directory Location
  - Idle Timeout
  - RA Indexing Readiness
  - Filesystem Watcher
  - IPC Mechanism
---

# LSP Daemon

`cargo brief lsp` manages a background `rust-analyzer` daemon per workspace and exposes query commands for code navigation. The daemon is spawned on demand, survives the invoking shell, and shuts down automatically after an idle period.

## Subcommand Overview {#260423-lsp-subcommand-overview}

Six commands are available under `cargo brief lsp`:

| Command | Purpose |
|---|---|
| `touch` | Ensure the daemon is running; wait until RA finishes indexing |
| `stop` | Gracefully shut down the daemon |
| `status` | Print daemon state and uptime |
| `references <SYMBOL>` | Find all references to a symbol |
| `blast-radius <SYMBOL>` | BFS of callers up to N levels deep |
| `call-hierarchy <SYMBOL>` | One-level incoming or outgoing call tree |

LSP commands do not support remote crate mode. Passing `-C` alongside any `lsp` subcommand is rejected immediately with an error.

The hidden `__lsp-daemon` entry point used for daemon re-exec is documented in [CLI Surface](cli-surface.md#260423-lsp-subcommand-lifecycle).

## `lsp touch` {#260423-lsp-touch-command}

Ensures the daemon is running and blocks until rust-analyzer finishes indexing. Progress dots print to stderr approximately every 3 seconds. The command exits 0 once RA reports Ready.

```
cargo brief lsp touch [--no-wait]
```

`--no-wait` — sends a status request with a 5-second timeout instead of blocking. Reports current daemon state and returns immediately regardless of indexing progress.

On startup failure or timeout (120 seconds), the last 20 lines of `lsp.log` are printed to help diagnose the problem.

> [!note] Implementation Gap · 2026-04-23
> A bug exists where the daemon spawns successfully (`touch` exits 0) but dies within seconds of startup. `lsp status` immediately shows "not running" after a successful `touch`. Root cause has not been identified. `cargo brief lsp` is non-functional in affected environments; `cargo brief code --refs` serves as a grep-based fallback for reference lookup.

## `lsp stop` {#260423-lsp-stop-command}

Sends a stop request to the daemon (5-second timeout), removes IPC files, and deletes the daemon directory. Silent when the daemon is not running.

```
cargo brief lsp stop
```

## `lsp status` {#260423-lsp-status-command}

Prints daemon state to stdout. When the daemon is running:

```
LSP daemon: running
  PID:    12345
  RA:     Ready
  Uptime: 4m 23s
  Dir:    /path/to/project/target/cargo-brief-lsp/<hash>/
```

RA state is one of `Initializing`, `Indexing`, or `Ready`. When the daemon is not running:

```
LSP daemon: not running
```

## `lsp references` {#260423-lsp-references-command}

Finds all references to a symbol in the workspace.

```
cargo brief lsp references <SYMBOL> [-q]
```

Normal output groups locations by file path, with right-aligned line numbers and the source line for each reference:

```
src/render.rs
    42  let widget = Widget::new();
   107  widget.render(&ctx);

src/lib.rs
     8  pub use render::Widget;
```

When a source line cannot be read, `<source unavailable>` is substituted.

`-q` / `--quiet` — one line per reference: `@<relative-path>:<line>`.

## `lsp blast-radius` {#260423-lsp-blast-radius-command}

BFS over all callers of a symbol up to `--depth` levels (clamped to [1, 10]).

```
cargo brief lsp blast-radius <SYMBOL> [--depth N] [-q]
```

Normal output groups callers by depth level:

```
Direct callers of Widget::render
  src/app.rs:88  run_frame()

Depth 2
  src/main.rs:12  main()
```

Callers are deduplicated by source location. `--depth` defaults to a small value sufficient to show immediate transitive impact.

`-q` / `--quiet` — one line per caller: `@<path>:<line>  <name>  [depth=N]`.

## `lsp call-hierarchy` {#260423-lsp-call-hierarchy-command}

One-level incoming or outgoing call tree for a symbol.

```
cargo brief lsp call-hierarchy <SYMBOL> [--outgoing] [-q]
```

Normal output uses arrow indicators and column-aligned names:

```
← Callers of Widget::render
  src/app.rs:88     run_frame
  src/bench.rs:14   bench_render
```

`--outgoing` reverses direction (`→` arrow, outgoing calls listed).

`-q` / `--quiet` — one line per entry: `@<path>:<line>  <name>`.

## Symbol Resolution {#260423-lsp-symbol-resolution}

All three query commands resolve the `<SYMBOL>` argument through a two-stage strategy:

**Stage 1 — `workspace/symbol`:** Queries RA for symbols whose name segment exactly matches the given name. When the symbol contains `::`, the part before the last `::` is used as a container name filter.

**Stage 2 — grep fallback:** When Stage 1 returns no matches, cargo-brief greps workspace `.rs` files for the qualified name (`Type::method`) and then the bare name, collecting up to 15 candidate locations. `use` declarations are prioritized. Each candidate is resolved via `textDocument/definition` and deduplicated by `(uri, line)`.

The fallback enables resolution of external dependency types, common method names, and qualified names that `workspace/symbol` filters out.

**Ambiguous results:** When multiple symbols match, a numbered disambiguation list is printed and the command exits 0. The user repeats the command with a more qualified name.

**Not found:** When no symbol matches either stage, cargo-brief exits 1 with `Symbol not found: <SYMBOL>`.

## Daemon Lifecycle {#260423-lsp-daemon-lifecycle}

One daemon process runs per workspace root. Any `lsp` command that requires the daemon (all except `stop` when already stopped) triggers auto-spawn if no live daemon is detected.

**Spawn mechanism:** cargo-brief re-execs itself with `__lsp-daemon` as the first argument (see [CLI Surface](cli-surface.md#260423-lsp-subcommand-lifecycle)), passing `--workspace-root` and `--daemon-dir` flags. The daemon process is detached from the invoking shell:

- **Unix** — `setsid()` creates a new session, fully isolating the daemon from `SIGHUP` on terminal close.
- **Windows** — `CREATE_NEW_PROCESS_GROUP` creation flag detaches from the console.

Daemon stderr is redirected to `lsp.log` in the daemon directory. Stdin and stdout are redirected to null.

**Stale daemon recovery:** On spawn, cargo-brief reads the PID file and checks whether the process is alive. A stale PID (dead process) triggers cleanup of IPC files before re-spawning.

**Shutdown:** The daemon sends LSP `shutdown` + `exit` to rust-analyzer before exiting. On `lsp stop`, the client also removes all IPC and PID files.

## Daemon Directory Location {#260423-lsp-daemon-dir-location}

The daemon directory is:

```
<target_dir>/cargo-brief-lsp/<hash>/
```

`<target_dir>` is the workspace's Cargo target directory. `<hash>` is an FNV-1a hash of the canonical workspace root path. Files inside the directory:

| File | Purpose |
|---|---|
| `lsp.pid` | Daemon PID |
| `lsp.lock` | Client serialization lock |
| `lsp.req` (Unix) | Request FIFO |
| `lsp.resp` (Unix) | Response FIFO |
| `lsp.req` / `lsp.resp` (Windows) | Atomic-rename message files |
| `lsp.ready` (Windows) | Readiness marker |
| `lsp.log` | Daemon stderr log |

## Idle Timeout {#260423-lsp-idle-timeout}

The daemon exits automatically when no client request has been received for the idle timeout duration.

Default: **600 seconds** (10 minutes). Override with `CARGO_BRIEF_LSP_TIMEOUT=<seconds>`.

## RA Indexing Readiness {#260423-lsp-indexing-readiness}

The daemon tracks rust-analyzer's indexing state via `$/progress` notifications, which require the daemon to declare `window.workDoneProgress: true` in the LSP `initialize` request.

States: `Initializing` → `Indexing` → `Ready`.

**Settle period:** When RA first signals Ready, the daemon continues draining RA output for 5 seconds. A new `Indexing` begin during this window resets the timer. This handles RA's multi-phase startup, which may report Ready before all indexing cycles complete.

**No-progress fallback:** If no `$/progress` notifications arrive within 10 seconds of RA startup, the daemon promotes RA status to `Ready` without waiting. This handles RA versions or configurations that do not emit progress notifications.

**Query-time wait:** `references`, `blast-radius`, and `call-hierarchy` wait for RA to reach Ready before dispatching LSP requests. The maximum wait is configurable:

- Default: **60 seconds**
- Override: `CARGO_BRIEF_LSP_READY_TIMEOUT=<seconds>`

Timeout message: `rust-analyzer is still indexing (waited Ns)`.

## Filesystem Watcher {#260423-lsp-filesystem-watcher}

The daemon watches the workspace root for file changes and forwards them to RA as `workspace/didChangeWatchedFiles` notifications, keeping RA's view of the workspace consistent without requiring a restart.

Accepted file patterns: `*.rs`, `Cargo.toml`, `Cargo.lock`.

Excluded paths: any path component named `target` or starting with `.` (hidden directories).

Changes are debounced with a 300 ms buffer. Within each window, duplicate events for the same URI are collapsed, keeping the latest `changeType`.

Watcher initialization failure is non-fatal — the daemon continues running without change notifications if `notify::RecommendedWatcher` cannot start.

## IPC Mechanism {#260423-lsp-ipc-mechanism}

Client–daemon communication uses a platform-specific IPC protocol. Message framing on both platforms: a 4-byte little-endian length prefix followed by a JSON payload, maximum 1 MiB per message. Concurrent clients are serialized via a lock file.

**Unix — FIFO pair:**

`lsp.req` and `lsp.resp` named pipes are created with `0o600` permissions. The daemon opens both `O_RDWR` to avoid blocking and prevent `POLLHUP` when no client is connected. Clients acquire `flock(LOCK_EX)` on `lsp.lock` before writing a request, then read the response, then release. The daemon drains any stale data from the response FIFO on startup.

Readiness is signaled by the existence of the `lsp.req` FIFO.

**Windows — atomic-rename file protocol:**

The client writes the request to `lsp.req.tmp` then renames it to `lsp.req`. The daemon reads and deletes `lsp.req`, writes the response to `lsp.resp.tmp`, then renames it to `lsp.resp`. The client polls for `lsp.resp` at 10 ms intervals, reads and deletes it. `LockFileEx(LOCKFILE_EXCLUSIVE_LOCK)` on `lsp.lock` serializes concurrent clients.

Readiness is signaled by the existence of a separate `lsp.ready` file.

> [!note] Implementation Gap · 2026-04-23
> The Windows IPC and process management modules are fully implemented in source (`ipc/windows.rs`, `process/windows.rs`) but have not been tested on an actual Windows system. Runtime behavior on Windows is unverified. Unix is the only platform with confirmed end-to-end functionality.
