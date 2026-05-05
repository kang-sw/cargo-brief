---
title: "Cross-crate module following + rustdoc JSON caching"
status: done
started: 2026-03-18
completed: 2026-03-18
---

# Cross-crate module following + rustdoc JSON caching

## Priority: P1

## Motivation

Facade crates like `bevy` re-export modules from sub-crates:
```rust
// bevy → bevy_internal → bevy_ecs, bevy_app, ...
pub use bevy_internal::*;
```

Currently `cargo brief --crates bevy` shows only `pub use bevy_internal::ecs;` lines.
Targeting modules (`bevy ecs`), `--recursive`, and `--search` all fail because:

1. The rustdoc JSON for `bevy` contains Import items pointing to external crates
2. `walk_modules()` skips unnamed re-exports (name=None at Item level)
3. Cross-crate import targets aren't resolvable from the source crate's JSON index
4. Module path lookup fails before any expansion runs

This blocks effective use on bevy, axum, and similar multi-crate ecosystems.

## Design

### Trigger rules (output safety)

| Invocation | Cross-crate? | Rationale |
|---|---|---|
| `bevy` (root, default depth) | No | Current behavior preserved |
| `bevy ecs` (module targeted) | ecs only | User explicitly targeted a module |
| `bevy --search Query` | All re-exported crates | Search is inherently exhaustive |
| `bevy --recursive` | All re-exported crates | Explicit opt-in |

Accidental `cargo brief bevy` remains fast and small.

### Phase 0: rustdoc JSON caching for `--crates` (standalone value)

**Problem:** Every invocation of `cargo brief --crates X` runs `cargo rustdoc`
even if the JSON already exists. For fixed-version crates, the output never changes.

**Fix:** In `generate_rustdoc_json()` (or a wrapper for remote pipeline), check if
`target/doc/<crate>.json` already exists before invoking `cargo rustdoc`. Skip
generation if present.

- Applies only to `--crates` (remote) pipeline where versions are locked
- `--no-cache` bypasses (existing flag — also deletes cached JSON)
- Local workspace pipeline always regenerates (source may have changed)

**Impact:** Repeat queries go from ~minutes to ~seconds (JSON parse only).
Also makes Phase 1-2 cheap since sub-crate JSONs accumulate in cache.

### Phase 1: On-demand single-module cross-crate following

When `cargo brief --crates bevy ecs` is invoked:

1. Parse bevy's JSON, find root module re-exports
2. Detect that `ecs` matches a `pub use bevy_internal::ecs` (or transitively `bevy_ecs`)
3. Resolve the actual crate name: parse bevy_internal's JSON to find `pub use bevy_ecs as ecs`
4. Generate `bevy_ecs` JSON in the same cached workspace (compilation already cached)
5. Parse and render `bevy_ecs`'s contents under the `ecs` module path

**Key detail:** The cached workspace already has all sub-crates compiled. Only
the rustdoc pass runs per sub-crate (~seconds). With Phase 0 caching, subsequent
calls are instant.

**Module resolution algorithm:**
```
user requests "ecs" →
  1. Check bevy's module_index → not found
  2. Scan root re-exports for name match:
     - pub use bevy_internal::* → expand glob, find "ecs" re-export
     - OR pub use X::ecs → direct match
  3. Resolve source crate: follow re-export chain to leaf crate (bevy_ecs)
  4. Generate + parse leaf crate's JSON
  5. Render leaf crate's root as virtual module "ecs"
```

### Phase 2: Multi-crate for `--search` and `--recursive`

When `--search` or `--recursive` is used with a facade crate:

1. Identify all cross-crate re-exports at root level
2. Generate JSON for each sub-crate (sequential — cargo build lock)
3. Parse all JSONs, merge search/render results
4. With Phase 0 caching, only first run is slow

### Phase 3: Fast binary cache (bincode)

`rustdoc-types` already derives `serde::Serialize` + `serde::Deserialize` unconditionally.
`bincode 1.x` works out of the box — it's even used in rustdoc-types' own test suite.

```toml
[dependencies]
bincode = "1"
```

```rust
// Write cache after JSON parse
let bytes = bincode::serialize(&krate)?;
std::fs::write(bin_path, &bytes)?;

// Read cache (skip JSON entirely)
let bytes = std::fs::read(bin_path)?;
let krate: rustdoc_types::Crate = bincode::deserialize(&bytes)?;
```

| Format | Parse 30MB crate | Code changes |
|---|---|---|
| serde_json | ~1-2s | current |
| bincode | ~100-200ms | ~30 lines + 1 dep |

**Why not rkyv?** `rustdoc-types` 0.57.2+ has `rkyv_0_8` feature, but zero-copy
requires working with `ArchivedCrate` types throughout the codebase — all field
accesses change. Full deserialization negates the benefit. bincode gives 5-10x
speedup with zero code changes beyond the cache layer.

**Cache file lifecycle:**
- After generating `target/doc/bevy_ecs.json`, also write `bevy_ecs.bin`
- On next load, if `.bin` exists and is newer than `.json`, load from `.bin`
- `--no-cache` and `--clean` clear both

### `--clean` cache management

With aggressive caching (workspace + JSON + bincode), users need a way to manage
disk usage. bevy's `target/` alone can be several GB.

```
cargo brief --clean              # delete entire ~/.cache/cargo-brief/
cargo brief --clean bevy         # delete only bevy's cached workspace
cargo brief --cache-info         # show cache size per crate (optional, P2+)
```

- `--clean` with no argument: remove `cache_dir()` entirely
- `--clean <spec>`: remove `cache_dir()/sanitize_spec(spec)/`
- Both print what was deleted and bytes freed
- Implement alongside Phase 0 (same module, trivial)

### Phase 4: Parallel JSON parsing (optional)

With cached binary files and multiple sub-crates to load:
```rust
use rayon::prelude::*;
let models: Vec<CrateModel> = crate_paths
    .par_iter()
    .map(|p| load_and_parse(p))
    .collect();
```

Only meaningful after Phase 2+3 when loading 10+ sub-crate models.
Adding `rayon` is a dependency cost — defer until profiling shows need.

## Implementation order

| Phase | Effort | Value | Dependencies |
|---|---|---|---|
| P0: rustdoc skip + bincode cache + --clean | Small | High (standalone) | None |
| P1: Single-module follow | Medium | High | P0 (for speed) |
| P2: Multi-crate search/recursive | Medium | Medium | P1 |
| P3: rayon parallel parse | Small | Low | P2 (meaningful only at scale) |

P0 and P1 are the high-ROI phases. P0 combines rustdoc generation skip (JSON exists)
with bincode binary caching — both are simple and complementary.

## Complexity

- P0: ~50 lines (JSON existence check + bincode serialize/deserialize + --clean + Cargo.toml dep)
- P1: ~100-150 lines (re-export chain resolution, sub-crate JSON generation, virtual module rendering)
- P2: ~50 lines on top of P1 (loop over all re-exports)
- P3: ~20 lines (rayon par_iter + Cargo.toml dep)

### Result - 26-03-18

Implemented Phase 0 (caching) + Phase 1 + Phase 2 (cross-crate) in a single pass as v0.4.0.

**What was implemented:**
- `--clean [SPEC]` CLI flag with `clean_cache()` in remote.rs
- `generate_rustdoc_json_cached()` — skips cargo rustdoc if JSON exists
- `parse_rustdoc_json_cached()` — bincode serialize/deserialize with mtime check
- `src/cross_crate.rs` module (~300 lines) with:
  - `resolve_cross_crate_module()` — single module targeting through re-export chains
  - `discover_all_reexported_crates()` — enumerate all sub-crates for search/recursive
  - `root_has_cross_crate_reexports()` — O(n) detection without JSON generation
- Integration in `run_remote_pipeline()`: module targeting, --search, --recursive all wired
- bincode dep added to Cargo.toml

**Deviations:** Phase 3 (rayon) deferred as planned — not enough sub-crates loaded concurrently to justify the dependency. Phase 4 (parallel JSON parse) also deferred.

**Key findings:** The `follow_use_chain` approach with max 5 hops + cycle detection handles bevy's 3-level chain (bevy → bevy_internal → bevy_ecs). Glob re-exports at root level require generating the glob source's JSON to discover named items within.

### Result (8da8648) - 26-03-18

Post-implementation fixes and enhancements after live testing with bevy.

**Fixes:**
- `root_has_cross_crate_reexports()` had false positives for intra-crate re-exports (`pub use self::*`). Added `is_intra_crate_source()` filter to all three entry points (detect, resolve, discover). (33226bb)
- `cargo brief --crates bevy ecs` silently failed — clap consumed `ecs` as `crate_name`, not `module_path`. Fixed: `run_remote_pipeline()` now detects when `crate_name != "self"` with `--crates` and treats it as module_path. (ee66c50)

**Enhancements:**
- Remote crate header now shows resolved version + features: `// crate bevy[0.18.1] features = ["bevy_winit"]`. Version read from Cargo.lock via line-based scanner. (8da8648)
- Restructured `run_remote_pipeline()` from early-return to single `let output = if/else` expression for cleaner post-processing.

**Known limitations:**
- `bevy` and `bevy@0.18` cache to separate directories even if they resolve to the same version (pre-existing `sanitize_spec` behavior).
- Cross-crate hop limit hardcoded at 5 — deep facade chains silently fall back to "module not found".
