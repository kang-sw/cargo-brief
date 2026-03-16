# Feature: `--doc-lines N` (doc comment line limit)

## Priority: P1

## Motivation

Large crates have extreme doc comment overhead:
- tokio: full=1123 lines, no-docs=183 lines (**84% doc comments**)
- http: 2324 lines total, dominated by method doc examples
- bytes `Bytes` struct: ~80 lines including ASCII art memory diagrams
- tokio macros (`pin!`, `try_join!`): ~100 lines of usage examples each

`--no-docs` is too extreme — removes all context. LLMs need the **first line
or paragraph** to understand what an item does, but not 80-line example blocks.

## Design

### CLI

```
--doc-lines <N>   Limit doc comments to first N lines (0 = suppress all)
```

Under "Filtering" help heading, alongside `--no-docs` and `--compact`.

### Behavior

- `--doc-lines 1` — show only the first non-empty line of each doc comment
- `--doc-lines 3` — show up to 3 lines (covers most summary paragraphs)
- `--doc-lines 0` — equivalent to `--no-docs`
- Default (no flag) — show full doc comments (current behavior)
- `--no-docs` is kept as shorthand for `--doc-lines 0`
- `--compact` continues to imply no docs (no change)

### Implementation

Modify `render_docs()` in `src/render.rs`:
```rust
fn render_docs(item: &Item, indent: &str, args: &BriefArgs, output: &mut String) {
    if args.no_docs || args.compact { return; }
    if let Some(docs) = &item.docs {
        let max_lines = args.doc_lines.unwrap_or(usize::MAX);
        for (i, line) in docs.lines().enumerate() {
            if i >= max_lines { break; }
            // ... existing formatting ...
        }
    }
}
```

Also applies to search mode's single-line doc rendering in `search.rs`
(`render_leaf` already shows only first line — unaffected by this feature).

### Token savings estimates

| Crate | Full | `--doc-lines 1` | `--no-docs` |
|-------|------|-----------------|-------------|
| tokio (6 features) | 1123 | ~350 (est) | 183 |
| http@1 | 2324 | ~600 (est) | ~400 (est) |
| anyhow@1 | ~280 | ~80 (est) | ~50 |

## Files to Modify

| File | Changes |
|------|---------|
| `src/cli.rs` | Add `doc_lines: Option<usize>` field |
| `src/render.rs` | Modify `render_docs()` to respect line limit |
| `tests/integration.rs` | Add `doc_lines: None` to `default_args()`, add tests |
| 5 other test files | Add `doc_lines: None` to BriefArgs constructors |

### Result (2971dcb)

Implemented as designed. Added `doc_lines: Option<usize>` to BriefArgs, modified
`render_docs()` to enumerate and break at the limit. `doc_lines=0` returns early
(matches `--no-docs` behavior). All 6 test files updated, 2 new tests added.
No deviations from plan.
