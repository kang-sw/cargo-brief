---
title: "Resolve ambiguous multi-version crate specs in cross-crate JSON generation"
status: done
started: 2026-03-21
completed: 2026-03-21
---

## Problem

When a workspace's dependency tree contains multiple versions of the same crate
(e.g. bevy 0.18 pulls in `glam` 0.14–0.30, `hashbrown` 0.15+0.16, `foldhash`
0.1+0.2), both batch pre-warming and per-crate `cargo rustdoc -p <name>` fail
with "specification is ambiguous".

Symptoms:
- Batch pre-warming silently skips ambiguous crates (warning only)
- Per-crate generation in `build_cross_crate_index` also fails → items from
  those sub-crates are missing from the index
- Repeated "is ambiguous" errors spam stderr

## Root Cause

`generate_rustdoc_json` and `batch_generate_rustdoc_json` pass bare crate names
(e.g. `glam`) to `cargo rustdoc -p`. When multiple versions exist, cargo
requires a version-qualified spec (`glam@0.30.10`).

`load_lockfile_packages()` only stores names (`HashSet<String>`), discarding
version info. `normalize_to_lockfile_name()` returns bare names — no way to
disambiguate.

## Solution Direction

Use `cargo metadata` resolve graph (already called once via
`resolve::load_cargo_metadata()`) to build a parent→child version mapping.
When generating JSON for a dependency of crate X, look up which version of that
dependency X actually uses.

Key insight: the correct version depends on the **parent crate** that re-exports
it. `bevy_math` uses `glam@0.30.10`, even if other transitive deps use older
versions.

### Affected Code Paths

- `rustdoc_json::load_lockfile_packages()` — needs version info
- `lib.rs::normalize_to_lockfile_name()` — needs to return versioned spec when
  ambiguous
- `lib.rs::pre_warm_cross_crate_json()` — batch generation with versioned specs
- `cross_crate.rs::WalkContext` / `walk_accessible()` — per-crate generation
  needs parent context to pick the right version
- `rustdoc_json::batch_generate_rustdoc_json()` — handle `name@version` specs

### Constraints

- Single `cargo metadata` call policy (CLAUDE.md rule 3)
- `generate_rustdoc_json` already handles `@version` in cache lookup (line 21)
- Must not break the common case where only one version exists

### Result (pending) - 26-03-21

Implemented Cargo.lock version tracking + auto-retry approach (approach 2 from plan).

**What changed:**
- New `LockfilePackages` struct in `rustdoc_json.rs` replaces `HashSet<String>`.
  Tracks multi-version crates and resolves to `name@latest_version` via
  `resolve_spec()`. Uses `semver::Version::parse()` with string fallback.
- `load_lockfile_packages()` now parses both `name` and `version` from
  `[[package]]` blocks. Multi-version entries (2+ versions) sorted ascending.
- `PipelineContext.available_packages` type changed throughout — all 6
  construction sites propagate automatically.
- `normalize_to_lockfile_name()` simplified to single delegation.
- `cross_crate.rs` types updated: `build_cross_crate_index()` and `WalkParams`
  accept `&LockfilePackages`. Existing `.contains()` calls work via delegation.
- `@version` stripping added to 3 file-path construction sites (batch cache
  check, batch post-gen check, BFS file check).
- `generate_rustdoc_json()` auto-retries with highest-version spec on
  ambiguity (non-verbose path). Guards against infinite recursion via `@` check.

**Deviation from original ticket:** Used Cargo.lock parsing + latest heuristic
instead of `cargo metadata` resolve graph. Avoids second subprocess call or
removing `--no-deps` from existing call. Pre-warming resolves names upfront;
auto-retry in `generate_rustdoc_json` covers edge cases.

**Known limitation:** Verbose mode (`-v`) uses `Stdio::inherit()` and can't
capture stderr for auto-retry. Pre-warming resolves names before per-crate
calls, so this is not a practical issue.
