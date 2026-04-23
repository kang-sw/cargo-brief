---
domain: Testing Infrastructure
description: BriefArgs coupling, fixture contracts, visibility test patterns, in-process vs subprocess
sources:
  - tests/integration.rs
  - tests/subprocess_integration.rs
  - tests/workspace_integration.rs
  - tests/external_crate_integration.rs
  - tests/facade_crate_integration.rs
  - tests/remote_crate_integration.rs
related:
  - visibility.md
---

# Testing Infrastructure

## Entry Points
- `tests/integration.rs` — in-process tests using `test_fixture/` crate.
- `tests/subprocess_integration.rs` — binary invocation tests using `test_workspace/`.
- `tests/workspace_integration.rs`, `tests/external_crate_integration.rs`, `tests/facade_crate_integration.rs` — specialized in-process tests using `test_workspace/`.
- `tests/remote_crate_integration.rs` — `#[ignore]` network tests for `-C` (crates.io mode).

## Module Contracts
- `fixture_model()` (integration.rs) guarantees: generates rustdoc JSON from `test_fixture/` (the workspace root) and returns a `CrateModel` for `test-fixture`. Shared setup — called once per test. The `test_fixture/` directory is itself a workspace (`glob-source` and `glob-inner` are members); the manifest path passed to `generate_rustdoc_json` must point to the workspace root `test_fixture/Cargo.toml`, not a member.
- `render_full()` / `render_module()` helpers always pass `same_crate=true`. Tests needing cross-crate visibility MUST call `render_module_api()` directly with `same_crate: false`.
- All test helpers (`default_args`, `workspace_args`, `either_args`, `facade_args`, `remote_args`) construct `ApiArgs` by composing `TargetArgs`, `FilterArgs`, and `GlobalArgs` plus per-subcommand fields. Remote mode is passed as a separate `&RemoteOpts` argument; local tests pass `RemoteOpts::default()`, remote tests construct `RemoteOpts { crates: true, features: ..., no_cache: ... }`. `integration.rs` separates `default_filter() -> FilterArgs` from `default_args() -> ApiArgs`. Adding a field to any composed struct causes compile errors across all helpers (intentional — forces update).
- `default_ts_args()` constructs `TsArgs` directly — it does NOT compose `TargetArgs` or `FilterArgs` (those are api/search only). `TsArgs` has its own flat field set: `crate_name`, `query`, `global`, `manifest_path`, `captures`, `context`. Adding a field to `TsArgs` will break only `default_ts_args()`, not other helpers.

## Coupling
- `FilterArgs` / `TargetArgs` / `GlobalArgs` fields → 5+ test helpers: Each helper constructs `ApiArgs` (or `SearchArgs`) by listing all fields of all composed structs. Adding/removing a field in any struct requires updating all helpers that use it. Compile errors enforce this. `RemoteOpts` is not a clap `Args` struct and is not flattened — it is constructed manually in test helpers that exercise crates.io mode.
- Fixture crate names → test assertions: Crate names (`"test-fixture"`, `"core-lib"`, `"app"`, `"glob-source"`, `"glob-inner"`) are string literals in both Cargo.toml and test code. Renaming a fixture crate requires updating all references manually — runtime failure, not compile-time.
- `test_fixture/src/lib.rs` structure → assertion strings: Tests assert on exact item names (`"pub struct PubStruct"`, `"pub enum PlainEnum"`). Renaming/removing items in the fixture breaks assertions at runtime.
- External dependency versions: `either = "=1.15.0"` is pinned. Tests assert exact method signatures (`pub fn is_left(&self) -> bool`). Version changes → assertion failures.

## Extension Points & Change Recipes
- **Add a new item type to fixture**: Add to `test_fixture/src/lib.rs`, add integration test in `tests/integration.rs`, add to `--no-*` flag tests if applicable.
- **Add a new fixture sub-crate** (e.g., for cross-crate glob testing): Add the crate directory under `test_fixture/`, register it in `test_fixture/Cargo.toml` under `[workspace] members` AND as a `[dependencies]` entry in the appropriate member. Without the workspace member entry, `cargo rustdoc -p` silently skips it.
- **Add a new test file**: Create a helper that constructs `ApiArgs` (or `SearchArgs`) spelling out all fields in the three composed structs (`TargetArgs`, `FilterArgs`, `GlobalArgs`). Must include ALL fields or won't compile. Pass `&RemoteOpts::default()` to pipeline calls for local mode.
- **Add tests for a new disk-only pipeline** (like `ts`): Add a `default_xxx_args()` helper in `tests/integration.rs` that constructs `XxxArgs` directly (flat struct, no `TargetArgs`/`FilterArgs` composition). Set `manifest_path` to point at `test_fixture/Cargo.toml` and `crate_name` to `"test-fixture"` to reuse the existing fixture.

## Common Mistakes
- Using `render_full()` for cross-crate visibility tests → `same_crate=true` is hardcoded, test passes incorrectly showing `pub(crate)` items.
- Setting `args.depth = 0` without `args.recursive = false` → depth is ignored because `recursive=true` overrides to `u32::MAX`.
- Setting `args.target.at_package` without matching the `same_crate` parameter when calling `render_module_api()` directly → inconsistent visibility context.
- Workspace tests using `manifest_path: Some("test_workspace/Cargo.toml")` — must point to workspace ROOT, not individual package Cargo.toml.
- Ignored tests (`#[ignore]`): If the blocked feature is later implemented, the `#[ignore]` attribute must be removed manually. No CI check for this.

## Technical Debt
- No tests for `pub(super)` or `pub(in path)` visibility from various observer positions. The fixture defines `pub(in crate::outer) struct InnerRestricted` but no test verifies its visibility behavior.
- `render_full()` and `render_module()` hide the `same_crate` parameter. ~40 tests use these helpers and implicitly assume same-crate context.
