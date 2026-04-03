---
title: LSP Daemon Integration
summary: Persistent rust-analyzer daemon for semantic code queries -- references, blast-radius, and call-hierarchy -- via the `cargo brief lsp` subcommand.

features:
  - Daemon Lifecycle
    - Auto-Start
    - Indexing
    - Idle Timeout
    - File Watching
    - Stale Daemon Recovery
  - `lsp touch`
  - `lsp stop`
  - `lsp status`
  - `lsp references <SYMBOL>`
    - Normal Output
    - Quiet Output (`-q`)
  - `lsp blast-radius <SYMBOL>`
    - Normal Output
    - Quiet Output (`-q`)
  - `lsp call-hierarchy <SYMBOL>`
    - Normal Output (Incoming)
    - Normal Output (Outgoing)
    - Quiet Output (`-q`)
  - Symbol Resolution
    - Resolution Strategy
    - Qualified Names
    - Ambiguous Symbols
    - Symbol Not Found
  - Quiet Mode (`-q`)
  - Environment Variables
  - Timeouts
  - Constraints
  - 🚧 Windows Platform Support
---

# LSP Daemon Integration

`cargo brief lsp` manages a persistent rust-analyzer daemon that provides
semantic code analysis for the local workspace. The daemon auto-starts on first
query and stays alive to serve subsequent queries with sub-second latency.

```
cargo brief lsp <command> [OPTIONS]
```

All `lsp` subcommands share the `--toolchain`, `--verbose`, and `--manifest-path`
flags. The `-C` (remote crate) flag is rejected -- LSP commands operate on the
local workspace only.

## Daemon Lifecycle

One daemon exists per workspace root. The workspace root is determined from
`cargo metadata` (respecting `--manifest-path` if given). Two projects with
different `Cargo.toml` paths that resolve to the same workspace root share a
single daemon instance. Symlinked paths are canonicalized before hashing, so
equivalent paths converge.

The daemon directory lives inside the project's `target/` directory:
`<target_dir>/cargo-brief-lsp/<hash>/`. This avoids macOS sandbox restrictions
that affect `$TMPDIR` or `$XDG_RUNTIME_DIR`.

### Auto-Start

The daemon starts automatically when any query command (`references`,
`blast-radius`, `call-hierarchy`) or `touch` is invoked. If no daemon is
running, one is spawned as a detached background process (new session via
`setsid()` on Unix). The daemon survives terminal closure.

### Indexing

After spawning, the daemon initializes rust-analyzer against the workspace.
Indexing may take seconds to minutes depending on project size. The daemon
tracks rust-analyzer's state:

- **Initializing** -- LSP handshake in progress.
- **Indexing** -- rust-analyzer is building its index (progress notifications active).
- **Ready** -- indexing complete, queries can execute.
- **Stopped** -- rust-analyzer has exited.

Query commands automatically wait (up to 60 seconds by default) for the daemon
to reach the Ready state before executing. The `touch` command in blocking mode
waits indefinitely.

### Idle Timeout

The daemon shuts down after 10 minutes of inactivity (no client requests).
Override with the `CARGO_BRIEF_LSP_TIMEOUT` environment variable (value in
seconds, read by the daemon at startup).

### File Watching

The daemon watches workspace source files for changes and sends
`textDocument/didChange` notifications to rust-analyzer, keeping the index
up to date without restarts. File events are debounced (300ms batches).

### Stale Daemon Recovery

If a daemon dies unexpectedly (crash, `kill -9`), the next client detects the
stale PID file, cleans up leftover files, and spawns a fresh daemon
automatically.

## `lsp touch`

Ensure the daemon is running and optionally wait for indexing to complete.

```
cargo brief lsp touch [--no-wait]
```

**Default (blocking):** Blocks until rust-analyzer reaches the Ready state.
Prints progress dots to stderr every 3 seconds while waiting. Returns
successfully once indexing is complete.

**`--no-wait` (fire-and-forget):** Returns immediately after confirming the
daemon is alive. Reports current status (PID, rust-analyzer state, uptime)
to stderr.

Use `touch` to pre-warm the daemon before running queries, especially in CI
scripts or editor startup hooks where you want indexing to complete before the
first real query.

## `lsp stop`

Gracefully shut down the daemon.

```
cargo brief lsp stop
```

Sends a stop request to the daemon and cleans up all IPC files. If no daemon
is running, exits silently (no error). The daemon directory is removed after
cleanup.

## `lsp status`

Show daemon status.

```
cargo brief lsp status
```

Output when running:

```
LSP daemon: running
  PID:     12345
  RA:      ready
  Uptime:  5m 32s
  Dir:     /path/to/target/cargo-brief-lsp/abc123def456
```

Output when not running:

```
LSP daemon: not running
```

The RA field reflects rust-analyzer's current state: `initializing`, `indexing`,
`ready`, or `stopped`.

## `lsp references <SYMBOL>`

Find all references to a symbol across the workspace.

```
cargo brief lsp references <SYMBOL> [-q]
```

### Normal Output

```
// 5 references to resolve_symbol
// src/lsp/query.rs
  24:  use super::transport::RaTransport;
 418:      match resolve_symbol(transport, symbol, workspace_root)? {
 551:      let m = match resolve_symbol(transport, symbol, workspace_root)? {
// src/lsp/mod.rs
  12:  use query::resolve_symbol;
  45:      let result = resolve_symbol(&mut transport, &symbol)?;
```

References are grouped by file, sorted by line number within each group. Source
lines are read from disk by the daemon and included inline. If a source file is
unreadable, individual lines show `<source unavailable>`.

Line numbers are 1-indexed and right-aligned within each file group.

### Quiet Output (`-q`)

```
@src/lsp/query.rs:24
@src/lsp/query.rs:418
@src/lsp/query.rs:551
@src/lsp/mod.rs:12
@src/lsp/mod.rs:45
```

Location-only: `@<relative-path>:<line>`, one per line. Paths are relative to the
workspace root.

## `lsp blast-radius <SYMBOL>`

Show direct and transitive callers of a symbol via BFS over the incoming call
hierarchy. Answers the question: "what breaks if I change this function?"

```
cargo brief lsp blast-radius <SYMBOL> [--depth N] [-q]
```

`--depth N` controls how many levels of transitive callers to traverse.
Default: 1 (direct callers only). Maximum: 10. Values outside `[1, 10]` are
silently clamped.

Best suited for functions and methods. For types (struct, enum, trait), use
`references` instead -- call hierarchy is not available for non-callable items.

### Normal Output

```
// Blast radius for handle_request (3 direct, 2 transitive)
//
// Direct:
//   run_daemon()        src/lsp/daemon.rs:142
//   process_message()   src/lsp/daemon.rs:205
//   test_handler()      tests/lsp_tests.rs:55
//
// Depth 2:
//   main()              src/main.rs:12       -> run_daemon()
//   integration_test()  tests/lsp_tests.rs:8 -> test_handler()
```

Output is grouped by depth level. Transitive callers show a `-> via()` annotation
indicating which direct/intermediate caller led to them. Callers are deduplicated
across levels.

### Quiet Output (`-q`)

```
@src/lsp/daemon.rs:142  run_daemon  [depth=1]
@src/lsp/daemon.rs:205  process_message  [depth=1]
@tests/lsp_tests.rs:55  test_handler  [depth=1]
@src/main.rs:12  main  [depth=2]
@tests/lsp_tests.rs:8  integration_test  [depth=2]
```

One entry per line: `@<path>:<line>  <name>  [depth=N]`.

## `lsp call-hierarchy <SYMBOL>`

Show incoming or outgoing call hierarchy for a symbol.

```
cargo brief lsp call-hierarchy <SYMBOL> [--outgoing] [-q]
```

**Default (incoming):** Who calls this symbol?

**`--outgoing`:** What does this symbol call?

Best suited for functions and methods. If `prepareCallHierarchy` returns no
results for the resolved symbol, prints `No call hierarchy found for <symbol>`.

### Normal Output (Incoming)

```
// Incoming calls to resolve_symbol
//
// <- run_pipeline()    src/pipeline.rs:42
// <- render_item()     src/render.rs:115
```

### Normal Output (Outgoing)

```
// Outgoing calls from resolve_symbol
//
// -> workspace_symbol()  src/lsp/transport.rs:88
// -> grep_workspace()    src/lsp/query.rs:55
```

Caller/callee names are shown with `()` suffix. Entries are column-aligned.

### Quiet Output (`-q`)

```
@src/pipeline.rs:42  run_pipeline
@src/render.rs:115  render_item
```

One entry per line: `@<path>:<line>  <name>`.

## Symbol Resolution

All three query commands (`references`, `blast-radius`, `call-hierarchy`) resolve
the `<SYMBOL>` argument to a source location before executing the LSP query.

### Resolution Strategy

Resolution uses a two-stage approach:

1. **Stage 1: workspace/symbol** (fast) -- Sends a `workspace/symbol` LSP request
   for the last `::` segment of the query. If the query is qualified (e.g.,
   `Foo::bar`), the container name is matched as a substring filter. This stage
   finds workspace-defined items quickly.

2. **Stage 2: grep + definition fallback** (slower) -- If stage 1 returns zero
   matches, greps workspace `.rs` files (skipping `target/` and hidden
   directories) for literal occurrences of the symbol name. `use` import lines
   are prioritized. Up to 15 hits are forwarded to `textDocument/definition`
   to resolve the actual definition location. This stage finds external
   dependency symbols (e.g., `hecs::World`, `serde::Serialize`).

### Qualified Names

Qualified names are supported and recommended for disambiguation:

- `hecs::World` -- external type via usage-site fallback
- `App::new` -- method on a specific type
- `MyStruct::method` -- disambiguate common method names

### Ambiguous Symbols

When multiple symbols match, the command prints a disambiguation list instead
of results:

```
Multiple symbols match "bar":
  1. fn Foo::bar  src/foo.rs:42
  2. fn Baz::bar  src/baz.rs:17
```

Use a more qualified name to narrow the match.

### Symbol Not Found

If no symbol matches at all, the command exits with an error:
`Symbol not found: <symbol>`.

Common reasons:
- The symbol name is misspelled.
- The symbol is in an external crate and no `use` import exists in the workspace
  (stage 2 relies on finding usage sites).
- rust-analyzer has not finished indexing (check `lsp status`).

## Quiet Mode (`-q`)

All three query commands support `-q` / `--quiet`. In quiet mode, output contains
only machine-friendly location lines:

- `references`: `@<path>:<line>`
- `blast-radius`: `@<path>:<line>  <name>  [depth=N]`
- `call-hierarchy`: `@<path>:<line>  <name>`

Paths are always relative to the workspace root. Line numbers are 1-indexed.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CARGO_BRIEF_LSP_TIMEOUT` | `600` (10 min) | Daemon idle timeout in seconds. The daemon exits after this duration without client requests. |
| `CARGO_BRIEF_LSP_READY_TIMEOUT` | `60` | Seconds to wait for rust-analyzer to reach Ready state before returning a timeout error on query commands. |

## Timeouts

| Operation | Timeout | Description |
|-----------|---------|-------------|
| `touch` (blocking) | unlimited | Blocks until indexing completes or the daemon dies |
| `touch --no-wait` | 5 seconds | Quick status check after ensuring daemon is alive |
| `stop` | 5 seconds | Time to send stop command and receive acknowledgment |
| `status` | 5 seconds | Time to query daemon status |
| Query commands | 120 seconds | Client-side budget: covers indexing wait (60s) + query execution + margin |
| Daemon startup polling | 120 seconds | `ensure_daemon` polls readiness indicator with exponential backoff (50ms to 500ms) |

## Constraints

- **Local workspace only.** The `-C` (remote crate) flag is rejected. LSP
  queries require a real workspace with source files on disk.

- **One daemon per workspace.** Concurrent queries from multiple terminals are
  serialized via file locking. The second client blocks until the first
  completes -- it does not fail with a timeout.

- **Nightly not required.** The LSP daemon uses rust-analyzer directly, not
  rustdoc JSON. No nightly toolchain is needed for `lsp` commands.

- **call-hierarchy and blast-radius work on callables.** These commands use
  the LSP call hierarchy protocol, which is designed for functions and methods.
  For tracking usage of types, traits, or constants, use `references` instead.

- **Blast-radius depth is clamped to [1, 10].** A `--depth 0` is silently
  treated as 1. A `--depth 100` is silently treated as 10.

## 🚧 Windows Platform Support

The LSP daemon currently works on Unix platforms (Linux, macOS). Windows support
is planned but not yet implemented. The IPC layer uses platform-abstracted
modules (`ipc/unix.rs`, `ipc/windows.rs`) to facilitate future cross-platform
support, but the Windows implementation is not yet functional.

Tracked in ticket `260326-feat-lsp-windows-support`.
