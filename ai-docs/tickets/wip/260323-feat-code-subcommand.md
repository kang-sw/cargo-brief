---
title: "code subcommand: pre-crafted tree-sitter code lookup with recursive dep search"
status: wip
started: 2026-03-23
---

## Summary

New `cargo brief code` subcommand that uses pre-crafted tree-sitter queries to look up
code elements (functions, structs, fields, etc.) by kind and name across a crate and its
recursively accessible dependencies. Fills the gap between `search` (API-level, no source
locations) and `ts` (raw S-expressions agents struggle to write).

## Motivation

- AI agents frequently struggle writing correct tree-sitter S-expression queries for Rust
- `search` gives API shape but not source file:line locations
- `ts` requires manual query authoring — error-prone for agents
- Need to search across facade crate dep trees (bevy) with pre-crafted, reliable queries

## CLI Design

```
cargo brief code <TARGET> [KIND] <NAME> [OPTIONS]

Positional:
  TARGET          Crate to search (same resolution as other subcommands)
  KIND            Optional: fn, struct, enum, trait, field, type, impl,
                  macro, const, use. Omit for catch-all across all kinds.
  NAME            Item name to search for

Options:
  --in <TYPE>     Scope to items inside a specific type/impl block
  --refs          Also show grep-based references after definitions
  --refs-only     Only show references (grep), skip definitions
  --src-only      Only search src/, skip examples/tests/benches
  --all-deps      Use cargo metadata direct deps instead of accessible-path
                  (no nightly needed, wider/noisier scope)
  --no-deps       Don't search dependencies at all
  --limit [OFF:]N Pagination
  --quiet / -q    Location-only output (@file:line)
  -C              Remote crate support (existing pattern)
```

## Output Format

```
@/path/to/bevy_ecs/src/system/commands.rs:314
  in bevy_ecs::system::commands, impl Commands<'w, 's>

pub fn spawn(&mut self, bundle: impl Bundle) -> EntityCommands<'_> {
    ...
}

@/path/to/bevy_ecs/src/world/mod.rs:891
  in bevy_ecs::world, impl World

pub fn spawn(&mut self, bundle: impl Bundle) -> EntityWorldMut<'_> {
    ...
}
```

Context line format: `in <crate>::<module_path>[, in <parent_type>]`

## Architecture

### Pipeline: hybrid (rustdoc JSON for dep scope, tree-sitter for code search)

```
1. build PipelineContext (cargo metadata + rustdoc JSON for root)
2. pre_warm_cross_crate_json() → recursive BFS for accessible deps (cached)
3. collect full accessible dep set (crate names) via collect_external_crate_names()
4. map crate names → source dirs via cargo metadata manifest_path
5. per-file: grep pre-filter (literal name match) → tree-sitter parse → format output
```

With `--all-deps`: skip steps 2-3, use cargo metadata direct deps instead.
With `--no-deps`: skip steps 2-4, search only target crate source.

### Module context derivation

- `cargo metadata` `targets[].src_path` gives exact crate entry point (lib.rs/main.rs)
- `src_path.parent()` = source root
- File path relative to source root, `.rs` stripped, `/` → `::` = module path
- `mod.rs` → strip filename, use parent dir name
- Handles non-standard source roots (crates not using `src/`)

### New files

- `src/code.rs` — pre-crafted tree-sitter queries per kind, name matching,
  grep-based refs, output formatting
- Depends on: `cli` (CodeArgs), `examples` (collect_rs_files), `resolve` (cargo metadata)
- Also depends on: `cross_crate` (collect_external_crate_names), `rustdoc_json`
  (for accessible-path dep resolution)
- Does NOT depend on: `model`, `render`

### Supported item kinds and tree-sitter nodes

| Kind keyword | Tree-sitter node(s) | Notes |
|---|---|---|
| `fn` | `function_item`, `function_signature_item` in impl/trait | Methods + free fns |
| `struct` | `struct_item` | |
| `enum` | `enum_item` | |
| `trait` | `trait_item` | |
| `field` | `field_declaration` | Shows owning struct/enum context |
| `type` | `type_item` | Type aliases |
| `impl` | `impl_item` | Match on the implementing type name |
| `macro` | `macro_definition` (macro_rules!) | |
| `const` | `const_item` | |
| `use` | `use_declaration` | Import statements |
| (omitted) | All of the above | Catch-all search |

### Dependency scope (default)

- Full recursive accessible-path set via `pre_warm_cross_crate_json()` BFS
- Reuses existing `collect_external_crate_names()` at each BFS level
- All intermediate rustdoc JSON cached — zero cost on repeated runs
- Validated against Cargo.lock via `LockfilePackages`

### `--refs` implementation (v1)

- Grep-based literal name search across same file scope as definitions
- Reuses `examples::collect_rs_files()` for file collection
- Output format: `@file:line` with context lines, `*` markers on match lines
- Definitions listed first, then references grouped by file
- Extensible to tree-sitter-based semantic refs in future versions

### Performance strategy

- **Grep pre-filter**: before tree-sitter parse, fast byte scan for target name string.
  Only parse files containing the name. Eliminates >95% of files.
- **Parallelism**: rayon for file-level parallelism (parse + query independent per file)
- **Target**: <10s per cached query on bevy-scale crate trees
- Indexing deferred unless grep+parallel proves insufficient

## Implementation Phases

### Phase 1: Core definitions (plan mode)

- `CodeArgs` in cli.rs, `run_code_pipeline()` in lib.rs, dispatch in main.rs
- Pre-crafted tree-sitter queries for all item kinds
- Name matching with smart-case (reuse search conventions)
- Module context derivation from cargo metadata
- Single-crate mode (no dep recursion yet)
- Integration tests against test_fixture

### Phase 2: Dependency recursion (plan mode)

- Accessible-path dep resolution via pre_warm + collect_external_crate_names BFS
- Map crate names to source dirs via cargo metadata
- Multi-crate scanning with grep pre-filter
- `--all-deps` / `--no-deps` flags
- Performance validation on bevy

### Phase 3: References and refinements (plan mode)

- `--refs` grep-based reference search
- `--refs-only` mode
- `--in <TYPE>` scoping
- Catch-all kind (no KIND argument)
- `--quiet` / `--limit` pagination
- Remote crate support (`-C`)

## Integration test criteria

- Single-crate fn/struct/enum/trait lookup against test_fixture
- Cross-crate dep recursion: verify accessible deps are searched, non-accessible skipped
- `--no-deps` / `--all-deps` flag behavior
- Module context accuracy (correct `in crate::module` lines)
- Grep pre-filter correctness (no false negatives)
- `--refs` shows call sites / usage locations
- `--in <TYPE>` scopes to correct parent
- Smart-case matching consistency with search subcommand
- Pagination with `--limit`

### Result (a996e92) - 26-03-23

Phase 1 implemented: single-crate code lookup with all planned item kinds.

- `src/code.rs`: ItemKind enum, pre-crafted tree-sitter queries (paired @name/@item captures),
  smart-case matching, grep pre-filter, module context (file + inline mod), parent context
  (impl/trait/struct/enum ancestor walk), limit/offset pagination, quiet mode.
- `src/cli.rs`: CodeArgs struct with all Phase 1 fields. BriefCommand::Code variant with
  after_help showing examples and item kinds.
- `src/lib.rs`: run_code_pipeline() following run_ts_pipeline pattern for local/remote.
  --all-deps prints warning (Phase 2).
- 20 integration tests: all kinds, catch-all, smart-case, quiet, limit, module context,
  parent context, error cases, src_only.
- Deviation: ItemKind::from_str renamed to ItemKind::parse (clippy should_implement_trait).
- All 365 tests pass, clippy clean for new code.

### Result (6e7e168) - 26-03-24

Phase 2 implemented: multi-crate dependency search with three modes.

- `src/resolve.rs`: `load_dep_package_dirs()` — runs cargo metadata (with deps), returns
  all package dirs + direct dep names. Uses `packages[].id` for robust node matching
  (handles both old and new cargo metadata ID formats).
- `src/lib.rs`: Restructured `run_code_pipeline()` into three phases: (A) resolve target
  via `CodeTarget` struct, (B) collect dep sources via mode-specific helpers, (C) search.
  `discover_accessible_deps()` — standalone BFS replicating `pre_warm_cross_crate_json()`
  with explicit params. `collect_all_deps_sources()` and `collect_accessible_deps_sources()`
  private helpers.
- `src/cli.rs`: Updated `--no-deps` and `--all-deps` help text.
- 8 new integration tests: all_deps (struct, named dep, module context, limit, quiet),
  no_deps exclusion, default accessible-path (direct + transitive).
- Deviation: cargo metadata node ID format changed (`path+file:///path#name@ver` vs
  `path+file:///path#ver`); initial fragment-parsing approach replaced with exact
  `packages[].id` matching after code review caught the bug.
- All 219 integration tests pass (373 total), clippy clean for new code.
