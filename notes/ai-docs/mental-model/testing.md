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

## Test Fixture (`test_fixture/`)

Single-crate library (`test_fixture/src/lib.rs`, ~151 lines) exercising all supported
item types with varying visibility levels.

**Structure:** `pub mod outer { pub mod inner { ... } ... }` with root-level re-export.

**Coverage:** All struct kinds (unit/tuple/plain), all enum variants, traits with
associated types, generics with bounds/defaults, function qualifiers, macros,
statics, unions, re-exports, and doc comments on every item type.
