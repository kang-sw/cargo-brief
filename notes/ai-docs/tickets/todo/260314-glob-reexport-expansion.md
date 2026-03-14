# Glob Re-export Expansion

## Goal

Facade crates like `clap` (which re-export everything via `pub use clap_builder::*`) currently
render as empty — only `// crate clap` appears. Make glob re-exports useful by expanding them.

## Background

Rustdoc JSON represents `pub use other_crate::*` as a single `use` item with `is_glob: true`.
The actual items from the source crate are **not inlined** into the JSON. This means facade
crates produce no meaningful output.

## Design

### Phase 1: Individual `pub use` enumeration (default behavior)

When a glob re-export is detected:

1. **Generate JSON** for the source crate (e.g., `clap_builder`) — cached by cargo, low cost.
2. **Enumerate** all pub items from the source crate's root module.
3. **Render** each as an individual `pub use source::ItemName;` statement.

Example output for `cargo brief clap`:
```rust
// crate clap
pub use clap_builder::Command;
pub use clap_builder::Arg;
pub use clap_builder::Parser;
pub use clap_derive::Parser;
// ...
```

This gives LLM agents a "table of contents" — they know what names are available through `clap`,
and can drill down via `cargo brief clap_builder::Command` for full definitions.

**One definition principle:** The full struct/trait/enum definition lives at the source crate.
The facade crate only shows `pub use` references. No duplication across invocations.

### Phase 2: `--expand-glob` flag (opt-in full inlining)

With `--expand-glob`, inline the full definitions from the source crate as if they were defined
in the facade crate. Deduplicate: if the same type appears via multiple re-export paths, show
the definition only once.

- Handles multiple glob sources (e.g., `clap` re-exports from both `clap_builder` and
  `clap_derive`)
- 1-depth expansion initially; recursive expansion can be added later if needed
- Semantically identical items (same type re-exported through different paths) are deduplicated

## Implementation Notes

- Glob re-exports are identified by `inner.use.is_glob == true` in rustdoc JSON
- Source crate name comes from `inner.use.source` field
- The additional `cargo rustdoc` call for the source crate uses the same toolchain/manifest-path
- Need to handle: multiple glob sources, mixed glob + explicit re-exports, source crate
  not having a lib target

## Testing

### Phase 1

- **Integration test: facade crate detection** — `cargo brief clap` (via `run_pipeline`)
  produces individual `pub use clap_builder::Command;` etc., not empty output.
- **Integration test: multiple glob sources** — verify that `clap`'s re-exports from both
  `clap_builder` and `clap_derive` are enumerated.
- **Integration test: mixed re-exports** — crate with both `pub use other::*` and explicit
  `pub use another::Specific;` renders both correctly.
- **Regression test: non-glob crates unchanged** — existing `either`, `core-lib`, `test-fixture`
  tests must continue to pass.

### Phase 2

- **Integration test: `--expand-glob` output** — full definitions inlined, no duplication.
- **Integration test: deduplication** — same type re-exported via multiple paths appears once.
- **Subprocess test: CLI flag** — `cargo brief clap --expand-glob` produces full definitions.

## Open Questions

- Should Phase 1 be the unconditional default, or opt-in? (Current plan: default)
- How to handle glob re-exports from `pub(crate)` modules that are re-exported at root?
  (These are already resolved by rustdoc — the glob target is the actual source crate)
- Depth limit for `--expand-glob` recursive expansion?

### Result (Phase 2)

**Implemented:** `--expand-glob` flag that inlines full definitions from glob re-export
source crates. Key implementation details:

- `GlobExpansionResult` struct in `lib.rs` preserves both item names (Phase 1) and source
  `CrateModel`s (Phase 2) from the same rustdoc JSON generation.
- `render_inlined_items()` in `render.rs` follows `Use` items to their targets to render
  actual definitions (not just re-export lines). Source crates like `clap_builder` re-export
  from submodules, so the root children are `Use` items, not direct definitions.
- Impl blocks collected from inlined types (both direct and via Use targets) and rendered
  inline with the definitions.
- Deduplication by item name via shared `HashSet<String>` across all glob source expansions.
- 5 new integration tests: full definitions present, no pub use lines remain, impl blocks
  present, dedup works, non-facade crates unaffected.
- All 139 tests pass (138 pass + 1 pre-existing ignored).
