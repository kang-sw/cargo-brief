---
title: "Add examples subcommand — grep crate examples from local registry"
status: todo
---

## Summary

Add `cargo brief examples <target> [pattern]` subcommand that reads example
source files from the local cargo registry and optionally greps them.

Depends on: `260319-refactor-subcommand-cli.md` (subcommand structure).

## Behavior

### List mode (no pattern)
```
cargo brief examples hecs
cargo brief examples --crates tokio@1
```
Output: list of example files with first doc comment or `fn main` signature.

### Grep mode (with pattern)
```
cargo brief examples hecs spawn_at
cargo brief examples --crates tokio@1 spawn --context 3:3
```
Output: matching snippets with line numbers and context.

### Output Format
```
// examples for hecs[0.10.5]

@examples/ffa.rs
  12:     let mut world = World::new();
  13:     let e = world.reserve_entity();
  14:     let e = world.spawn_at(e, (Name("abc"), 42));
  15:     assert!(world.contains(e));

@examples/another.rs
  44:     world.spawn_at(entity, components);
```

- `@relative/path` header per match group
- Each line prefixed with line number (`cat -n` style)
- Context lines controlled by `--context N` (default: 2) or `--context B:A`
- Smart-case matching (from refactor ticket)

### `--include-tests`
Extends grep scope to `tests/` directory in addition to `examples/`.

## Implementation

1. Resolve crate source path:
   - `cargo metadata` on the (remote or local) workspace
   - Find target crate's `manifest_path` from the dependency list
   - Parent directory = crate source root
2. Scan `examples/` (and optionally `tests/`) for `.rs` files
3. List mode: enumerate files with brief summary
4. Grep mode: line-by-line search with context window, smart-case
5. Format output with `@path` headers and line-numbered content

## Edge Cases

- Crate has no `examples/` directory → clear message
- Crate author used `include = [...]` excluding examples → same message
- Binary examples (non-.rs) → skip
- Very large example files → no hard limit, but `--context` keeps output bounded
- Nested `examples/` subdirectories → recurse

## Acceptance Criteria

- `cargo brief examples <local-workspace-crate>` lists example files
- `cargo brief examples --crates serde@1 Serialize` greps examples
- Output includes line numbers on every line
- `--context 0:0` shows only matching lines
- `--include-tests` adds tests/ to scope
- No examples → informative message, not an error
