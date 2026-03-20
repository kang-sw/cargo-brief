---
title: "Add examples subcommand — grep crate examples from local registry"
status: done
started: 2026-03-20
completed: 2026-03-20
---

## Summary

Add `cargo brief examples <target> [pattern]` subcommand that reads example
source files from the local cargo registry and optionally greps them.

Depends on: `260319-refactor-subcommand-cli.md` (subcommand structure) — done.

## Behavior

### List mode (no pattern)
```
cargo brief examples hecs
cargo brief examples --crates tokio@1
```
Output: list of example files with module-level doc comments (`//!`).
Doc comment lines are stripped of `//!` prefix and indented 4 spaces.
Files with no module doc show `(no module doc)`.

### Grep mode (with pattern)
```
cargo brief examples hecs spawn_at
cargo brief examples --crates tokio@1 spawn --context 3:3
```
Output: matching snippets with line numbers and context.

### Output Format

#### List mode
```
// examples for tokio[1.44.2]
// root: /Users/user/.cargo/registry/src/index.crates.io-.../tokio-1.44.2/

@examples/echo.rs
    TCP echo server — demonstrates async read/write loop
    with multiple concurrent connections.

@examples/chat.rs
    Multi-client chat using broadcast channels

@examples/proxy.rs
    (no module doc)
```

#### Grep mode
```
// examples for hecs[0.10.5]
// root: /Users/user/.cargo/registry/src/index.crates.io-.../hecs-0.10.5/

@examples/ffa.rs
 12:  let mut world = World::new();
 13:  let e = world.reserve_entity();
*14:  let e = world.spawn_at(e, (Name("abc"), 42));
 15:  assert!(world.contains(e));
  ...
*44:  world.spawn_at(entity, components);
 45:  println!("done");
 46:

@examples/another.rs
*22:  bar.set(world.spawn_at());
 23:
*24:  let e = world.spawn_at().clone();
 25:
 26:
```

#### Formatting rules

- `@relative/path` header per file (one header per file, not per match group)
- Line number column: dynamically sized based on largest line number in the file.
  Width = `max(digit_count(max_line_no), 4) + 3` (star/space, digits, colon, 2 spaces).
  Colon always at same column; code starts 2 spaces after colon.
- `*` prefix on matched lines (replaces leading space), context lines have space prefix
- Adjacent match groups (overlapping context ranges) merged into single block
- Non-adjacent groups within same file separated by `  ...`
- Context lines controlled by `--context N` (default: 2) or `--context B:A`
- Smart-case matching (all-lowercase = case-insensitive, any uppercase = case-sensitive)
- Only files with matches are shown in grep mode

### Scope flags

- Default: `examples/` directory only
- `--tests [DEPTH]`: include `tests/` directory. Optional depth limits recursion
  (no value = unlimited, `--tests 1` = top-level only). clap: `num_args(0..=1)`,
  `default_missing_value("999")`.
- `--benches [DEPTH]`: same pattern for `benches/` directory.

## Implementation

1. Resolve crate source path:
   - `cargo metadata` on the (remote or local) workspace
   - Find target crate's `manifest_path` from the dependency list
   - Parent directory = crate source root
2. Scan `examples/` (and optionally `tests/`, `benches/`) for `.rs` files
3. List mode: enumerate files, extract `//!` doc comments, format with 4-space indent
4. Grep mode:
   a. Collect all match line numbers per file
   b. Sort and compute context ranges (match_line ± context_size)
   c. Merge overlapping ranges
   d. Output with `*`-prefixed match lines, space-prefixed context, `...` separators
5. Format output with `@path` headers and dynamically-aligned line numbers

## Edge Cases

- Crate has no `examples/` directory → clear message
- Crate author used `include = [...]` excluding examples → same message
- Binary examples (non-.rs) → skip
- Very large example files → no hard limit, but `--context` keeps output bounded
- Nested `examples/` subdirectories → recurse (respects depth for tests/benches)

## Acceptance Criteria

- `cargo brief examples <local-workspace-crate>` lists example files with module docs
- `cargo brief examples --crates serde@1 Serialize` greps examples
- Line numbers on every line, `*` on matched lines, colon-aligned
- Adjacent matches merged, non-adjacent separated by `...`
- `--context 0:0` shows only matching lines
- `--tests` adds tests/ to scope, `--benches` adds benches/
- `--tests 1` limits to top-level test files only
- No examples → informative message, not an error
- Header shows crate name/version and root source path

### Result - 26-03-20

Implemented as planned. Key files:

- `src/examples.rs` (~215 lines): `render_examples()`, `render_list()`, `render_grep()`,
  `collect_all_files()`, `collect_rs_files()`, `parse_context()`. Pure file I/O module
  with no dependencies on model/render/rustdoc_json.
- `src/lib.rs`: `run_examples_pipeline(&ExamplesArgs)` — local path uses
  `package_manifest_dirs` for resolution, remote path uses `find_dep_source_root()`.
- `src/cli.rs`: `ExamplesArgs` with `--tests`/`--benches` as `Option<u32>`
  (`num_args(0..=1)`, `default_missing_value = "999"`).
- `src/resolve.rs`: Added `package_manifest_dirs: HashMap<String, PathBuf>` to
  `CargoMetadataInfo`, `find_dep_source_root()` for remote crate source lookup.
- 5 new integration tests, 4 unit tests in examples.rs.

Deviations from plan: None significant. Positional arg handling (crate_name → pattern
when `--crates` used) mirrors search subcommand pattern exactly.
