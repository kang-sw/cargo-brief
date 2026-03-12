# model — CrateModel & Visibility Resolution

**File:** `src/model.rs`

## Core Type

### `CrateModel`
```rust
pub struct CrateModel {
    pub krate: rustdoc_types::Crate,           // Original parsed JSON (items in krate.index)
    pub module_index: HashMap<String, Id>,      // "crate::mod::sub" → module Id
    pub item_module_path: HashMap<Id, String>,  // Id → containing module path (currently unused)
}
```

## Public Methods

### `CrateModel::from_crate(krate: Crate) -> Self`
Constructs model by recursively walking module tree via `walk_modules()`.
Populates `module_index` and `item_module_path`.

### `crate_name(&self) -> &str`
Root crate name from `krate.index[krate.root].name`. Falls back to `"unknown"`.

### `find_module(&self, module_path: &str) -> Option<&Item>`
Looks up by relative path (e.g., `"outer::inner"`). Tries full-qualified path first
(`crate_name::path`), then relative.

### `root_module(&self) -> Option<&Item>`
Returns `krate.index[krate.root]`.

### `module_children<'a>(&'a self, module_item: &'a Item) -> Vec<(&'a Id, &'a Item)>`
Direct children of a module. Returns empty vec for non-modules.

### `module_path(&self, module_id: &Id) -> Option<&str>`
Reverse lookup: Id → module path. O(n) scan of `module_index`.

### `is_ancestor_or_equal(ancestor_path: &str, descendant_path: &str) -> bool`
Static. `"foo"` is ancestor of `"foo::bar"` but not `"foobar"`.

## Visibility Function

### `is_visible_from(model, item, _item_id, observer_module_path, same_crate) -> bool`

```rust
pub fn is_visible_from(
    model: &CrateModel,
    item: &Item,
    _item_id: &Id,
    observer_module_path: &str,
    same_crate: bool,
) -> bool
```

| Visibility | Rule |
|---|---|
| `Public` | Always visible |
| `Crate` | Visible iff `same_crate` |
| `Restricted { parent, .. }` | Visible iff `same_crate` AND observer is within restricted scope (via `is_ancestor_or_equal`) |
| `Default` | Not visible (impl items delegated to parent type) |

## Dependencies
- External: `rustdoc_types` (`Crate`, `Id`, `Item`, `ItemEnum`, `Visibility`)
- Internal: none
- Used by: `render` module, `lib.rs::run_pipeline()`
