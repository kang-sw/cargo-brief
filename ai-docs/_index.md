# cargo-brief — Project State & Architecture

## What This Tool Is

`cargo-brief` is a Cargo subcommand that extracts and formats a Rust crate's API surface as
pseudo-Rust documentation. It is a visibility-aware, context-sensitive extension of
`cargo-public-api`.

Primary consumer: **AI coding agents** (e.g., Claude Code) that need to understand a crate's
interface without reading full source files — saving context window tokens.

Output style: "text-document-like" pseudo-Rust (not machine-readable JSON). Function bodies
are replaced with `;`, doc comments are preserved verbatim, private items are filtered by
perspective.

---

## Core Concept: Visibility-Aware Perspective

The tool's key differentiator from `cargo-public-api` is **`--at-mod`**: rather than dumping
everything that is technically `pub`, it shows only what would compile if `use`d from the
specified module. This includes re-exports and respects `pub(crate)`, `pub(super)`,
`pub(in path)` appropriately.

For **external crates**, `--at-mod` degenerates to "show `pub` items only" (since cross-crate
visibility is always `pub`-only). This makes external dep support architecturally simpler.

---

## CLI Interface

```
cargo brief api [target] [module_path] [OPTIONS]
cargo brief search [target] <pattern> [OPTIONS]
cargo brief examples [target] [pattern] [OPTIONS]
```

### Subcommands

**`api`** — Extract and render crate API as pseudo-Rust documentation.
Owns: `--depth`, `--recursive`, `--expand-glob`, target/module resolution.

**`search`** — Search for items by name across a crate.
Pattern is positional. Owns: `--limit`, `--methods-of`.
Smart-case: all-lowercase = case-insensitive, any uppercase = case-sensitive.
Comma-separated = OR groups, space-separated = AND within group.

**`examples`** — Grep example/test/bench source files from a crate.
List mode (no pattern) shows files with `//!` doc comments; grep mode shows matching
lines with context and `*` markers. `--tests [DEPTH]` / `--benches [DEPTH]` extend scope.

### Target Resolution (api subcommand)
| Syntax              | Resolves to                                     |
|---------------------|-------------------------------------------------|
| `<crate_name>`      | Named crate (exact match or hyphen/underscore)  |
| `self`              | Current package (cwd-based detection)           |
| `self::module`      | Current package, specific module                |
| `crate::module`     | Named crate, specific module (single-arg)       |
| `src/cli.rs`        | File path → auto-converted to module path       |

### Shared Options (all subcommands)
| Flag                    | Description                                                    |
|-------------------------|----------------------------------------------------------------|
| `--crates <spec>`       | Fetch crate from crates.io (e.g., `serde`, `tokio@1`)          |
| `--features <FEATURES>` | Comma-separated features to enable for --crates                |
| `--no-cache`            | Skip cache for `--crates` (use temp workspace)                 |
| `--clean [SPEC]`        | Clear cached remote crate workspaces                           |
| `--toolchain <name>`    | Nightly toolchain name (default: `nightly`)                    |
| `-v` / `--verbose`      | Show progress messages on stderr                               |
| `--no-structs` .. `--no-macros` | Exclude item kinds (FilterArgs)                       |
| `--no-docs` / `--doc-lines` / `--compact` / `--verbose-metadata` | Output density       |
| `--all`                 | Show blanket/auto-trait impls                                  |

### Api-only Options
| `--depth <n>`           | Submodule recursion depth (default: 1)                         |
| `--recursive`           | Recurse into all submodules                                    |
| `--expand-glob`         | Inline full definitions from glob re-export sources            |
| `--at-package` / `--at-mod` | Visibility resolution overrides                           |
| `--manifest-path`       | Path to Cargo.toml                                             |

### Search-only Options
| `--limit [OFFSET:]N`   | Limit/page search results                                     |
| `--methods-of <TYPE>`  | Show methods/fields of a type                                  |

---

## Source Layout

```
src/
  lib.rs           — re-exports all modules, pipeline orchestration via PipelineContext → shared api/search functions
  examples.rs      — example/test/bench file scanning, list mode and grep mode rendering
  main.rs          — CLI arg parsing, subcommand dispatch
  cli.rs           — Subcommand types: ApiArgs, SearchArgs, ExamplesArgs + shared TargetArgs/RemoteArgs/FilterArgs/GlobalArgs
  cross_crate.rs   — cross-crate module following for facade crates
  remote.rs        — temp workspace creation for --crates (crates.io fetch) + cache management
  resolve.rs       — flexible target resolution (self, crate::module, fallback) + cargo metadata
  rustdoc_json.rs  — JSON generation (with use_cache param) + parsing (bincode-cached)
  model.rs         — CrateModel with module index, visibility resolution
  render.rs        — pseudo-Rust rendering of all item types
  search.rs        — search mode: leaf item walker + one-line-per-item renderer
```

### Supported Item Types
Structs (unit, tuple, plain), enums (plain, tuple, struct variants), traits,
free functions (async, const, unsafe), type aliases, constants, statics
(static, static mut), unions, macros (macro_rules!), re-exports (use),
inherent impls, trait impls.

### Backend: rustdoc JSON
`cargo +nightly rustdoc -p <crate> -- --output-format json -Z unstable-options --document-private-items`

Parsed via `rustdoc-types` 0.57. Post-macro-expansion output.

### Visibility Resolution
- `pub` → always visible
- `pub(crate)` → visible if same crate
- `pub(super)` / `pub(in path)` → visible if observer is in scope
- `default` → hidden (except impl items, delegated to parent type)

### Error Handling
- Missing nightly toolchain: actionable install command
- Package not found: clear message with original cargo error
- Module not found: lists available modules in the crate
- `.with_context()` at each pipeline step

---

## Operational State (v0.5.1)

- Core pipeline complete. All item types supported. 156 integration tests.
- Flexible package name resolution: `self`, `crate::module`, file path→module. Bare names always resolve as package.
- Remote crate support: `--crates <spec>` fetches any crate from crates.io. Workspaces cached at `~/.cache/cargo-brief/crates/` with version-normalized directory names (`name[version]`). Exact version resolved via crates.io API with 24h cache; bare specs auto-update.
- **Unified pipeline**: Local and remote entry points produce a `PipelineContext`, then call shared `run_shared_api_pipeline()` / `run_shared_search_pipeline()`. Cross-crate discovery fires automatically for both local and remote crates.
- **Cross-crate accessible paths**: Facade crates (bevy, axum) show items with user-facing paths via `CrossCrateIndex`. `build_cross_crate_index()` walks the facade root top-down, tracking accessible paths through glob/named re-exports. All three pipelines (search, api, summary) use the unified index — items appear as `render::render_resource::AsBindGroup` not `bevy_render::render_resource::bind_group::AsBindGroup`. Dedup keeps shortest non-prelude path per (crate_idx, item_id). Module targeting (`bevy ecs`) still uses the original `resolve_cross_crate_module()`.
- **rustdoc JSON + bincode caching**: Single `generate_rustdoc_json()` with `use_cache` parameter. Workspace members always regenerate; non-members skip if JSON exists. Bincode parse cache always used. `--clean [SPEC]` manages disk usage. **Batch pre-warming**: `pre_warm_cross_crate_json()` uses `cargo doc` + `RUSTDOCFLAGS` to batch-generate JSON for all cross-crate deps in one invocation (recursive BFS, max depth 8). Names validated against `Cargo.lock` via `load_lockfile_packages()` → `LockfilePackages` (tracks multi-version crates, resolves to `name@latest_version` via `resolve_spec()`). Auto-retry on "specification is ambiguous" errors picks highest semver version. Existing per-crate calls hit cache after pre-warming.
- Visibility auto-detection: `same_crate` inferred from cwd package context. Cross-crate views use reachability-based filtering via `ReachableInfo` (replaces `HashSet<Id>`). `ReachableInfo` carries `glob_private_modules` and `glob_inlined` metadata — private modules reached via `pub use private::*` are skipped in render/summary and their items inlined at the parent level. Search paths flattened for glob-private modules.
- Glob re-export expansion: Phase 1 (individual `pub use` lines) + Phase 2 (`--expand-glob` inlines full definitions). **Recursive**: cross-crate glob chains followed up to depth 8 with cycle prevention. Underscore/hyphen package name fallback. **Intra-crate globs**: handled at render level via `ReachableInfo.glob_inlined`; private module contents inlined directly, no string-based post-processing needed.
- Search mode: `cargo brief search <pattern>` finds leaf items with smart-case matching (all-lowercase = insensitive, any uppercase = sensitive). Comma-separated = OR groups, space-separated = AND within group. Pattern DSL operators: glob wildcards (`*`/`?`, full-path anchored), exclusion (`-term`, global post-filter), exact name match (`=term`, final `::` segment). Operators are embedded in tokens — no new CLI flags.
- `--methods-of <TYPE>`: exact parent-type matching (shows only methods/fields of the named type, not substring matches). Zero-result sub-crate headers suppressed in normal mode.
- Crate-level docs: root module `//!` comments rendered after `// crate <name>` header. `--no-crate-docs` suppresses independently.
- Trait impl collapsing: simple trait impls (no assoc items) collapsed into per-type summary comments. `--all` expands.
- Output density: `--no-docs`, `--doc-lines N`, `--compact` for token-budget control.
- Attribute rendering: `#[deprecated]`, `#[non_exhaustive]` by default; `--verbose-metadata` adds `#[repr]`, `#[must_use]`, etc.
- Re-export kind annotations: `pub use` lines show `// struct`, `// trait`, etc.
- **Examples subcommand**: `cargo brief examples <target> [pattern]` greps example/test/bench source files. List mode (no pattern) shows files with `//!` docs; grep mode shows matches with `*` markers, dynamic line numbers, context control. `--tests [DEPTH]` / `--benches [DEPTH]` extend scope. Smart-case matching.
- Dependencies: `clap` 4, `rustdoc-types` 0.57, `serde_json` 1, `anyhow` 1, `tempfile` 3, `bincode` 1, `semver` 1, `ureq` 2.
- Test fixture (`test_fixture/`) covers all supported item types. Now a workspace with `glob-source`/`glob-inner` sub-crates for cross-crate glob chain testing.

## Mental Model Documents

Domain-oriented operational knowledge in `ai-docs/mental-model/`:

| Document | Domain |
|----------|--------|
| `overview.md` | Pipeline paths (local/remote), module graph, shared coupling patterns |
| `visibility.md` | Visibility resolution: `is_visible_from`, `same_crate` inference, observer normalization |
| `target-resolution.md` | CLI → package/module resolution: 4-case algorithm, dual invocation, `--crates` bypass |
| `remote-pipeline.md` | `--crates` lifecycle: TempDir borrow chain, version semantics, remote-only constraints |
| `glob-expansion.md` | Glob re-export expansion: string-based detection, Phase 1/2 inlining, coupling with render |
| `testing.md` | Test infrastructure: BriefArgs coupling, fixture contracts, visibility test patterns |

---

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary backend | rustdoc JSON + `rustdoc-types` | Post-macro-expansion, official, matches LSP-level output |
| `--at-mod` semantics | "compiles when `use`d from here" | Matches developer mental model; includes re-exports |
| Output format | Pseudo-Rust text (not JSON) | LLM consumption; readable as documentation |
| Item-kind filtering | Default=show all common; `--no-*` to exclude; `--all` adds blanket/auto-trait impls | Subtractive model is more ergonomic |
| Statics grouped with constants | `--no-constants` hides both | Conceptually similar; avoids flag proliferation |
| lib.rs + slim main.rs | `run_api_pipeline()` / `run_search_pipeline()` returns String | Enables integration tests without subprocess |
| External deps | Phase 2 | Adds ~30% complexity; architecture supports it cleanly |
| Target resolution | Semantic layer between CLI and pipeline | `BriefArgs` stays unchanged; resolution in `src/resolve.rs` |
| Single cargo metadata call | `resolve::load_cargo_metadata()` | Eliminates redundant `find_target_dir()` call |
| Ambiguous single arg | Always package | Bare name = package; `self::mod` for own modules |
| File path as module | Heuristic: `/` or `.rs` → file path | 2-level fallback: cwd-relative, then package `src/`-relative |

---

## In Progress

1. **`tickets/wip/260321-feat-canonical-reexport-paths.md`** — Phase 1 done (intra-crate). Phase 2 (cross-crate) pending.

## Next Up (priority order)

1. **`tickets/idea/260316-feat-output-summary-mode.md`** — P2: `--summary` TOC mode
   (Pipeline unification completed — `tickets/done/260320-refactor-unify-local-remote-pipeline.md`)

## Backlog

- `tickets/idea/260316-feat-search-kind-filter.md` — P2: `--search-kind` filter
