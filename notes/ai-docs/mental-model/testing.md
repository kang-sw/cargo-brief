# testing — Test Infrastructure

## Integration Tests (`tests/integration.rs`)

### Setup Helpers

- **`fixture_model()`** — loads metadata from `test_fixture/Cargo.toml`, generates rustdoc JSON,
  parses it, builds `CrateModel`. Single shared setup for all tests.
- **`default_args()`** — baseline `BriefArgs` with `recursive: true`, all filters off,
  targeting `test-fixture` with nightly toolchain.
- **`render_full(model)`** — renders entire crate, same-crate context.
- **`render_module(model, path)`** — renders specific module.

### Test Categories (~30+ tests)

| Category | What's tested |
|----------|--------------|
| Structs | Field visibility (pub/crate/super/private), private struct hiding, external view |
| Enums | Plain, tuple, struct variant rendering |
| Functions | Regular, async, const, unsafe qualifiers |
| Generics | Generic structs (bounds, defaults), traits (supertraits), functions |
| Traits | Definition rendering, impl block condensing (`{ .. }`) |
| Constants/Statics | `const`, `static`, `static mut` |
| Macros | `macro_rules!` with `#[macro_export]` |
| Unions | `#[repr(C)]` union fields |
| Re-exports | `pub use ... as Alias` |
| Doc comments | Preservation on all item types |
| Depth control | depth=0 collapses all, depth=1 expands root only |
| Item filtering | Each `--no-*` flag tested individually |
| Module navigation | Targeting `outer`, `outer::inner` |
| Visibility model | same_crate=true shows pub(crate), same_crate=false hides them |
| Formatting | Crate header, indentation, inherent impls, trait impl one-liners |

## Subprocess Integration Tests (`tests/subprocess_integration.rs`)

Tests that invoke the `cargo-brief` binary via `std::process::Command` with explicit
working directories. This exercises the full pipeline including cwd detection, `self`
resolution, and arg parsing — things in-process tests cannot cover.

### Fixture

Uses `test_workspace/` (workspace with `core-lib` + `app` + `either` dependency).

### Helpers

- **`cargo_brief_bin()`** — binary path via `CARGO_BIN_EXE_cargo-brief`
- **`run(cwd, args)`** — returns `(stdout, stderr, success)`
- **`run_ok(cwd, args)`** — asserts success, returns stdout
- **`run_err(cwd, args)`** — asserts failure, returns stderr

### Test Categories (23 tests: 22 passing, 1 ignored)

| Category | Tests | Status |
|----------|-------|--------|
| A. Explicit crate name | `explicit_core_lib`, `explicit_app`, `explicit_underscore_normalization` | passing |
| B. `self` keyword | `self_from_core_lib`, `self_from_app`, `self_module_from_core_lib`, `self_from_virtual_root` | passing |
| C. `crate::module` syntax | `crate_module_syntax` | passing |
| D. File path as module | `file_path_from_package_dir`, `self_with_file_path`, `pkg_with_file_path` | 2 passing, 1 ignored |
| E. External crate | `external_crate_either` | passing |
| F. Visibility auto-detection | `auto_visibility_cross_crate`, `auto_visibility_same_crate`, `auto_visibility_reverse` | passing |
| G. `--at-package` override | `at_package_cross_crate`, `at_package_same_crate` | passing |
| H. Depth/recursion | `depth_zero`, `recursive` | passing |
| I. Item filtering | `no_structs`, `no_functions` | passing |
| J. Error cases | `nonexistent_crate`, `self_from_non_package` | passing |

### Ignored Tests

- **D.pkg_with_file_path:** `#[ignore = "blocked: file path not resolved relative to package dir when cwd != package dir"]`

## Remote Crate Integration Tests (`tests/remote_crate_integration.rs`)

All 4 tests are `#[ignore = "network: fetches from crates.io"]`. Run with `cargo test -- --ignored`.

Tests exercise `run_pipeline` with `BriefArgs { crates: Some(spec), .. }`:
- `remote_serde_latest` / `remote_serde_pinned` — check output contains expected trait names
- `remote_nonexistent` — expects `Err`
- `remote_with_module_path` — verifies module-not-found error message in output

When constructing `BriefArgs` directly in tests, `crate_name` must be set (e.g., `"self"`) even though it is ignored by the remote pipeline path.

## Test Fixture (`test_fixture/`)

Single-crate library (`test_fixture/src/lib.rs`, ~151 lines) exercising all supported
item types with varying visibility levels.

**Structure:** `pub mod outer { pub mod inner { ... } ... }` with root-level re-export.

**Coverage:** All struct kinds (unit/tuple/plain), all enum variants, traits with
associated types, generics with bounds/defaults, function qualifiers, macros,
statics, unions, re-exports, and doc comments on every item type.
