---
title: "LSP symbol resolution: grep + definition fallback for external/method symbols"
plans:
  phase-1: 2026-03/28-2130.lsp-grep-fallback-resolution
---

# LSP grep-based fallback symbol resolution

## Problem

`resolve_symbol` uses `workspace/symbol` as its sole resolution strategy.
This LSP method only returns workspace-defined symbols, causing three
classes of failure:

1. **External dependency types** — `World`, `App`, `TcpStream` etc. are
   invisible to workspace/symbol even though ra has fully indexed them.
2. **Common method names** — `new`, `get`, `default` are not returned as
   individual workspace symbols by ra.
3. **Qualified method syntax** — `ActionRegistry::new` fails because ra's
   fuzzy search for the full string doesn't surface the method.

All downstream queries (`references`, `call-hierarchy`, `blast-radius`)
fail with "Symbol not found" in these cases, despite ra knowing everything
needed once given a position.

## Root Cause

The bottleneck is exclusively step 1 of the query pipeline: **name to
position** resolution. Once a (uri, line, col) position is obtained,
`textDocument/references`, `callHierarchy/*`, and `textDocument/definition`
all work perfectly on external deps and methods alike.

## Solution: grep + textDocument/definition fallback

When `workspace/symbol` returns no matches, fall back to:

```
1. Grep workspace .rs files for the symbol name (word-boundary match)
2. Sample N usage sites from grep results
3. For each site, send textDocument/definition to ra
4. Collect and deduplicate definition targets by (uri, line)
5. Route result through existing ResolveResult:
   - 0 definitions → NotFound
   - 1 unique definition → Ok(SymbolMatch)
   - N unique definitions → Ambiguous(Vec<SymbolMatch>)
```

### Key decisions

- **Grep is the candidate finder; ra is the resolver.** Grep provides
  positions; `textDocument/definition` provides semantic truth. False
  positives (comments, strings) are filtered out because definition
  returns empty for non-semantic positions.

- **Qualified name narrowing.** For `hecs::World`, grep for
  `hecs::World` or `use hecs::World` first. If no hits, fall back to
  bare `World`. This improves precision for qualified queries.

- **Disambiguation uses existing pattern.** `ResolveResult::Ambiguous`
  already displays a numbered list and does not execute the query.
  No new UX needed.

- **Sample size.** Check a bounded number of grep hits (e.g., 10-20)
  to avoid excessive LSP round-trips while still catching distinct
  symbols. Stop early if a unique definition is confirmed.

- **didOpen may be required.** ra might need `textDocument/didOpen` for
  files not yet opened before `textDocument/definition` works. Test
  and add if needed.

### Phase 1: Implement fallback in resolve_symbol

Scope: `src/lsp/query.rs` (primary), `src/lsp/daemon.rs` (if didOpen
plumbing needed).

1. Add grep-based candidate finder (ripgrep or `std::fs` walk + regex)
2. Add `textDocument/definition` helper to transport/query
3. Wire fallback into `resolve_symbol` after workspace/symbol misses
4. Handle qualified names (split on `::`, narrow grep pattern)
5. Test on gunpowder-odyssey repo:
   - `lsp references World` (external type)
   - `lsp references hecs::World` (qualified external type)
   - `lsp call-hierarchy new` with container disambiguation
   - `lsp blast-radius resolve_movement` (should still work, regression check)
6. Update lsp help text to remove "workspace-defined only" caveat
