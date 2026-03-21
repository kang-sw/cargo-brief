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

### 3. Cross-crate module paths not resolved in local pipeline (high)

```sh
cargo brief api bevy::pbr  # → module 'pbr' not found. Available modules: bevy
```

`bevy::pbr` is the canonical user-facing path, but the tool only sees the
top-level module. The remote API pipeline *does* have partial support via
`cross_crate::resolve_cross_crate_module()` (lib.rs:487–527), but the local
pipeline never enters this code path. Root cause is shared with #1: positional
args use the local pipeline, which lacks cross-crate logic entirely.

### 4. Glob re-export items missing (high)

```sh
# AsBindGroup trait lives in bevy_render::render_resource::bind_group
# but is not found via search or api — glob re-export (pub use bind_group::*)
# items are not collected into the parent module's reachable set
```

v0.5.1 (50f096b) fixed intra-crate glob re-exports, but **confirmed
single-level only**: `expand_glob_reexports()` (lib.rs:747–798) collects
direct children of the source module's root and does not recurse into deeper
re-exports. A chain like `pub mod render_resource { pub use bind_group::*; }`
where `bind_group` itself re-exports from deeper modules will not surface
items like `AsBindGroup`.

### 5. Submodule direct access returns empty (medium)

```sh
cargo brief api bevy_render::render_resource::bind_group  # → empty output
```

Even when the sub-crate and full module path are specified, the submodule
renders as empty. Likely caused by #4: `render_resource` uses glob re-exports
to pull items from `bind_group`, but glob expansion is single-level and
doesn't populate the submodule's reachable set.

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

## Root Cause Analysis

Issues 1–5 stem from **two distinct root causes**:

### A. Local pipeline lacks cross-crate logic entirely

The local pipeline (`resolve_target()` → `generate_rustdoc_json()`) never
calls `root_has_cross_crate_reexports()` or `discover_all_reexported_crates()`.
The remote pipeline already has this wired up for both search (lib.rs:370–407)
and API (lib.rs:459–577). Positional args route through the local pipeline,
so umbrella crates like `bevy` get no sub-crate expansion.

**Fixes #1, #2, #3** — enabling cross-crate discovery in the local pipeline
(or unifying the two pipelines) would make positional args behave like
`--crates`.

### B. Glob re-export expansion is single-level

`expand_glob_reexports()` (lib.rs:747–798) collects only direct children
of the source module root. Nested re-export chains (`pub use submod::*` where
`submod` itself re-exports deeper) are not followed.

**Fixes #4, #5** — recursive glob expansion would surface deeply re-exported
items like `AsBindGroup`.

### Structural concern: local/remote pipeline divergence

The local and remote pipelines operate on different semantic models:
- **Local**: `CargoMetadataInfo` + `ResolvedTarget`, no cross-crate, no caching
- **Remote**: `WorkspaceDir` + bare strings, cross-crate discovery, JSON/bincode caching

This divergence means features added to one pipeline don't automatically
appear in the other — which is how #1–#3 happened. A longer-term unification
(shared `PipelineInput` + caching abstraction) would prevent this class of
bugs.

## Implementation Note

Rather than patching each symptom individually, this warrants addressing
the two root causes and considering pipeline unification:

- **Root cause A**: Enable cross-crate discovery in local pipeline, or
  unify local/remote into a shared pipeline that always has cross-crate
  support
- **Root cause B**: Make glob re-export expansion recursive
- **Structural**: Shared caching/expansion code path so local and remote
  can't silently diverge

## Priority

**Root cause A** (#1–#3) is highest priority — it's the most visible gap
and the remote pipeline already has the logic.

**Root cause B** (#4–#5) is next — requires recursive glob expansion.

**#6, #8** are independent bugs (substring matching, zero-result headers)
that can be fixed separately.

## Related

- 260320-refactor-unify-local-remote-pipeline: pipeline unification that
  structurally resolves root cause A (Phase 3)

### Result (Root Cause B) - 26-03-21

**Implemented:** Recursive cross-crate glob re-export expansion (fixes #4, #5).

- `expand_glob_reexports()` now recursively follows `is_glob=true` Use items
  in source crate roots via `collect_glob_items_recursive()` (max depth 8,
  cycle-safe via `visited: HashSet<String>`).
- New `try_generate_rustdoc_json()` helper handles underscore→hyphen package
  name fallback (rustdoc `use_item.source` gives Rust identifiers but
  `cargo -p` needs package names).
- `GlobExpansionResult.source_models` changed to `HashMap<String, Vec<CrateModel>>`
  to carry recursively discovered models alongside the direct source model.
- `apply_glob_expansions()` Phase 2 iterates all models in the Vec.
- Test fixture converted to workspace with `glob-source`/`glob-inner` sub-crates
  testing a 2-level cross-crate glob chain.
- 3 new integration tests (Phase 1, Phase 2, search).

**Deviation from plan:** Added `try_generate_rustdoc_json()` — not in original plan
but required because `use_item.source` uses underscores while cargo needs hyphens.

**Remaining:** Root Cause A (#1–#3), #6, #8 still open.
