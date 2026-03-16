# Testing Infrastructure

## Entry Points
- `tests/integration.rs` — in-process tests using `test_fixture/` crate.
- `tests/subprocess_integration.rs` — binary invocation tests using `test_workspace/`.
- `tests/workspace_integration.rs`, `tests/external_crate_integration.rs`, `tests/facade_crate_integration.rs` — specialized in-process tests using `test_workspace/`.
- `tests/remote_crate_integration.rs` — `#[ignore]` network tests for `--crates`.

## Module Contracts
- `fixture_model()` (integration.rs) guarantees: generates rustdoc JSON from `test_fixture/` and returns a `CrateModel`. Shared setup — called once per test.
- `render_full()` / `render_module()` helpers always pass `same_crate=true`. Tests needing cross-crate visibility MUST call `render_module_api()` directly with `same_crate: false`.
- All test helpers (`default_args`, `workspace_args`, `either_args`, `facade_args`, `remote_args`) enumerate all `BriefArgs` fields explicitly. Adding a field causes compile errors across all helpers (intentional — forces update).

## Coupling
- `BriefArgs` fields → 5 test helpers: Each helper constructs the full struct. Adding/removing a field requires updating all 5 helpers. Compile errors enforce this.
- Fixture crate names → test assertions: Crate names (`"test-fixture"`, `"core-lib"`, `"app"`) are string literals in both Cargo.toml and test code. Renaming a fixture crate requires updating all references manually — runtime failure, not compile-time.
- `test_fixture/src/lib.rs` structure → assertion strings: Tests assert on exact item names (`"pub struct PubStruct"`, `"pub enum PlainEnum"`). Renaming/removing items in the fixture breaks assertions at runtime.
- External dependency versions: `either = "=1.15.0"` is pinned. Tests assert exact method signatures (`pub fn is_left(&self) -> bool`). Version changes → assertion failures.

## Extension Points & Change Recipes
- **Add a new item type to fixture**: Add to `test_fixture/src/lib.rs`, add integration test in `tests/integration.rs`, add to `--no-*` flag tests if applicable.
- **Add a new test file**: Create helper that constructs full `BriefArgs`. Must include ALL fields or won't compile.

## Common Mistakes
- Using `render_full()` for cross-crate visibility tests → `same_crate=true` is hardcoded, test passes incorrectly showing `pub(crate)` items.
- Setting `args.depth = 0` without `args.recursive = false` → depth is ignored because `recursive=true` overrides to `u32::MAX`.
- Setting `args.at_package` without matching the `same_crate` parameter when calling `render_module_api()` directly → inconsistent visibility context.
- Workspace tests using `manifest_path: Some("test_workspace/Cargo.toml")` — must point to workspace ROOT, not individual package Cargo.toml.
- Ignored tests (`#[ignore]`): If the blocked feature is later implemented, the `#[ignore]` attribute must be removed manually. No CI check for this.

## Technical Debt
- No tests for `pub(super)` or `pub(in path)` visibility from various observer positions. The fixture defines `pub(in crate::outer) struct InnerRestricted` but no test verifies its visibility behavior.
- `render_full()` and `render_module()` hide the `same_crate` parameter. ~40 tests use these helpers and implicitly assume same-crate context.
