---
title: "LSP daemon: query commands (references, blast-radius, call-hierarchy)"
---

# LSP daemon: query commands

**Parent:** `260326-feat-lsp-daemon`

## Goal

Implement the user-facing query commands that translate cargo-brief symbol
queries into LSP requests and format results as pseudo-Rust / cargo-brief
style output.

## Dependencies

- `260326-feat-lsp-daemon-bootstrap` (daemon + ra running)
- `260326-feat-lsp-file-watcher` (recommended but not blocking)

## Commands

### `cargo brief lsp references <symbol>`

- **LSP method:** `textDocument/references`
- **Flow:**
  1. Resolve `<symbol>` to a file position (see Symbol Resolution below)
  2. Send `textDocument/references` with `includeDeclaration: false`
  3. Collect `Location[]` results
  4. Format: group by file, show context lines, cargo-brief style

- **Output example:**
  ```
  // 12 references to Foo::bar

  // src/pipeline.rs
  42:   let result = foo.bar(ctx);
  87:   self.bar(input)

  // src/render.rs
  115:  item.bar(writer)
  ```

### `cargo brief lsp blast-radius <symbol>`

- **LSP methods:** `textDocument/references` + `callHierarchy/incomingCalls`
- **Flow:**
  1. Find all references to `<symbol>`
  2. For each reference, identify the containing function/method
  3. Optionally: recurse one level (callers of callers) for broader impact
  4. Deduplicate and group by module

- **Output example:**
  ```
  // Blast radius for Foo::bar (3 direct callers, 7 transitive)

  // Direct callers:
  //   src/pipeline.rs:  run_pipeline()
  //   src/render.rs:    render_item()
  //   src/search.rs:    search_index()

  // Transitive (callers of callers):
  //   src/lib.rs:       run_api_pipeline() → run_pipeline()
  //   src/lib.rs:       run_search_pipeline() → search_index()
  //   ...
  ```

### `cargo brief lsp call-hierarchy <symbol>`

- **LSP methods:** `callHierarchy/incomingCalls`, `callHierarchy/outgoingCalls`
- **Flow:**
  1. Resolve symbol → `callHierarchy/prepare`
  2. Fetch incoming and/or outgoing calls (flag-controlled)
  3. Format as tree

- **Output example:**
  ```
  // Call hierarchy for Foo::bar

  // Incoming (who calls bar):
  //   ← run_pipeline()        src/pipeline.rs:42
  //   ← render_item()         src/render.rs:115
  //     ← run_api_pipeline()  src/lib.rs:89

  // Outgoing (bar calls):
  //   → resolve_path()        src/resolve.rs:23
  //   → Model::lookup()       src/model.rs:156
  ```

## Symbol resolution

The biggest design question: how to go from a user-typed symbol string
(e.g., `Foo::bar`, `model::CrateModel`) to a file:line:column position
that LSP needs.

### Approach: workspace symbol search

1. Send `workspace/symbol` request with the symbol name
2. ra returns `SymbolInformation[]` with locations
3. If ambiguous (multiple matches), show disambiguation list
4. If unique, use its location for subsequent queries

This avoids cargo-brief needing to maintain its own symbol index.
`workspace/symbol` is fast after ra is warmed up.

### Fallback: tree-sitter resolution

If `workspace/symbol` is insufficient (e.g., for method names that need
type context), fall back to cargo-brief's existing `code` subcommand
resolution to find the definition location.

## CLI additions

```rust
pub enum LspCommand {
    Touch,
    Stop,
    Status,
    /// Find all references to a symbol
    References {
        symbol: String,
        #[arg(long, short)]
        quiet: bool,
    },
    /// Show edit impact of changing a symbol
    BlastRadius {
        symbol: String,
        /// Transitive depth (default: 1)
        #[arg(long, default_value = "1")]
        depth: u32,
    },
    /// Show call hierarchy for a symbol
    CallHierarchy {
        symbol: String,
        /// Show outgoing calls instead of incoming
        #[arg(long)]
        outgoing: bool,
    },
}
```

## Acceptance criteria

- `lsp references` finds all references including cross-module
- `lsp blast-radius` identifies direct and transitive callers
- `lsp call-hierarchy` shows incoming call tree
- Macro-expanded references are included (ra resolves these)
- Trait method dispatch references included
- Output grouped by file/module, cargo-brief pseudo-Rust style
- `--quiet` outputs location-only format (consistent with `code -q`)
- Ambiguous symbol → clear disambiguation prompt

## Estimated scope

~600-800 lines (symbol resolution + 3 query commands + output formatting)
