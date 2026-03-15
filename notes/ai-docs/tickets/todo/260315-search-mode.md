# Search Mode (`--search` flag)

## Goal

Add a search mode that finds leaf items by name across the entire crate,
outputting one-line-per-item with full path. Designed for AI agents that
need imprecise/fuzzy lookup without knowing exact module paths.

## Motivation

Agents often don't know which module contains a particular function, type,
or field. Currently they must `--recursive` the entire crate and grep the
output. `--search` provides a first-class discovery mechanism: type a rough
keyword, get back a compact list of matching items with full paths that can
be used for further drill-down (`cargo brief crate::module`).

---

## Spec (committed in 0d9756f)

**CLI:** `cargo brief <TARGET> --search <PATTERN>` (also works with `--crates`)

**Matching:**
- Case-insensitive substring on the full display path
  (e.g., `world::World::spawn`)
- Multiple words → AND-matched (all must appear somewhere in the path)

**Output format:**
```
// crate <name> — search: "<pattern>" (N results)

/// doc comment first line (if present)
fn module::Type::method(&self, arg: T) -> Ret;
struct module::StructName;
field module::Struct::field_name: Type;
variant module::Enum::Variant(T1, T2);
const module::CONST_NAME: Type = ..;
type module::AliasName = ActualType;
macro module::macro_name!;
```

**Leaf item types** (search targets):

| Category | Leaf items |
|----------|-----------|
| Functions | free fn, inherent method, trait impl method |
| Types (name match) | struct, enum, trait, union |
| Fields | named struct fields (skip positional tuple fields) |
| Variants | enum variants (unit, tuple, struct) |
| Constants | const, static |
| Aliases | type alias |
| Macros | macro_rules! |
| Associated | associated type, associated const |

**Non-leaf** (excluded from results): mod, impl block itself.

---

## Phase 1: Core search infrastructure

Build the item walker and search renderer.

**Changes:**
- `src/model.rs` — Add method to walk all items recursively, yielding
  `(full_path, &Item)` pairs. Must handle:
  - Module tree traversal (all depths)
  - Inherent impl methods → `module::Type::method`
  - Trait impl methods → `module::Type::method` (or `module::<Type as Trait>::method`)
  - Struct fields → `module::Struct::field`
  - Enum variants → `module::Enum::Variant`
  - Associated types/consts
- `src/render.rs` (or new `src/search.rs`) — Search render function that:
  1. Collects all leaf items with full paths
  2. Filters by pattern (case-insensitive, multi-word AND)
  3. Renders each match as one line with kind prefix
  4. Prepends first-line doc comment if present
- `src/lib.rs` — In `run_pipeline` / `run_remote_pipeline`, branch on
  `args.search.is_some()` to use search render instead of module render.

**Design decisions:**
- Visibility filtering still applies (respect `--at-mod`, `same_crate`)
- `--no-*` exclusion flags still apply in search mode
- `--depth` / `--recursive` are ignored in search mode (always full crate)
- Glob expansion is skipped in search mode (search sees the raw model)

**Tests:**
- Unit: path construction for each leaf type (method, field, variant, etc.)
- Integration: `--search` on test_fixture, verify expected items appear
- Integration: multi-word AND matching
- Integration: case-insensitive matching

---

## Phase 2: Polish & edge cases (if needed)

- Sort results by kind, then alphabetically
- Limit result count (e.g., `--search-limit 50`) to avoid flooding
- Handle re-exports (show canonical path vs re-export path)
- Consider fuzzy matching (edit distance) if substring proves insufficient
