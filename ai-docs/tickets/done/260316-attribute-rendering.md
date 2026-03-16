# Attribute / Metadata Rendering

## Goal

Render item attributes (`#[deprecated]`, `#[non_exhaustive]`, `#[must_use]`,
`#[repr(...)]`, etc.) in pseudo-Rust output. Two tiers:

1. **Default (always shown):** `#[deprecated]`, `#[non_exhaustive]` — these
   directly affect API usage decisions.
2. **Verbose (`--verbose-metadata`):** All supported attributes including
   `#[must_use]`, `#[repr(...)]`, `#[no_mangle]`, `#[macro_export]`, etc.

## Data Source

`rustdoc_types::Item` provides:
- `deprecation: Option<Deprecation>` — `{ since: Option<String>, note: Option<String> }`
- `attrs: Vec<Attribute>` — enum with variants:
  - `NonExhaustive`
  - `MustUse { reason: Option<String> }`
  - `Repr(AttributeRepr)` — kind (Rust/C/Transparent/Simd), align, packed, int
  - `MacroExport`
  - `NoMangle`
  - `ExportName(String)`
  - `LinkSection(String)`
  - `AutomaticallyDerived`
  - `TargetFeature { enable: Vec<String> }`
  - `Other(String)`

## Design

### CLI

- No new flag for default tier (always rendered).
- `--verbose-metadata` flag: render ALL attributes from the `attrs` vec.
- `--compact` and `--no-docs` do NOT suppress attributes (they affect API
  semantics, not documentation).

### Rendering

Add `render_attrs()` function in `render.rs`, called from `render_item()`
before `render_docs()`. Emits `#[...]` lines with proper indentation.

**Default tier (no flag needed):**
```rust
#[deprecated = "use new_fn instead"]
#[deprecated(since = "1.2.0", note = "use new_fn instead")]
#[non_exhaustive]
```

**Verbose tier (`--verbose-metadata`):**
```rust
#[must_use = "this returns a Result"]
#[repr(C)]
#[repr(C, align(8))]
#[repr(transparent)]
#[no_mangle]
#[export_name = "foo"]
#[macro_export]
```

### Search mode

Search results are one-liners — prepend `[deprecated]` or `[non_exhaustive]`
marker prefix when applicable.

## Files to Modify

| File | Changes |
|------|---------|
| `src/cli.rs` | Add `--verbose-metadata` flag |
| `src/render.rs` | Add `render_attrs()`, call from `render_item()` and `render_module_contents()` (for modules) |
| `src/search.rs` | Add deprecation/non_exhaustive markers to one-liner output |
| `test_fixture/src/lib.rs` | Add deprecated items, non_exhaustive enum |
| `tests/integration.rs` | Tests for attribute rendering |

### Result (cca95ed)

Implemented as designed. All changes in a single commit:

- `src/cli.rs`: Added `verbose_metadata: bool` field with `--verbose-metadata` flag
- `src/render.rs`: New `render_attrs()` (~65 lines) handling both tiers; wired into
  `render_item()`, `render_impl_item()`, and `render_module_contents()` (before docs)
- `src/search.rs`: Added `[deprecated]`/`[non_exhaustive]` inline markers in `render_leaf()`
- `test_fixture/src/lib.rs`: Added `deprecated_function`, `DeprecatedStruct`, `NonExhaustiveEnum`
- 7 new integration tests covering both tiers, search markers, and repr visibility
- Updated all 6 BriefArgs constructors across test files

No deviations from the plan. All 79 integration tests pass, no new clippy warnings.
