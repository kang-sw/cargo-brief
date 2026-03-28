---
title: "LSP symbol resolution: grep-based fallback for workspace/symbol misses"
status: todo
---

# LSP grep-based fallback symbol resolution

## Problem

`workspace/symbol` only returns workspace-defined symbols. External dependency
types (`hecs::World`, `bevy::App`), common method names (`new`, `get`), and
qualified `Type::method` patterns fail with "Symbol not found" even though
rust-analyzer fully knows about them after indexing.

## Solution

Add a fallback path to `resolve_symbol` when `workspace/symbol` returns no
results:

1. ripgrep workspace `.rs` files for the symbol name (`--word-regexp`)
2. Sample candidate usage sites (prefer `use` imports, then type annotations)
3. `textDocument/definition` at each candidate to get semantic definition site
4. Deduplicate by definition location
   - 1 unique target → `ResolveResult::Ok`
   - N unique targets → `ResolveResult::Ambiguous` (existing disambiguation UI)
   - 0 targets → `ResolveResult::NotFound`

Qualified names (`hecs::World`) narrow the grep pattern for better precision.

## Scope

- Modify `resolve_symbol()` in `src/lsp/query.rs`
- May need `textDocument/didOpen` before definition requests
- Need workspace root path threaded into resolve_symbol (currently only gets transport)
- Grep via `std::process::Command` calling `rg`, or read files + regex in-process
