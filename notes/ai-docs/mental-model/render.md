# render — Pseudo-Rust Rendering Engine

**File:** `src/render.rs` (~1069 lines)

## Public API

### `render_module_api(model, target_module_path, args, observer_module_path, same_crate) -> String`

```rust
pub fn render_module_api(
    model: &CrateModel,
    target_module_path: Option<&str>,
    args: &BriefArgs,
    observer_module_path: Option<&str>,
    same_crate: bool,
) -> String
```

Entry point. Returns pseudo-Rust with `// crate {name}` header. If target module
not found, returns error comment listing available modules.

## Rendering Algorithm

1. **`render_module_contents()`** — recursive core:
   - Walks `model.module_children()` at each level
   - Filters by `is_visible_from()` (skips invisible items)
   - Dispatches to item-specific renderers
   - At depth limit: emits `mod name { /* ... */ }` stub
   - After children: calls `render_impl_blocks()` for module's types

2. **`render_impl_blocks()`** — collects impls from visible structs/enums/unions:
   - Skips synthetic/blanket unless `args.all`
   - Trait impls: shows only associated types/consts, body as `{ .. }`
   - Inherent impls: shows all visible methods/items

3. **`render_item()`** — dispatcher:
   - Emits doc comment via `render_docs()`
   - Matches `ItemEnum` → calls `render_struct`, `render_enum`, etc.

## Item Renderers

| Renderer | Key behavior |
|----------|-------------|
| `render_struct` | Handles Unit/Tuple/Plain variants. Visibility-filters fields. Hidden fields → `// .. private fields` or `{ .. }` |
| `render_enum` | Plain/Tuple/Struct variants. Private tuple fields → `_` |
| `render_trait` | Header with bounds, items: methods (`;` body), assoc types, assoc consts |
| `render_union` | Field visibility filtering, `// ... private fields` |
| `render_type_alias` | `type Name<G> = Type;` |
| `render_constant` | `const NAME: Type = value;` |
| `render_static` | `static [mut] name: Type = expr;` |
| `render_use` | `use source [as alias];` — if target ID is absent from the crate index (re-export of an external crate item), falls back to `pub use source::Name;` verbatim |
| Macro | `macro_rules! name { /* ... */ }` |
| Function | `[pub] [const] [async] [unsafe] fn name<G>(params) -> Ret;` |

## Type Formatting

- `format_type(ty: &Type) -> String` — handles all `rustdoc_types::Type` variants
  (ResolvedPath, DynTrait, Generic, Primitive, FunctionPointer, Tuple, Slice, Array,
  ImplTrait, RawPointer, BorrowedRef, QualifiedPath, etc.)
- `format_generics(g: &Generics) -> String` — `<T: Bound, U = Default, const N: usize>`
- `format_function_sig(name, f, vis) -> String` — full fn signature with self detection
- `format_visibility(vis) -> String` — `pub `, `pub(crate) `, `pub(in path) `, or empty
- `format_path`, `format_generic_args`, `format_generic_bound` — sub-formatters

## Output Conventions

- Function bodies → `;`
- Impl block bodies → `{ .. }` (trait) or expanded (inherent)
- Macro bodies → `{ /* ... */ }`
- Collapsed modules → `{ /* ... */ }`
- Hidden fields → `// .. private fields`
- Doc comments → `/// ...` (preserved verbatim)
- Indentation: 4 spaces per depth level

## Dependencies
- Internal: `model::CrateModel`, `model::is_visible_from`, `cli::BriefArgs`
- External: `rustdoc_types` (all item/type/generic types)
