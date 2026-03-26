# LSP Query Commands Phase 2: blast-radius + call-hierarchy

## Context
- **Ticket:** `260326-feat-lsp-query-commands` Phase 2
- **Prerequisite:** Phase 1 (references) is complete. `resolve_symbol()`,
  `find_references()`, `format_references()`, `handle_references()` exist in
  `query.rs`. `RaTransport`, UDS protocol, daemon dispatch are all proven.
- **Goal:** Two new commands using LSP call hierarchy protocol:
  - `lsp blast-radius <symbol> [--depth N]` — direct + transitive callers
  - `lsp call-hierarchy <symbol> [--outgoing] [-q]` — call tree (incoming by default)
- **Key decision: shared call hierarchy infrastructure.** Both commands use
  `callHierarchy/prepare` → `incomingCalls`/`outgoingCalls`. `blast-radius`
  recurses incomingCalls to a configurable depth; `call-hierarchy` shows
  direct callers/callees in tree format.
- **Rejected:** Using `textDocument/references` + position heuristics for
  blast-radius. The call hierarchy protocol gives function-level granularity
  directly — no need to reverse-map reference positions to containing functions.
- **Addition beyond ticket spec:** `-q`/`--quiet` on both commands for
  consistency with `references` and `code`. Location-only output.
  Update ticket CLI section in the Result entry.
- **Depth semantics:** `--depth N` means N levels of `incomingCalls` hops.
  depth=1 (default) = direct callers only. depth=2 = direct + callers-of-callers.
  depth=0 is clamped to 1 (minimum useful value).
- **Depth cap:** Maximum depth is 10 to prevent runaway BFS.
- **Known risk:** `send_initialize` sends `"capabilities": {}`. The LSP spec
  requires advertising `callHierarchyProvider` in client capabilities, but ra
  is permissive and responds regardless. Noted as existing tech debt.

### LSP Call Hierarchy Protocol
```
callHierarchy/prepare(TextDocumentPositionParams)
  → CallHierarchyItem[] { name, kind, uri, range, selectionRange, data? }

callHierarchy/incomingCalls({ item: CallHierarchyItem })
  → CallHierarchyIncomingCall[] { from: CallHierarchyItem, fromRanges: Range[] }

callHierarchy/outgoingCalls({ item: CallHierarchyItem })
  → CallHierarchyOutgoingCall[] { to: CallHierarchyItem, fromRanges: Range[] }
```

The `data` field is opaque (ra-specific) and must be preserved when passing
a CallHierarchyItem back in subsequent requests.

### Output Formats

**blast-radius:**
```
// Blast radius for Foo::bar (3 direct, 5 transitive)
//
// Direct:
//   run_pipeline()          src/pipeline.rs:42
//   render_item()           src/render.rs:115
//   search_index()          src/search.rs:67
//
// Depth 2:
//   run_api_pipeline()      src/lib.rs:89  → run_pipeline()
//   run_search_pipeline()   src/lib.rs:134 → search_index()
```

**call-hierarchy (incoming, default):**
```
// Incoming calls to Foo::bar
//
// ← run_pipeline()          src/pipeline.rs:42
// ← render_item()           src/render.rs:115
// ← search_index()          src/search.rs:67
```

**call-hierarchy (outgoing):**
```
// Outgoing calls from Foo::bar
//
// → resolve_path()          src/resolve.rs:23
// → Model::lookup()         src/model.rs:156
```

**quiet mode (call-hierarchy):** `@file:line  function_name` per line.
**quiet mode (blast-radius):** `@file:line  function_name  [depth=N]` per line.
Note: this differs from `references` quiet mode which shows only `@file:line`
(no name). The name is included here because call hierarchy results represent
distinct functions, not raw reference positions.

## Relevant Files
- `src/lsp/query.rs` — Add `prepare_call_hierarchy()`, `incoming_calls()`,
  `outgoing_calls()`, `handle_blast_radius()`, `handle_call_hierarchy()`,
  and formatting functions. Reuse `resolve_symbol()` from Phase 1.
- `src/lsp/protocol.rs` — Add `DaemonRequest::BlastRadius` and
  `DaemonRequest::CallHierarchy` variants.
- `src/lsp/daemon.rs` — Add match arms in `handle_client()` for new requests.
- `src/lsp/mod.rs` — Add `cmd_blast_radius()`, `cmd_call_hierarchy()` dispatch
  functions, match arms in `run_lsp_command()`.
- `src/cli.rs` — Add `LspCommand::BlastRadius` and `LspCommand::CallHierarchy`
  variants.

## Conventions (verified from code)

- **Query handler pattern** (from `handle_references`):
  1. `resolve_symbol(transport, &symbol)` → match on ResolveResult
  2. On `Ok(match_)`: proceed with LSP calls using match_.uri/line/col
  3. On `Ambiguous(matches)`: return `format_disambiguation(&matches, &symbol, workspace_root)`
     (note: 3 args — `query` string is the second parameter)
  4. On `NotFound`: bail with "Symbol not found"
  5. Return formatted string (daemon wraps in `DaemonResponse::QueryResult`)

- **Daemon dispatch pattern** (from `DaemonRequest::References`):
  ```rust
  DaemonRequest::References { symbol, quiet } => {
      match query::handle_references(transport, workspace_root, &symbol, quiet) {
          Ok(output) => DaemonResponse::QueryResult { output },
          Err(e) => DaemonResponse::Error { message: format!("{e}") },
      }
  }
  ```

- **Client command pattern** (from `cmd_references`):
  ```rust
  ensure_daemon(workspace_root, verbose)?;
  let dir = daemon_dir(workspace_root);
  let sock = dir.join("lsp.sock");
  let mut stream = UnixStream::connect(&sock).context("...")?;
  stream.set_read_timeout(Some(Duration::from_secs(30)))?;
  let resp = send_command(&mut stream, DaemonRequest::Foo { ... })?;
  ```

- **Path relativization** (from `format_references`): `uri.strip_prefix("file://")`,
  then strip workspace_root prefix + "/" to get relative path.

- **Protocol roundtrip tests**: each new DaemonRequest variant gets a
  `#[test] fn roundtrip_*()` in `protocol::tests`.

## Implementation Steps

1. **CLI: Add `BlastRadius` and `CallHierarchy` to `LspCommand`**
   - `BlastRadius { symbol: String, depth: u32 (default 1), quiet: bool }`
   - `CallHierarchy { symbol: String, outgoing: bool, quiet: bool }`
   - Delegation: main
   - Depends on: none

2. **Protocol: Add new `DaemonRequest` variants**
   - `DaemonRequest::BlastRadius { symbol, depth, quiet }`
   - `DaemonRequest::CallHierarchy { symbol, outgoing, quiet }`
   - Add roundtrip tests for both.
   - Delegation: main
   - Depends on: none

3. **query.rs: Add call hierarchy LSP wrappers**
   - `prepare_call_hierarchy(transport, uri, line, col) -> Result<Vec<Value>>`
     Sends `callHierarchy/prepare` with TextDocumentPositionParams. Returns
     the raw JSON array (preserves `data` field for subsequent calls).
   - `incoming_calls(transport, item: &Value) -> Result<Vec<Value>>`
     Sends `callHierarchy/incomingCalls` with `{ item }`. Returns raw array.
   - `outgoing_calls(transport, item: &Value) -> Result<Vec<Value>>`
     Sends `callHierarchy/outgoingCalls` with `{ item }`. Returns raw array.
   - Keep return types as `serde_json::Value` — the `data` field is opaque
     and must be preserved for roundtripping to ra.
   - Delegation: main
   - Depends on: none

4. **query.rs: Add `handle_call_hierarchy` orchestrator + formatter**
   - `handle_call_hierarchy(transport, workspace_root, symbol, outgoing, quiet) -> Result<String>`
   - Flow: resolve_symbol → prepare_call_hierarchy → incoming_calls or
     outgoing_calls → format.
   - Formatting: `← name  relative_path:line` (incoming) or `→ name  path:line`
     (outgoing). Header: `// Incoming calls to <symbol>`.
   - Quiet mode: `@file:line  name` per line.
   - Edge case: prepare returns empty → "No call hierarchy found for <symbol>"
   - Edge case: prepare returns multiple items → use first item (ra may return
     multiple for overloads; first is the primary definition).
   - Formatting note: function names from `CallHierarchyItem.name` don't include
     `()` — the formatter appends `()` for display.
   - Delegation: main
   - Depends on: step 3

5. **query.rs: Add `handle_blast_radius` orchestrator + formatter**
   - `handle_blast_radius(transport, workspace_root, symbol, depth, quiet) -> Result<String>`
   - Flow: resolve_symbol → prepare_call_hierarchy → BFS/recursive
     incoming_calls up to `depth` levels.
   - BFS approach: collect callers at each depth level. Use a
     `HashSet<(String, u32)>` keyed on `(uri, selectionRange.start.line)` for
     deduplication — `selectionRange` is the function name position, stable
     across call sites. Track first-encountered parent for each node (for
     the `→ via_function()` annotation; if multiple parents exist, show
     whichever BFS discovered first).
   - Depth clamped: `depth = depth.clamp(1, 10)`.
   - Formatting: header with counts, then sections per depth level.
   - Quiet mode: `@file:line  name  [depth=N]` per line.
   - Edge case: prepare returns empty → "No call hierarchy found"
   - Edge case: prepare returns multiple items → use first item.
   - Delegation: main
   - Depends on: step 3 (LSP wrappers only; does NOT depend on step 4)

6. **daemon.rs: Add match arms in `handle_client`**
   - `DaemonRequest::BlastRadius { symbol, depth, quiet }` →
     `query::handle_blast_radius(transport, workspace_root, &symbol, depth, quiet)`
   - `DaemonRequest::CallHierarchy { symbol, outgoing, quiet }` →
     `query::handle_call_hierarchy(transport, workspace_root, &symbol, outgoing, quiet)`
   - Same Ok/Err wrapping pattern as References.
   - Delegation: main
   - Depends on: steps 4, 5

7. **mod.rs: Add `cmd_blast_radius` and `cmd_call_hierarchy` + dispatch**
   - Follow `cmd_references` pattern: ensure_daemon, fresh UDS connection,
     30s timeout, send_command, match QueryResult/Error.
   - Add match arms in `run_lsp_command()` for new `LspCommand` variants.
   - Delegation: main
   - Depends on: steps 1, 2, 6

**Note:** Steps 1-3 have no interdependencies and touch different files.
Steps 4-5 depend on step 3 (call hierarchy wrappers). Steps 6-7 depend on
all prior steps. However, all changes must be applied before the code compiles
(new CLI variants need daemon dispatch, protocol needs serde derives, etc.).
Implement sequentially in one pass.

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| CLI variants (cli.rs) | Manual | Clap derives — compilation is the test |
| Protocol roundtrips (protocol.rs) | Post-impl | Serde roundtrip, follows existing pattern |
| Call hierarchy LSP wrappers (query.rs) | Manual | Requires running ra instance |
| `handle_call_hierarchy` orchestrator | Manual | Requires daemon lifecycle |
| `handle_blast_radius` orchestrator | Manual | Requires daemon lifecycle |
| Formatting functions (query.rs) | Post-impl | Pure string formatting, testable with mock data |

### Post-impl: Protocol roundtrip tests
- `roundtrip_blast_radius_request` — serialize/deserialize BlastRadius variant
- `roundtrip_call_hierarchy_request` — serialize/deserialize CallHierarchy variant

### Post-impl: Formatting tests
- `format_call_hierarchy_incoming` — mock CallHierarchyItem data → expected output
- `format_call_hierarchy_outgoing` — direction arrow changes
- `format_call_hierarchy_empty` — "No call hierarchy found" message
- `format_call_hierarchy_quiet` — location-only output
- `format_blast_radius_depth_one` — single level of callers
- `format_blast_radius_depth_two` — two levels with via annotations
- `format_blast_radius_empty` — no callers found
- `format_blast_radius_quiet` — location-only with depth annotations

### Manual verification
1. `cargo brief lsp call-hierarchy resolve_symbol` — should show incoming callers
2. `cargo brief lsp call-hierarchy resolve_symbol --outgoing` — outgoing calls
3. `cargo brief lsp call-hierarchy resolve_symbol -q` — quiet mode
4. `cargo brief lsp blast-radius run_lsp_command` — direct callers
5. `cargo brief lsp blast-radius run_lsp_command --depth 2` — transitive
6. `cargo brief lsp blast-radius NonExistentSymbol` — error message

## Success Criteria
- `lsp call-hierarchy <symbol>` shows incoming callers with file locations
- `lsp call-hierarchy <symbol> --outgoing` shows outgoing calls
- `lsp blast-radius <symbol>` shows direct callers
- `lsp blast-radius <symbol> --depth N` shows transitive callers up to depth N
- `-q` flag produces location-only output for both commands
- Ambiguous symbol → disambiguation list (reuses existing resolve_symbol)
- Non-existent symbol → clear error message
- `callHierarchy/prepare` returning empty → graceful "no call hierarchy" message
- No new compilation warnings
- `cargo test` passes
- Protocol roundtrip tests pass
- Formatting unit tests pass
