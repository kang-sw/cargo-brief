# Phase 1: Core Implementation

## Design Goals

Implement the core pipeline of cargo-brief:
1. Parse CLI arguments
2. Invoke `cargo +nightly rustdoc` to generate JSON
3. Parse rustdoc JSON into an internal item tree
4. Filter items by visibility from `--at-mod` perspective
5. Render filtered items as pseudo-Rust output

## Key Technical Findings

### Rustdoc JSON Format (v57)

Visibility representation in JSON:
- `"public"` → `pub`
- `"crate"` → `pub(crate)` (also normalized from `pub(super)` when super=crate root)
- `"default"` → impl blocks, trait method impls (no explicit visibility)
- `{"restricted": {"parent": <module_id>, "path": "<path>"}}` → `pub(super)`, `pub(in path)`, and private items

The `parent` ID in `restricted` points to the module the item is visible within.

### Nightly Requirement

`--output-format json` requires nightly toolchain (`cargo +nightly rustdoc -- --output-format json -Z unstable-options`).
Also requires `--document-private-items` for full visibility metadata.

### Dependencies

- `rustdoc-types` v0.57.x — matches format version 57, official types crate
- `clap` with derive — CLI argument parsing
- `serde` + `serde_json` — JSON deserialization
- `anyhow` — error handling

## Module Structure

```
src/
  main.rs          — entry point, orchestration
  cli.rs           — CLI argument definitions (clap derive)
  rustdoc_json.rs  — invoke cargo rustdoc, parse JSON, build item tree
  model.rs         — internal item representation (simplified from rustdoc-types)
  visibility.rs    — visibility filtering logic
  render.rs        — pseudo-Rust output formatter
```

## Session Breakdown

### Session 1: Foundation (COMPLETED)
All core modules implemented in a single pass:
- [x] Investigate rustdoc JSON format with test fixture
- [x] Set up Cargo.toml with dependencies (clap, rustdoc-types 0.57, serde_json, anyhow)
- [x] Implement `cli.rs` — clap derive args (cargo subcommand pattern)
- [x] Implement `rustdoc_json.rs` — invoke nightly rustdoc, locate JSON output, deserialize
- [x] Implement `model.rs` — CrateModel with module tree walking, visibility check
- [x] Implement `render.rs` — full pseudo-Rust output formatter
- [x] Visibility filtering (public, crate, restricted) working
- [x] Impl blocks rendered from struct/enum `impls` field (not module children)
- [x] `pub use` re-exports rendered (name from inner Use struct, not item.name)
- [x] `--all` / default exclusion of blanket+synthetic impls working
- [x] `--no-*` flags working
- [x] `--depth` / `--recursive` working
- [x] `--at-package` for external perspective tested
- [x] Doc comment preservation working
- [x] Unit tests for model and rustdoc_json

### Remaining Work
- [ ] More unit tests for visibility edge cases
- [ ] Integration tests (automated, against test fixture)
- [ ] `--at-mod` with specific module path (currently defaults to crate root)
- [ ] Error handling for missing nightly, missing crate, etc.
- [ ] Edge cases: empty modules, no matching items
- [ ] `has_stripped_fields` indicator for structs with hidden private fields

## Visibility Filtering Algorithm

Given `--at-mod M` in `--at-package P`, for each item `I` in target module:

1. `I.visibility == "public"` → **visible**
2. `I.visibility == "crate"` → **visible** iff `P` == target crate
3. `I.visibility == {"restricted": {"parent": mod_id, ...}}`:
   - Resolve `mod_id` to a module path
   - **visible** iff `M` is within (descendant of or equal to) that module
4. `I.visibility == "default"` → context-dependent:
   - For impl blocks: visibility is determined by the impl's type + trait
   - For items inside impl blocks: inherits from the impl context

For **external crates** (Phase 2): only `"public"` items are ever visible.
