---
title: "Batch rustdoc JSON generation via cargo doc + RUSTDOCFLAGS"
status: wip
started: 2026-03-21
---

## Problem

Cross-crate expansion (facade crates like `bevy`) invokes `cargo rustdoc -p <crate>`
separately for each sub-crate. Each invocation:
- Acquires/releases the workspace target dir lock
- Re-resolves dependencies
- Spawns a fresh cargo process

For bevy_pbr alone, 17+ sub-crate attempts occur sequentially.

## Solution

Use `cargo doc` with `RUSTDOCFLAGS` environment variable to batch-generate
rustdoc JSON for multiple crates in a single cargo invocation:

```bash
RUSTDOCFLAGS="--output-format json -Z unstable-options --document-private-items" \
  cargo +nightly doc -p bevy_render -p bevy_pbr --no-deps
```

**Verified**: produces valid JSON at `target/doc/{crate}.json`, same path as
`cargo rustdoc` output.

## Design

### Scope

Batch generation applies to **cross-crate expansion** paths only:
- `cross_crate.rs`: `follow_reexport_to_source()` and `expand_glob_reexports()`
- `lib.rs`: `discover_cross_crate_sources()` → `try_generate_rustdoc_json()`

The primary crate's own JSON is always generated individually (single `cargo rustdoc`
call, unchanged).

### Approach

1. **Collect phase**: Walk re-exports / glob sources to build a list of sub-crate
   names that need JSON generation.
2. **Filter cached**: Remove crates whose JSON already exists in `target/doc/`.
3. **Batch generate**: If any remain, invoke a single `cargo doc` with all `-p` flags
   and `RUSTDOCFLAGS`.
4. **Individual fallback**: Crates that fail in batch (e.g., internal-only crates)
   are silently skipped — same behavior as current per-crate `try_generate_rustdoc_json`.

### Key Details

- Output path: `target/doc/{crate_name}.json` — same as `cargo rustdoc`, no path changes.
- `--no-deps` flag: prevents documenting transitive deps (faster, less disk).
- `--document-private-items` only when `same_crate` — batch call needs to respect this.
  For cross-crate (external deps), private items are not needed.
- Error handling: batch `cargo doc` may partially succeed (some crates fail). Parse
  stderr for per-crate failures and report as warnings, same as current behavior.
- Cache check (`use_cache`) remains per-crate in the existing `generate_rustdoc_json`.

### API Change

Add to `rustdoc_json.rs`:
```rust
pub fn batch_generate_rustdoc_json(
    crate_names: &[&str],
    toolchain: &str,
    manifest_path: Option<&str>,
    document_private_items: bool,
    target_dir: &Path,
    verbose: bool,
) -> Vec<(String, Result<PathBuf>)>;
```

Callers in `cross_crate.rs` and `lib.rs` switch from loop-of-`generate_rustdoc_json`
to collect-then-batch.

## Non-Goals

- Changing the primary crate's generation path
- Parallel `cargo rustdoc` processes (target dir lock prevents this)
- Changing cache/bincode logic
