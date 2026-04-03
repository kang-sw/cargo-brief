---
title: Output Format
summary: >
  Pseudo-Rust rendering rules for the api, search, and summary subcommands.
  Covers item-type rendering, doc handling, density controls, attribute
  rendering, re-export representation, trait impl collapsing, and search
  output format.

features:
  - Crate Header and Crate-Level Docs
  - Pseudo-Rust Philosophy
  - Module Structure and Depth Control
    - Glob-Private Module Inlining
  - Item-Type Rendering
    - Structs
    - Enums
    - Traits
    - Functions
    - Type Aliases
    - Constants
    - Statics
    - Unions
    - Macros
  - Inherent Impl Blocks
  - Trait Impl Collapsing
  - Re-Export Rendering
    - Named Re-Exports
    - Glob Re-Exports
    - Cross-Crate Named Re-Exports
  - Doc Comment Handling
    - Density Flags
  - Attribute Rendering
    - Default Attributes (always shown)
    - Verbose Attributes (`--verbose-metadata`)
  - Item-Kind Filtering
  - Search Output Format
    - One-Line Item Format
    - Collapsed Member Display
    - Search-Mode Doc Comments
    - Search-Mode Attribute Markers
    - Sorting
    - Pagination
    - Member Suppression
  - Summary Output Format
---

# Output Format

cargo-brief renders crate APIs as pseudo-Rust text designed for AI agent
consumption. The output is valid enough for syntax highlighters but is not
compilable Rust -- function bodies are replaced with `;`, hidden struct fields
with `..`, and impl blocks are condensed.


## Crate Header and Crate-Level Docs

Every output begins with a crate header comment:

```
// crate <crate_name>
```

If the crate root module has `//!` doc comments, they are rendered immediately
after the header using inner-doc-comment syntax:

```
// crate serde
//! Serde is a framework for serializing and deserializing Rust data structures.
//! ...
```

Crate-level docs are suppressed by any of: `--no-docs`, `--compact`,
`--no-crate-docs`, or `--doc-lines 0`. The `--no-crate-docs` flag suppresses
only crate-level docs while preserving item-level doc comments. `--doc-lines N`
limits crate-level docs to the first N lines.


## Pseudo-Rust Philosophy

The output mimics Rust syntax closely enough that editors apply correct syntax
highlighting, but it is not machine-parseable Rust:

- Function and method bodies are omitted; signatures end with `;`
- Struct fields that are not visible are replaced with `// .. private fields`
- Collapsed modules beyond the depth limit show `mod name { /* ... */ }`
- Compact-mode impl blocks show `impl Type { .. }`
- Trait impls without associated items collapse to a summary comment
- Visibility qualifiers (`pub`, `pub(crate)`, `pub(in path)`) are preserved
  verbatim
- Generics, where clauses, and lifetimes are reproduced faithfully
- `impl Trait` parameters are re-sugared from synthetic generic params back to
  `param: impl Trait` form


## Module Structure and Depth Control

Modules are rendered as nested `mod name { ... }` blocks. The `--depth N` flag
controls how deep submodule contents are expanded (default: 1). `--recursive`
expands all levels.

At the depth limit, modules are collapsed:

```
mod inner { /* ... */ }
```

Root-level items (depth 0) have no indentation. Each nesting level adds 4-space
indentation.

### Glob-Private Module Inlining

When a private module's contents are re-exported via `pub use private_mod::*`,
the private module is not shown. Instead, its items are inlined at the parent
module level, as if they were defined there directly. This matches how users
actually access these items.


## Item-Type Rendering

### Structs

Three struct kinds are supported:

**Unit structs:**
```
pub struct UnitStruct;
```

**Tuple structs:**
```
pub struct TupleStruct(pub i32, /* private */);
```
Private tuple fields render as `/* private */`.

**Plain (named-field) structs:**
```
pub struct PubStruct {
    pub pub_field: i32,
    // .. private fields
}
```
Fields not visible from the observer position are hidden. If all fields are
hidden, the struct collapses to `pub struct Name { .. }`. With `--compact`, all
plain structs collapse to `{ .. }` regardless of field visibility.

### Enums

Enums list all variants with their payloads:

```
pub enum PlainEnum {
    Alpha,
    Beta,
}

pub enum TupleEnum {
    One(i32),
    Two(String, bool),
}

pub enum StructEnum {
    Point {
        x: f64,
        y: f64,
    },
}
```

With `--compact`, variants are name-only on a single line when the result fits
within 120 columns:
```
pub enum PlainEnum { Alpha, Beta }
pub enum TupleEnum { One(..), Two(..) }
```
If the one-liner exceeds 120 columns, it falls back to one-variant-per-line
with compact names (e.g., `StructVariant { .. }`).

### Traits

Trait definitions list all associated items -- methods, associated types, and
associated constants:

```
pub trait MyTrait {
    fn do_thing(&self) -> bool;
    type Output;
    const LIMIT: usize = 100;
}
```

Supertraits are shown: `pub trait Sub: Base + Clone { ... }`.

With `--compact`, traits collapse to `pub trait MyTrait { .. }`.

### Functions

Functions render as signatures ending with `;`. Qualifiers appear in order:
`const`, `async`, `unsafe`.

```
pub fn free_function(x: i32) -> i32;
pub async fn async_fn() -> Result<(), Error>;
pub const fn const_fn() -> usize;
pub unsafe fn unsafe_fn(ptr: *const u8);
```

`impl Trait` parameters are re-sugared from the rustdoc JSON synthetic generic
representation back to the natural form:
```
pub fn process(input: impl AsRef<str>) -> String;
```

Where clauses are preserved and formatted after the return type:
```
pub fn complex<T>(val: T) -> T
where
    T: Clone + Debug;
```

### Type Aliases

```
pub type MyResult<T> = Result<T, MyError>;
```

### Constants

Constants include their value when available:
```
pub const MY_CONST: i32 = 42;
```

### Statics

Statics include mutability and value:
```
pub static GLOBAL_COUNT: AtomicUsize = _;
pub static mut MUTABLE_STATIC: i32 = 0;
```

### Unions

Unions list their fields with visibility, similar to structs:
```
pub union MyUnion {
    pub int_val: i32,
    pub float_val: f32,
    // ... private fields
}
```

### Macros

Macros render as stub definitions:
```
macro_rules! my_macro { /* ... */ }
```


## Inherent Impl Blocks

Inherent `impl` blocks are rendered after the type definition they belong to.
Visible methods, associated types, and associated constants are listed:

```
impl PubStruct {
    pub fn pub_method(&self) -> i32;
    pub fn new() -> Self;
}
```

With `--compact`, inherent impls collapse to `impl PubStruct { .. }`.

Methods not visible from the observer position are omitted. If no methods are
visible, the entire impl block is suppressed.


## Trait Impl Collapsing

Simple trait impls (those with no associated types or constants) are collapsed
into a per-type summary comment:

```
// PubStruct: Clone, Debug, Display, Eq, Hash, MyTrait, PartialEq
```

Trait names within a summary line are sorted alphabetically.

"Rich" trait impls (those with associated types or constants) are rendered
expanded, but only show the associated items -- methods are replaced with
`// ..`:

```
impl Converter for PubStruct {
    type Output = String;
    // ..
}
```

Negative trait impls (e.g., `impl !Send for Type`) are always rendered
expanded.

Blanket impls and auto-trait (synthetic) impls are hidden by default. The
`--all` flag expands all trait impls, including simple ones that would
otherwise be collapsed into summary comments:

```
impl MyTrait for PubStruct { .. }
```


## Re-Export Rendering

### Named Re-Exports

Non-glob `pub use` statements are rendered with a kind annotation comment:

```
pub use other_module::MyStruct; // struct
pub use other_module::MyTrait; // trait
pub use path::some_fn; // fn
pub use source as Alias; // enum
```

Supported kind annotations: `// struct`, `// enum`, `// trait`, `// fn`,
`// type`, `// const`, `// static`, `// union`, `// macro`.

### Glob Re-Exports

Glob re-exports (`pub use source::*`) are handled in two phases:

**Phase 1** (with `--no-expand-glob`): The `pub use` line is rendered as-is:
```
pub use other_crate::*;
```

**Phase 2** (default): Glob re-exports are expanded inline. The contents of
the source module are rendered as if they were defined in the importing module.
For intra-crate globs targeting private modules, the private module disappears
and its items appear at the parent level. For cross-crate globs, source crate
definitions are inlined with full detail (definition + impl blocks).

### Cross-Crate Named Re-Exports

Named re-exports from external crates (e.g., `pub use serde_core::Serialize`)
are also expanded inline when source models are available, showing the full
definition rather than just the `pub use` line.


## Doc Comment Handling

Item-level doc comments are preserved verbatim from rustdoc JSON using `///`
syntax:

```
/// This is a documented function.
///
/// It does important things.
pub fn documented() -> bool;
```

Multi-line doc comments are rendered line by line. Empty doc lines produce
bare `///` lines.

### Density Flags

| Flag | Effect |
|------|--------|
| `--no-docs` | Suppresses all doc comments (crate-level and item-level) |
| `--doc-lines N` | Limits each doc comment to the first N lines. `--doc-lines 0` suppresses all docs. |
| `--compact` | Suppresses doc comments AND collapses struct fields, enum variant details, trait bodies, and impl blocks |
| `--no-crate-docs` | Suppresses only crate-level `//!` docs; item-level docs are preserved |


## Attribute Rendering

### Default Attributes (always shown)

These attributes are always rendered when present:

```
#[deprecated]
#[deprecated = "use new_fn instead"]
#[deprecated(since = "1.0", note = "use new_fn instead")]
#[non_exhaustive]
```

### Verbose Attributes (`--verbose-metadata`)

These attributes are only shown when `--verbose-metadata` is enabled:

```
#[must_use]
#[must_use = "this returns a Result"]
#[repr(C)]
#[repr(C, align(8))]
#[repr(transparent)]
#[repr(C, packed)]
#[no_mangle]
#[macro_export]
#[export_name = "custom_symbol"]
#[target_feature(enable = "avx2")]
```

`#[repr]` supports combinations: kind (`C`, `transparent`, `simd`, `Rust`),
plus optional `align(N)`, `packed`, `packed(N)`, and integer discriminant type.


## Item-Kind Filtering

A subtractive filtering model: all item kinds are shown by default. Each
`--no-*` flag excludes one kind.

| Flag | Excludes |
|------|----------|
| `--no-structs` | Structs |
| `--no-enums` | Enums |
| `--no-traits` | Traits |
| `--no-functions` | Free functions and methods |
| `--no-aliases` | Type aliases and associated types |
| `--no-constants` | Constants, statics, and associated constants |
| `--no-unions` | Unions |
| `--no-macros` | Macros |

Note: `--no-constants` hides both `const` and `static` items (they are grouped
together). The `--all` flag shows blanket/auto-trait impls that are hidden by
default.


## Search Output Format

The `search` subcommand renders one line per matched item. The header shows the
pattern and result count:

```
// crate test_fixture — search: "PubStruct" (3 results)
```

### One-Line Item Format

Each result is a single line with a kind prefix followed by the full path:

```
fn outer::free_function(x: i32) -> i32;
fn outer::PubStruct::pub_method(&self) -> i32;
struct outer::PubStruct { pub_field: i32, .. };
enum outer::PlainEnum;
trait outer::MyTrait;
union outer::MyUnion;
field outer::PubStruct::pub_field: i32;
variant outer::PlainEnum::Alpha;
variant outer::TupleEnum::One(i32);
variant outer::StructEnum::Point { x: f64, y: f64 };
const outer::MY_CONST: i32 = 42;
static outer::GLOBAL_COUNT: AtomicUsize;
type outer::MyAlias = i32;
macro outer::my_macro!;
pub use source::Name; // struct
```

Functions include their full signature. Structs in search mode show visible
fields inline and impl summary (method count) as a trailing comment. Enum
variants show their payload. Constants show their value.

### Collapsed Member Display

When consecutive items share a parent path, members render with a `-::member`
continuation line to reduce visual noise:

```
struct outer::PubStruct { pub_field: i32, .. };
      -::pub_method(&self) -> i32;
      -::new() -> Self;
```

The continuation line uses padding to align with the parent path, and shows
only the member name with its signature.

### Search-Mode Doc Comments

In search mode, the first line of an item's doc comment is shown as a `///`
line above the result (unless `--no-docs` or `--compact` is active).

### Search-Mode Attribute Markers

Deprecated and non-exhaustive items show inline markers:
```
[deprecated] fn outer::old_function();
[non_exhaustive] enum outer::OpenEnum;
```

### Sorting

Results are sorted by kind (functions first, then structs, enums, traits,
unions, fields, variants, constants, statics, type aliases, macros, associated
types, associated constants, re-exports) and alphabetically within each kind.

With `--members`, sorting is path-based instead, grouping members with their
parent type.

### Pagination

`--limit N` limits output to N results. `--limit M:N` skips M results and
shows N. Skipped results are indicated:

```
// (skipped 10 results)
... results ...
// ... and 5 more results
```

### Member Suppression

By default, member items (fields, variants, impl methods, associated types/
constants) are suppressed unless a search token exactly matches the member's
name. This keeps search results focused on top-level items.

`--members` expands all members of matched types. `--methods-of <TYPE>` shows
only members of the named type.


## Summary Output Format

The `summary` subcommand produces a compact module-level overview with item
counts per kind:

```
// crate my_crate
mod config;     // 2 structs, 1 enums, 3 fns
mod net;        // 1 traits, 4 structs, 5 fns
mod util;       // 2 fns, 1 consts
// root: 1 structs, 2 fns
```

Module lines are column-aligned for readability. The `// root:` line shows
items defined directly in the scoped module (or crate root).

Item kinds appear in a fixed order: traits, structs, enums, fns, types,
consts, macros, unions. Only non-zero counts are shown.

Empty modules (those with zero visible items) are omitted from the output.

For cross-crate summaries (facade crates), sub-crate module paths are prefixed
with the accessible path segment.
