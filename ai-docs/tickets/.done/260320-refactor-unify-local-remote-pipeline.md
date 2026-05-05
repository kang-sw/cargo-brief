---
title: "unify local/remote pipelines into shared post-resolution path"
status: done
started: 2026-03-21
completed: 2026-03-21
---

## Summary

Local and remote pipelines diverged into two separate code paths with different
semantic models, caching strategies, and feature sets. Features added to one
pipeline (cross-crate discovery, JSON/bincode caching) silently don't apply to
the other — this is the root cause of umbrella crate issues #1–#3 in
260320-bug-search-positional-crate-no-workspace-expansion.

## Problem

| Concern | Local pipeline | Remote pipeline |
|---------|---------------|-----------------|
| Target resolution | `CargoMetadataInfo` + `ResolvedTarget` | `WorkspaceDir` + bare strings |
| JSON caching | None (`generate_rustdoc_json`) | Bincode + JSON (`_cached` variant) |
| Cross-crate discovery | Not called | Conditional via flag checks |
| Glob expansion | Always, uncached | In `render_remote_normal`, uncached |
| Same-crate detection | `metadata.current_package` | Hardcoded `false` |

The only genuinely different part is the **entry point** — how the workspace is
obtained. Everything after resolution (JSON generation, parsing, cross-crate
discovery, glob expansion, render/search) is the same work duplicated with
divergent behavior.

## Goal

A single post-resolution pipeline shared by both local and remote entry points,
so that:

- Features (cross-crate, caching, glob expansion) exist once and apply
  everywhere
- Adding a capability to one entry point cannot silently skip the other
- Caching strategy is parameterized, not forked

## Phases

### Phase 1 — shared post-resolution context (plan mode)

Define a common context type that both local and remote entry points produce
after resolution. Merge the downstream processing (api/search/examples) into
shared functions that consume this context.

### Phase 2 — unified JSON generation and caching

Collapse `generate_rustdoc_json()` and `generate_rustdoc_json_cached()` into
a single entry point. Cache root becomes a parameter rather than a code path
fork.

### Phase 3 — cross-crate discovery in shared pipeline

Move `root_has_cross_crate_reexports()` → `discover_all_reexported_crates()`
into the shared pipeline so it fires uniformly regardless of entry point.
This directly resolves umbrella crate issues #1–#3.

## Design Decisions (pre-plan)

### Observer model

`observer_package: Option<String>` — single value. Virtual workspace root
yields `None` (external view, no same-crate privileges). No multi-observer
list — showing `pub(crate)` items for packages the caller isn't actually
inside would be misleading.

### Cross-crate expansion

Automatic inside the shared pipeline. After loading the primary crate model,
check `root_has_cross_crate_reexports()` and discover sub-crates without
caller intervention. Output volume / pagination concerns are out of scope
for this ticket.

### Caching strategy

Single `generate_rustdoc_json()` with `use_cache: bool`. The split is
**workspace member vs everything else**, not local vs remote:

- **Workspace member** (`package_name ∈ metadata.workspace_packages`):
  `use_cache = false` — always invoke `cargo rustdoc` (source may change;
  cargo's incremental build handles no-op case)
- **Non-member** (external dependency, remote crate): `use_cache = true` —
  skip `cargo rustdoc` entirely when JSON already exists (source is frozen)

Bincode parse cache (`parse_rustdoc_json_cached`) is always used regardless
of `use_cache` — it only depends on JSON file freshness.

## Constraints

- 117 integration tests must pass after each phase
- No observable behavior regression — output for existing commands must not
  change (except where bugs are fixed)
- Concrete interface types to be designed during Phase 1 planning, recorded
  in result section

## Related

- 260320-bug-search-positional-crate-no-workspace-expansion (motivation;
  root cause A is directly resolved by Phase 3)

### Result — 26-03-21

All three phases implemented in a single pass:

1. **PipelineContext struct** — shared context with `manifest_path`, `target_dir`, `package_name`, `module_path`, `observer_package`, `toolchain`, `verbose`, `use_cache`, `crate_header`. Both local and remote entry points produce this, then call shared functions.

2. **Unified JSON generation** — `generate_rustdoc_json_cached()` removed. Single `generate_rustdoc_json()` with `use_cache: bool`. Local callers pass `false`, remote/cross-crate pass `true`. All JSON parsing now uses `parse_rustdoc_json_cached()` (bincode always).

3. **Shared pipelines** — `run_shared_api_pipeline()` and `run_shared_search_pipeline()` called by both local and remote paths. Cross-crate discovery fires automatically for all crates with cross-crate re-exports. `expand_glob_reexports()` gains `use_cache` parameter.

**Removed**: `run_remote_api_pipeline`, `run_remote_search_pipeline`, `render_remote_normal`, `generate_rustdoc_json_cached`.

**Behavioral changes**: Local api/search now get cross-crate module resolution and recursive sub-crate expansion (previously remote-only). `at_mod` only passed when `same_crate=true` (semantically correct — no effect for external view).

**Tests**: 117/117 integration, 24/24 subprocess, 4/4 cli_smoke pass. 1 pre-existing workspace_integration failure (`core_lib_trait_impl_rendered`), 3 pre-existing flaky unit test failures (remote cache race condition).
