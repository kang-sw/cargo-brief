# LSP Query Commands Phase 1 — Symbol Resolution + References

## Context
- **Ticket:** `260326-feat-lsp-query-commands` Phase 1
- **Prerequisite:** `260326-feat-lsp-daemon-bootstrap` (done), `260326-feat-lsp-file-watcher` (done)
- **Goal:** Add the first user-facing LSP query command: `cargo brief lsp references <symbol>`.
  This validates the full query pipeline (client → daemon → ra → format → output) and
  establishes symbol resolution as the shared foundation for Phase 2 (blast-radius,
  call-hierarchy).
- **Scope:** ~350-450 lines across 5 files (1 new, 4 modified).

### Key decisions
- **Daemon-side formatting:** The daemon reads source files from disk to build context-line
  output. The client receives a pre-formatted string. This keeps the client thin and avoids
  passing file contents over UDS.
- **Fresh UDS connection per query:** `ensure_daemon()` returns a stream that has already
  been used for a Ping exchange. Rather than reusing it (potential for closed-connection
  issues), query commands drop the ensure_daemon stream and open a fresh connection with
  a 30-second read timeout.
- **`workspace/symbol` for resolution:** ra supports fuzzy matching on qualified names.
  We query with the user's full string, then filter results for exact `name` match on the
  last `::` segment. If still ambiguous, filter by `containerName`. If multiple remain,
  return a disambiguation list.
- **No `didOpen` needed:** ra indexes the entire workspace via `rootUri` in initialize.
  `textDocument/references` works on files already in ra's VFS without explicit `didOpen`.
  If this assumption fails, we'll add `didOpen`/`didClose` around queries.
- **Output to stdout:** Consistent with all other cargo-brief subcommands. Status/errors
  go to stderr.
- **Single-threaded query processing:** The daemon main loop blocks during query execution.
  Only one query at a time. Acceptable for a dev tool.
- **Rejected:** `didOpen` for every query file (unnecessary with ra's VFS), async query
  processing (overkill for single-client dev tool), client-side formatting (requires passing
  file contents).

## Relevant Files
- `src/lsp/daemon.rs` — `run_daemon()` main loop, `handle_client()` function. Must pass
  `&mut RaTransport` and `workspace_root: &Path` to `handle_client` for query forwarding.
- `src/lsp/transport.rs` — `RaTransport::send_request_and_wait(method, params)` for LSP
  request-response, `send_notification(method, params)` for one-way messages.
- `src/lsp/protocol.rs` — `DaemonRequest`/`DaemonResponse` enums for UDS messages.
- `src/lsp/client.rs` — `ensure_daemon()`, `daemon_dir()`, `send_command()`.
- `src/lsp/mod.rs` — `run_lsp_command()` dispatcher, command handler functions.
- `src/cli.rs` — `LspCommand` enum (line 709), `LspArgs` struct (line 696).
- `src/lsp/query.rs` — **NEW** file. Symbol resolution, references, output formatting.

## Conventions (verified from code)
- `LspCommand` variants: unit variants for management (`Touch`, `Stop`, `Status`).
  Query variants have fields: `symbol: String`, option flags.
- Client commands follow pattern: `ensure_daemon(workspace_root, verbose)?`, then
  open fresh connection, send request, print response.
- Daemon stderr logging: `eprintln!("[lsp-daemon] ...")` format.
- LSP responses are `serde_json::Value`. Fields accessed via `msg["field"]` pattern.
  `send_request_and_wait()` returns the full response (includes `"result"` key).
- `DaemonResponse` uses `Error { message: String }` for error reporting.
- Quiet output format: `@file:line` (e.g., `@src/main.rs:42`) — from `code.rs` quiet mode.
- Protocol: `write_message`/`read_message` handle length-prefixed JSON on UDS.
  `send_command()` is a convenience wrapper.
- Output pattern: query results go to stdout (`println!`), status to stderr (`eprintln!`).

## Implementation Steps

1. **Add `References` variant to `LspCommand` in `cli.rs`**
   ```rust
   /// Find all references to a symbol via rust-analyzer
   References {
       /// Symbol to find references for (e.g., "Foo::bar", "CrateModel")
       symbol: String,
       /// Location-only output format
       #[arg(long, short)]
       quiet: bool,
   },
   ```
   - Delegation: main
   - Depends on: none

2. **Add protocol types in `protocol.rs`**
   - Add `DaemonRequest::References { symbol: String, quiet: bool }`
   - Add `DaemonResponse::QueryResult { output: String }` — pre-formatted output
   - Delegation: main
   - Depends on: none

3. **Create `src/lsp/query.rs` — core query logic**
   - `SymbolMatch` struct: `{ name: String, container_name: Option<String>, uri: String, line: u32, col: u32, kind: String }`
     - `kind` is mapped from LSP `SymbolKind` integer: 5→"struct", 6→"fn", 10→"enum",
       11→"trait", 12→"fn" (method), 13→"const", others→"symbol"
   - `ReferenceLocation` struct: `{ uri: String, line: u32, col: u32 }`
     - **All line/col fields are 0-indexed** as returned by LSP. Add `+1` for display.
   - `resolve_symbol(transport: &mut RaTransport, query: &str) -> Result<ResolveResult>`
     where `ResolveResult = Ok(SymbolMatch) | Ambiguous(Vec<SymbolMatch>) | NotFound`
     - Sends `workspace/symbol` with `{ "query": query }`
     - **Important:** `send_request_and_wait()` returns the full JSON-RPC envelope.
       Access symbol data via `response["result"]`, not the top-level response.
     - Parses `result[]` as `SymbolInformation` (fields: `name`, `kind`, `location.uri`,
       `location.range.start.line`, `location.range.start.character`, `containerName`)
     - Filtering: extract last `::` segment from query string. Keep only results where
       `name` matches that segment exactly (case-sensitive). If query has `::`, also
       filter by `containerName` containing earlier segments.
     - If 0 results after filtering → `NotFound`
     - If 1 result → `Ok(match)`
     - If multiple → `Ambiguous(matches)`
   - `find_references(transport: &mut RaTransport, uri: &str, line: u32, col: u32) -> Result<Vec<ReferenceLocation>>`
     - Sends `textDocument/references` with:
       ```json
       {
         "textDocument": { "uri": uri },
         "position": { "line": line, "character": col },
         "context": { "includeDeclaration": false }
       }
       ```
     - Parses `result[]` as `Location` (fields: `uri`, `range.start.line`,
       `range.start.character`)
   - `format_references(refs: &[ReferenceLocation], workspace_root: &Path, symbol_name: &str, quiet: bool) -> String`
     - **Quiet mode:** `@relative/path:line\n` for each reference (1-indexed lines)
     - **Normal mode:**
       ```
       // N references to SYMBOL

       // relative/path/file.rs
       42:   source line content
       87:   source line content

       // relative/path/other.rs
       115:  source line content
       ```
     - URI → path: use `uri.strip_prefix("file://")` (NOT `trim_start_matches`)
     - Relative path: strip `workspace_root` prefix
     - Source lines: read files from disk, get line at position (0-indexed → 1-indexed
       for display). If file unreadable, show `<source unavailable>`.
     - Group references by file, sort by line within each file.
     - Line numbers: right-aligned to max width in file group: `format!("{:>width$}:  {}", line, content)`.
   - `format_disambiguation(matches: &[SymbolMatch], query: &str) -> String`
     - Shows numbered list of matches with file:line and kind
     - Example: `Multiple symbols match "bar":\n  1. fn Foo::bar  src/foo.rs:42\n  2. fn Baz::bar  src/baz.rs:10`
   - `handle_references(transport: &mut RaTransport, workspace_root: &Path, symbol: &str, quiet: bool) -> Result<String>`
     - Orchestrator: resolve_symbol → match on result → find_references → format
     - On `NotFound`: return error message
     - On `Ambiguous`: return disambiguation list
     - On `Ok`: find references, format output
   - Delegation: main
   - Depends on: step 1 (for testing fixture types only; no compile dep)

4. **Add `mod query;` to `src/lsp/mod.rs`**
   - Add `mod query;` declaration
   - Delegation: main
   - Depends on: step 3

5. **Modify `daemon.rs` — pass transport to handle_client**
   - Change `handle_client` signature:
     ```rust
     fn handle_client(
         stream: UnixStream,
         ra_status: RaStatus,
         start_time: Instant,
         shutdown: &mut bool,
         transport: &mut RaTransport,  // NEW
         workspace_root: &Path,         // NEW
     ) -> Result<()>
     ```
   - Add `use super::query;` import at top of `daemon.rs`
   - Add match arm for `DaemonRequest::References { symbol, quiet }`:
     ```rust
     DaemonRequest::References { symbol, quiet } => {
         match query::handle_references(transport, workspace_root, &symbol, quiet) {
             Ok(output) => DaemonResponse::QueryResult { output },
             Err(e) => DaemonResponse::Error { message: format!("{e}") },
         }
     }
     ```
   - Update `handle_client` call site in main loop to pass `&mut transport, workspace_root`
   - Note: while `handle_client` holds `&mut transport`, the FS event drain cannot run.
     This is acceptable — queries are brief and the debounce buffer accumulates events.
   - Delegation: main
   - Depends on: steps 2, 3, 4

6. **Add `cmd_references()` to `src/lsp/mod.rs`**
   - Function:
     ```rust
     fn cmd_references(workspace_root: &Path, symbol: String, quiet: bool, verbose: bool) -> Result<()> {
         ensure_daemon(workspace_root, verbose)?;
         let dir = daemon_dir(workspace_root);
         let sock = dir.join("lsp.sock");
         let mut stream = std::os::unix::net::UnixStream::connect(&sock)
             .context("Failed to connect to LSP daemon")?;
         stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
         let resp = send_command(&mut stream, DaemonRequest::References { symbol, quiet })?;
         match resp {
             DaemonResponse::QueryResult { output } => {
                 print!("{output}");
                 Ok(())
             }
             DaemonResponse::Error { message } => anyhow::bail!("{message}"),
             _ => anyhow::bail!("Unexpected response from daemon"),
         }
     }
     ```
   - Add match arm in `run_lsp_command()`:
     ```rust
     LspCommand::References { symbol, quiet } => {
         cmd_references(&metadata.workspace_root, symbol.clone(), *quiet, args.global.verbose)
     }
     ```
   - Update module doc comment to include `references`
   - Delegation: main
   - Depends on: steps 1, 2, 5

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `query.rs` (`format_references`) | Post-impl | Pure string formatting with synthetic data |
| `query.rs` (`format_disambiguation`) | Post-impl | Pure string formatting |
| `query.rs` (`resolve_symbol`, `find_references`) | Manual | Requires live ra process |
| `protocol.rs` (new variants) | Post-impl | Serde roundtrip |
| `daemon.rs` (handle_client changes) | Manual | Requires running daemon + ra |
| `mod.rs` (cmd_references) | Manual | Requires running daemon |

### Post-impl module: `format_references`
- **Key scenarios:**
  - Empty references list → header only ("// 0 references to X")
  - Single file, single reference → one file group, one line
  - Multiple files → grouped output with file headers
  - Multiple references in same file → sorted by line, consistent padding
  - Quiet mode → `@file:line` format
  - Unreadable source file → `<source unavailable>` placeholder
  - Line number padding → consistent width within each file group

### Post-impl module: `format_disambiguation`
- **Key scenarios:**
  - Two matches → numbered list
  - Match with containerName → shows qualified name

### Post-impl module: protocol roundtrip
- **Key scenarios:**
  - `DaemonRequest::References` roundtrip
  - `DaemonResponse::QueryResult` roundtrip

### Manual verification
1. `cargo brief lsp touch` — ensure daemon running
2. `cargo brief lsp references CrateModel` — should find references in test_fixture or cargo-brief itself
3. `cargo brief lsp references CrateModel -q` — location-only output
4. `cargo brief lsp references nonexistent_symbol` — "not found" error
5. Verify daemon stays alive after query (not crashed)

## Success Criteria
- `cargo brief lsp references <symbol>` resolves symbol and shows all references
- Output grouped by file with context lines (normal mode) or `@file:line` (quiet mode)
- Ambiguous symbols show disambiguation list
- Symbol not found returns clear error message
- 30-second timeout for ra processing (configurable daemon stays responsive)
- Daemon does not crash or hang on query failures (errors sent as `DaemonResponse::Error`)
- No new compilation warnings
- `cargo test` passes (no existing tests broken)
- `cargo clippy` clean (new files)
- Post-impl formatting tests pass
