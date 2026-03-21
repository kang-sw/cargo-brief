---
title: "Search: show struct fields and type members"
status: idea
---

# Search: show struct fields and type members

## Summary

Extend `cargo brief search` to index and display public struct fields,
inherent methods, and trait impl methods. Use a collapsed path display
format to keep output compact.

## Design

### Display format

Shared path prefix collapsed with `-::` continuation:

```
foo::bar::StructName::field0 : i32
                   -::field1 : f32
                   -::field3 : i32
```

Same format applies to methods when expanded.

### Visibility rules

- **Default (no flag)**: Fields/methods NOT shown in search results.
- **Exact match**: A field/method appears in results when a search token
  matches the field/method name exactly, even without the expand flag.
  Parent struct/type appears as a context header.
- **Expand flag** (e.g., `--members`): Shows all public fields, inherent
  methods, and trait impl methods for matched types, using collapsed display.

### Exact match semantics

A normal search token matching a field name is sufficient — no need for the
`=` operator. The field is already scoped under its parent type, so false
positives are unlikely.

### Interaction with existing features

- `--methods-of <TYPE>`: Already shows methods/fields of a specific type.
  The new feature complements it by surfacing fields in general search results.
  `--methods-of` remains the focused "show everything about this type" tool.
- `--limit`: Applies after member expansion (each field/method counts as a result line).

## Open questions

- Flag naming: `--members`, `--expand`, `--fields`? `--members` covers both
  fields and methods.
- Should the collapsed display also apply to enum variants?
