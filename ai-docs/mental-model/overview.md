# Overview

## Entry Points
- `src/lib.rs` — `run_pipeline()` orchestrates everything; start here.
- `src/main.rs` — CLI dispatch only (dual invocation: `cargo brief` vs `cargo-brief`).

## Module Contracts
- `lib.rs` guarantees: two pipeline paths exist. **Local**: metadata → resolve → rustdoc → model → same_crate detection → render → glob expand. **Remote** (`--crates`): early exit to `run_remote_pipeline` which skips resolve and same_crate detection. No stage within a path may be reordered.
- `resolve` and `rustdoc_json` and `remote` are pure utilities with zero internal dependencies. They can be tested in isolation.
- `model` depends only on `rustdoc_types` (external). `render` depends on `model` + `cli`.
- `lib.rs` is the sole orchestrator — all cross-module data flow passes through it.

## Coupling
- `render` → `lib.rs`: Glob re-export output format must match exactly. `render_module_api()` emits `pub use {source}::*;\n` for top-level globs (no indent); `apply_glob_expansions()` searches for this exact string without indentation. Indented globs (from deeper modules) would not match — this coupling is fragile. Change either side without the other → globs silently remain unexpanded.
- `cli` → all test files: Every `BriefArgs` field must appear in every test helper (5 helpers across 7 test files). Adding a field causes compile errors (good — not silent).
- `lib.rs` → `resolve` + `rustdoc_json`: `manifest_path` is threaded through without validation. If it points to the wrong Cargo.toml, failure surfaces at JSON generation time, not at metadata loading.

## Extension Points & Change Recipes
- **Add a new `--no-*` filter flag**: Touch `cli.rs` (add field), `render.rs` (`should_render_item`), all test helpers (add field). Compile errors guide you.
- **Add a new item type**: Touch `render.rs` (add renderer + visibility check), `test_fixture/src/lib.rs` (add example), `tests/integration.rs` (add assertion). Missing the visibility check → private items leak silently.

## Common Mistakes
- Calling `render_item()` for a new item type without a preceding `is_visible_from()` check → private items appear in output.

## Technical Debt
- String-based glob detection/replacement in `apply_glob_expansions` — fragile, first-occurrence-only semantics. See `glob-expansion.md`.
- No progress indication for long-running `cargo rustdoc` subprocess calls.
