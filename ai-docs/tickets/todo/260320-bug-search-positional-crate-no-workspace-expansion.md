---
title: "umbrella crate (bevy) usability: search/api fails to follow re-exports and workspace members"
status: todo
---

## Summary

When working with umbrella crates like `bevy`, both `search` and `api`
subcommands fail to produce useful results because re-export chains and
workspace member crates are not traversed. This is the single biggest
usability gap for large ecosystem crates.

## Issues

### 1. Positional crate arg doesn't expand workspace members (high)

```sh
# Only searches top-level `bevy` crate — 0 results
cargo brief search bevy "ShaderRef" --compact

# --crates correctly expands to all workspace members — 16 results in bevy_shader
cargo brief search --crates bevy "ShaderRef" --compact
```

Positional-to-`--crates` normalization (v0.5.0, 38010c3) feeds a single exact
crate name without triggering workspace member expansion.

### 2. Re-export chain not followed for search (high)

```sh
cargo brief search bevy "Material"  # → 0 results
```

`bevy` re-exports via `bevy → bevy_internal → bevy_pbr`, but search doesn't
walk this chain. Users must already know which sub-crate defines the item.

### 3. Cross-crate module paths not resolved (high)

```sh
cargo brief api bevy::pbr  # → module 'pbr' not found. Available modules: bevy
```

`bevy::pbr` is the canonical user-facing path, but the tool only sees the
top-level module. Need to resolve re-exported module paths across crate
boundaries.

### 4. Glob re-export items missing (high)

```sh
# AsBindGroup trait lives in bevy_render::render_resource::bind_group
# but is not found via search or api — glob re-export (pub use bind_group::*)
# items are not collected into the parent module's reachable set
```

Note: v0.5.1 (50f096b) fixed intra-crate glob re-exports, but the fix may
not be recursive. A chain like `pub mod render_resource { pub use
bind_group::*; }` where `bind_group` itself re-exports from deeper modules
requires recursive glob expansion to surface items like `AsBindGroup`.
Verify whether the current implementation expands only one level or walks
the full depth.

### 5. Submodule direct access returns empty (medium)

```sh
cargo brief api bevy_render::render_resource::bind_group  # → empty output
```

Even when the sub-crate and full module path are specified, the submodule
renders as empty.

### 6. `--methods-of` is substring match — result explosion (medium)

```sh
cargo brief search bevy_pbr "Material" --methods-of "Material"
# → 651–803 results: ExtendedMaterial, MaterialPlugin, etc. all match
```

`--methods-of` should do exact type name matching, not substring.

### 7. Search doesn't support regex (low)

```sh
cargo brief search bevy_pbr "^Material$"  # → 0 results
```

Only substring matching is supported. Regex (or at least exact-match mode)
would help narrow results.

### 8. Zero-result crate headers should be verbose-only (medium)

```
// crate bevy_picking — search: "ShaderRef" (0 results)
// crate bevy_platform — search: "ShaderRef" (0 results)
// crate bevy_ptr — search: "ShaderRef" (0 results)
// ...
// crate bevy_shader — search: "ShaderRef" (16 results)
fn shader::ShaderRef::default() -> ShaderRef;
```

When workspace members are expanded, the output is flooded with `(0 results)`
lines for every sub-crate that didn't match. These headers should only appear
in `--verbose` mode. In normal mode, only crates with actual results should
be shown — making the expansion seamless to the user.

### 9. `--compact` hides trait method signatures (low)

```sh
# --compact collapses trait bodies to { .. }
# To see Material trait methods, must drop --compact — but then everything
# else becomes verbose too
```

Consider keeping method signatures visible in compact mode for traits, or
adding a middle-ground verbosity level.

### Out of scope

WGSL shader module introspection (e.g. `forward_io` structs like
`VertexOutput`) — not Rust API, but noted as a frequent need for bevy custom
material workflows.

## Implementation Note

Issues 1–5 share a common theme: the visibility/reachability engine doesn't
consistently follow re-export chains across crate boundaries or through
recursive glob expansions. Rather than patching each symptom individually,
this warrants a holistic revision of the re-export resolution logic to ensure:

- Workspace member expansion works uniformly (positional and `--crates`)
- Cross-crate `pub use` chains are walked transitively
- Glob re-exports (`pub use submod::*`) are expanded recursively, not just
  one level deep
- The resulting reachable set is the same regardless of which entry point
  the user specifies (umbrella crate vs sub-crate vs module path)

## Priority

Issues 1–5 are the most impactful — they all stem from the tool not following
re-export chains / workspace member relationships for umbrella crates. A
unified revision of re-export resolution would address most of them.
