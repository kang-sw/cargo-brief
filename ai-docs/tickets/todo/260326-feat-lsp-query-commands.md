---
title: "LSP daemon: query commands (references, blast-radius, call-hierarchy)"
related:
  - 260326-feat-lsp-daemon             # parent
  - 260326-feat-lsp-daemon-bootstrap   # prerequisite
  - 260326-feat-lsp-file-watcher       # recommended but not blocking
---

# LSP daemon: query commands

## Goal

Implement the user-facing query commands that translate cargo-brief symbol
queries into LSP requests and format results as pseudo-Rust / cargo-brief
style output.

### Phase 1: Symbol resolution + references

Symbol resolution is the shared foundation — `workspace/symbol` to turn a
user-typed string (`Foo::bar`, `model::CrateModel`) into a file:line:column
position. `references` is the simplest consumer to validate it end-to-end.

**Symbol resolution strategy:**
- Primary: `workspace/symbol` request. ra returns `SymbolInformation[]` with
  locations. If unique, use directly; if ambiguous, show disambiguation list.
- Fallback: cargo-brief's existing `code` subcommand resolution for method
  names that need type context (workspace/symbol may not disambiguate
  `impl A::foo` vs `impl B::foo`).

**References command:**
- LSP method: `textDocument/references` (`includeDeclaration: false`)
- Output: grouped by file, context lines, cargo-brief pseudo-Rust style
- `--quiet`: location-only format (consistent with `code -q`)

Output example:
```
// 12 references to Foo::bar

// src/pipeline.rs
42:   let result = foo.bar(ctx);
87:   self.bar(input)

// src/render.rs
115:  item.bar(writer)
```

**Success criteria:**
- Symbol string → file position works for types, functions, methods, traits
- Ambiguous symbol → clear disambiguation list
- `lsp references` finds all references including cross-module
- Macro-expanded and trait dispatch references included (ra resolves these)

### Phase 2: blast-radius + call-hierarchy

Layers on top of Phase 1's symbol resolution and output formatting.

**blast-radius:**
- LSP methods: `textDocument/references` + `callHierarchy/incomingCalls`
- For each reference, identify containing function/method. Optionally recurse
  (configurable depth) for transitive impact.
- Output: direct callers + transitive callers, grouped by module.

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

**call-hierarchy:**
- LSP methods: `callHierarchy/prepare` → `incomingCalls` / `outgoingCalls`
- Flag-controlled direction (`--outgoing` for outgoing, default incoming)
- Output: tree format.

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

**Success criteria:**
- `lsp blast-radius` identifies direct and transitive callers
- `lsp call-hierarchy` shows incoming call tree
- Output grouped by file/module, cargo-brief pseudo-Rust style

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

## Estimated scope

~600-800 lines total. Phase 1 ~350-450, Phase 2 ~250-350.
