---
title: "Annotate re-exports with item kind"
status: done
completed: 2026-03-16
---

# Feature: Annotate re-exports with item kind

## Priority: P1

## Motivation

Re-export lines like `pub use util::AsyncReadExt;` give zero indication of what
the item **is** (trait? struct? function? type alias?). For LLM consumers, this
is critical information for deciding whether to drill deeper.

**Examples of the problem (tokio --compact):**
```rust
pub use self::async_buf_read::AsyncBufRead;   // what is this?
pub use self::read_buf::ReadBuf;               // struct? type? trait?
pub use util::AsyncReadExt;                    // trait? struct?
pub use split::ReadHalf;                       // struct? type?
```

## Design

Append a `// kind` comment to re-export lines when the target item is resolved
in the crate index:

```rust
pub use self::async_buf_read::AsyncBufRead; // trait
pub use self::read_buf::ReadBuf; // struct
pub use util::AsyncReadExt; // trait
pub use split::ReadHalf; // struct
pub use split::split; // fn
```

### Rules

- Only annotate when target ID resolves in `model.krate.index`
- For cross-crate re-exports (target not in index), omit annotation
- Kind labels: `struct`, `enum`, `trait`, `fn`, `type`, `const`, `static`,
  `union`, `macro`, `mod`
- Search mode: no change (search results already have kind prefixes)
- Format: `// <kind>` appended to the `pub use` line, single space before `//`

### Implementation

In `render_use()` (`src/render.rs`), look up the target item and append kind:

```rust
fn render_use(item, use_item, target_item, indent, output) {
    // ... existing alias logic ...
    let kind_suffix = match &target_item.inner {
        ItemEnum::Struct(_) => " // struct",
        ItemEnum::Enum(_) => " // enum",
        ItemEnum::Trait(_) => " // trait",
        ItemEnum::Function(_) => " // fn",
        ItemEnum::TypeAlias(_) => " // type",
        // ... etc
        _ => "",
    };
    output.push_str(&format!("{indent}{vis}use {source}{alias}{kind_suffix};\n"));
}
```

Also annotate glob expansion lines in `apply_glob_expansions()`.

## Files to Modify

| File | Changes |
|------|---------|
| `src/render.rs` → `render_use()` | Append kind suffix |
| `src/lib.rs` → `apply_glob_expansions()` | Annotate Phase 1 `pub use` lines |
| `tests/integration.rs` | Test re-export kind annotations |

### Result (509818d)

Implemented `render_use()` kind annotation + `item_kind_suffix()` helper.
9 kind labels: struct, enum, trait, fn, type, const, static, union, macro.
Deviation from plan: glob expansion Phase 1 lines (`apply_glob_expansions`)
deliberately skipped — those are synthetic `pub use` lines without target item
resolution. Existing `test_reexport` updated to check `// struct` suffix.
