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
cargo brief [-C] api [target] [module_path] [OPTIONS]
cargo brief [-C] search [target] <pattern> [OPTIONS]
cargo brief [-C] examples [target] [pattern] [OPTIONS]
cargo brief [-C] summary [target] [module_path] [OPTIONS]
cargo brief [-C] ts [target] '<query>' [OPTIONS]
cargo brief clean [SPEC]
cargo brief lsp {touch|stop|status|references|blast-radius|call-hierarchy} [OPTIONS]
```

### Subcommands

**`api`** — Extract and render crate API as pseudo-Rust documentation.
Owns: `--depth`, `--recursive`, `--no-expand-glob`, target/module resolution.

**`search`** — Search for items by name across a crate.
Pattern is positional. Owns: `--limit`, `--methods-of`, `--members`.
Smart-case: all-lowercase = case-insensitive, any uppercase = case-sensitive.
Comma-separated = OR groups, space-separated = AND within group.

**`examples`** — Grep example/test/bench source files from a crate.
List mode (no pattern) shows files with `//!` doc comments; grep mode shows matching
lines with context and `*` markers. `--tests [DEPTH]` / `--benches [DEPTH]` extend scope.

**`summary`** — Compact module-level overview with item counts per kind.

**`ts`** — Run tree-sitter structural queries against crate source files.
Query is a required S-expression pattern. Scans `src/`, `examples/`, `tests/`, `benches/` (default) or `--src-only` for `src/` only.
Modes: verbatim (default), `--captures` (capture name + text pairs), `--context N` (surrounding lines).
`--quiet`/`-q` outputs location-only (`@file:line`). `--limit [OFFSET:]N` paginates results.
Capture-less queries auto-augmented with `@_match`. Remote crate support (`-C`) via disk-only pipeline (no rustdoc JSON).

**`code`** — Look up code definitions by kind and name using pre-crafted tree-sitter queries.
Positional: `[KIND] NAME` (omit KIND for catch-all). Item kinds: fn, struct, enum, trait, field, type, impl, macro, const, use.
Dep modes: default (accessible-path BFS via rustdoc JSON), `--no-deps` (target only), `--all-deps` (cargo metadata direct deps, no nightly needed).
Smart-case matching. `--quiet`/`-q` location-only output. `--limit [OFFSET:]N` pagination. `--src-only` restricts to src/.
Remote crate support (`-C`) with dep recursion.

**`clean`** — Clear cached remote crate workspaces. Optional `SPEC` argument for specific crate.

**`lsp`** — Manage persistent rust-analyzer daemon. Subcommands: `touch [--no-wait]`
(ensure running; blocks until indexing completes by default, `--no-wait` for fire-and-forget),
`stop` (graceful shutdown), `status` (show PID/ra state/uptime), `references <symbol> [-q]`
(find all references via ra), `blast-radius <symbol> [--depth N] [-q]` (direct + transitive
callers via BFS), `call-hierarchy <symbol> [--outgoing] [-q]` (incoming/outgoing call tree).
Rejects `-C`.
Daemon per workspace root; idle timeout 10min (override: `CARGO_BRIEF_LSP_TIMEOUT`).

### Target Resolution (api subcommand)
| Syntax              | Resolves to                                     |
|---------------------|-------------------------------------------------|
| `<crate_name>`      | Named crate (exact match or hyphen/underscore)  |
| `self`              | Current package (cwd-based detection)           |
| `self::module`      | Current package, specific module                |
| `crate::module`     | Named crate, specific module (single-arg)       |
| `src/cli.rs`        | File path → auto-converted to module path       |

With `-C`, TARGET is the crate spec (e.g., `serde@1`, `tokio@1::net`).

### Global Flags (on BriefDirect, `global = true`)
| Flag                         | Description                                                    |
|------------------------------|----------------------------------------------------------------|
| `-C` / `--crates`           | Interpret TARGET as a crates.io package spec                   |
| `-F` / `--features <FEATS>` | Comma-separated features to enable (requires -C)               |
| `--no-cache`                 | Skip cache, use temp workspace (requires -C)                   |
| `--toolchain <name>`        | Nightly toolchain name (default: `nightly`)                    |
| `-v` / `--verbose`          | Show progress messages on stderr                               |

### Subcommand Options (all subcommands)
| Flag                                                          | Description              |
|---------------------------------------------------------------|--------------------------|
| `--no-structs` .. `--no-macros` | Exclude item kinds (FilterArgs)                       |
| `--no-docs` / `--doc-lines` / `--compact` / `--verbose-metadata` | Output density       |
| `--all`                 | Show blanket/auto-trait impls                                  |

### Api-only Options
| `--depth <n>`           | Submodule recursion depth (default: 1)                         |
| `--recursive`           | Recurse into all submodules                                    |
| `--no-expand-glob`      | Suppress glob re-export expansion (show pub use lines instead) |
| `--at-package` / `--at-mod` | Visibility resolution overrides                           |
| `--manifest-path`       | Path to Cargo.toml                                             |

### Search-only Options
| `--limit [OFFSET:]N`   | Limit/page search results                                     |
| `--methods-of <TYPE>`  | Show methods/fields of a type                                  |
| `--members`            | Show all members (fields, variants, methods) of matched types  |

---

## Source Layout

```
src/
  lib.rs           — re-exports all modules, pipeline orchestration via PipelineContext → shared api/search functions
  examples.rs      — example/test/bench file scanning, list mode and grep mode rendering
  main.rs          — CLI arg parsing, subcommand dispatch, RemoteOpts extraction from BriefDirect, __lsp-daemon early-exit
  cli.rs           — Subcommand types: ApiArgs, SearchArgs, ExamplesArgs, SummaryArgs, CleanArgs, LspArgs + shared TargetArgs/FilterArgs/GlobalArgs + RemoteOpts (plain struct)
  cross_crate.rs   — cross-crate module following for facade crates
  lsp/             — LSP daemon management (cross-platform): mod.rs (entry), daemon.rs (main loop), client.rs (daemon lifecycle: ensure/spawn/wait), ipc/ (platform-abstracted IPC: unix.rs FIFO + windows.rs atomic-rename), process/ (platform-abstracted process mgmt: unix.rs + windows.rs), protocol.rs (message framing), transport.rs (LSP JSON-RPC framing + background reader thread), watcher.rs (filesystem watching), query.rs (symbol resolution with grep+definition fallback + references)
  remote.rs        — temp workspace creation for --crates (crates.io fetch) + cache management
  resolve.rs       — flexible target resolution (self, crate::module, fallback) + cargo metadata
  rustdoc_json.rs  — JSON generation (with use_cache param) + parsing (bincode-cached)
  model.rs         — CrateModel with module index, visibility resolution
  render.rs        — pseudo-Rust rendering of all item types
  search.rs        — search mode: leaf item walker + one-line-per-item renderer
  ts.rs            — tree-sitter structural query execution + output formatting
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
- Missing nightly toolchain: proactive pre-check via `rustup which`; interactive install prompt when TTY, actionable error otherwise
- Package not found: clear message with original cargo error
- Module not found: lists available modules in the crate
- Leaf item not found: lists available items in the parent module (visibility-filtered)
- `.with_context()` at each pipeline step

---

## Operational State (v0.6.0)

- Core pipeline complete. All item types supported. 191 integration tests.
- Flexible package name resolution: `self`, `crate::module`, file path→module. Bare names always resolve as package. **Smart leaf resolution**: when the final path segment is a leaf item (struct, enum, trait, fn, etc.) instead of a module, resolves the parent module and renders the item with full detail (definition + impls). Module resolution always wins (backward compatible).
- Remote crate support: `-C` boolean flag + TARGET positional as crate spec (e.g., `cargo brief -C api serde@1`). Workspaces cached at `~/.cache/cargo-brief/crates/` with version-normalized directory names (`name[version]`). Exact version resolved via crates.io API with 24h cache; bare specs auto-update. `cargo brief clean [SPEC]` clears cached workspaces.
- **Unified pipeline**: Local and remote entry points produce a `PipelineContext`, then call shared `run_shared_api_pipeline()` / `run_shared_search_pipeline()`. Cross-crate discovery fires automatically for both local and remote crates.
- **Cross-crate accessible paths**: Facade crates (bevy, axum) show items with user-facing paths via `CrossCrateIndex`. `build_cross_crate_index()` walks the facade root top-down, tracking accessible paths through glob/named re-exports. All three pipelines (search, api, summary) use the unified index — items appear as `render::render_resource::AsBindGroup` not `bevy_render::render_resource::bind_group::AsBindGroup`. Dedup keeps shortest non-prelude path per (crate_idx, item_id). Module targeting (`bevy ecs`) still uses the original `resolve_cross_crate_module()`.
- **rustdoc JSON + bincode caching**: Single `generate_rustdoc_json()` with `use_cache` parameter. Primary package: `use_cache` is true for non-workspace-member targets (external deps like `bevy`), false for workspace members. Bincode parse cache always used. `cargo brief clean [SPEC]` manages disk usage. **Batch pre-warming**: `pre_warm_cross_crate_json()` uses `cargo doc` + `RUSTDOCFLAGS` to batch-generate JSON for all cross-crate deps in one invocation (recursive BFS, max depth 8). Names validated against `Cargo.lock` via `load_lockfile_packages()` → `LockfilePackages` (tracks multi-version crates, resolves to `name@latest_version` via `resolve_spec()`). `load_or_find_source_crate` also resolves specs upfront. Auto-retry on "specification is ambiguous" errors picks highest semver version. Existing per-crate calls hit cache after pre-warming.
- Visibility auto-detection: `same_crate` inferred from cwd package context. Cross-crate views use reachability-based filtering via `ReachableInfo` (replaces `HashSet<Id>`). `ReachableInfo` carries `glob_private_modules` and `glob_inlined` metadata — private modules reached via `pub use private::*` are skipped in render/summary and their items inlined at the parent level. Search paths flattened for glob-private modules.
- Re-export expansion: Phase 1 (individual `pub use` lines) + Phase 2 (full definition inlining, on by default; `--no-expand-glob` reverts to Phase 1 only). **Glob expansion**: cross-crate glob chains followed up to depth 8 with cycle prevention. Underscore/hyphen package name fallback. **Named expansion**: cross-crate named re-exports (`pub use serde_core::Serialize;`) also expanded inline — source models shared with glob pass via `GlobExpansionResult.source_models`. Module re-exports preserved as-is. **Intra-crate globs**: handled at render level via `ReachableInfo.glob_inlined`; private module contents inlined directly, no string-based post-processing needed.
- Search mode: `cargo brief search <pattern>` finds leaf items with smart-case matching (all-lowercase = insensitive, any uppercase = sensitive). Comma-separated = OR groups, space-separated = AND within group. Pattern DSL operators: glob wildcards (`*`/`?`, full-path anchored), exclusion (`-term`, global post-filter), exact name match (`=term`, final `::` segment). Operators are embedded in tokens — no new CLI flags. **Member filtering**: by default, member items (fields, variants, impl methods, assoc types/consts) are suppressed unless a search token exactly matches the member's name. `--members` flag expands all members of matched types. Collapsed display: consecutive items sharing a parent path render with `-::member` continuation lines. Cross-crate search also walks struct fields and enum variants.
- `--methods-of <TYPE>`: exact parent-type matching (shows only methods/fields of the named type, not substring matches). Bypasses member suppression. Zero-result sub-crate headers suppressed in normal mode.
- Crate-level docs: root module `//!` comments rendered after `// crate <name>` header. `--no-crate-docs` suppresses independently.
- Trait impl collapsing: simple trait impls (no assoc items) collapsed into per-type summary comments. `--all` expands.
- Output density: `--no-docs`, `--doc-lines N`, `--compact` for token-budget control.
- Attribute rendering: `#[deprecated]`, `#[non_exhaustive]` by default; `--verbose-metadata` adds `#[repr]`, `#[must_use]`, etc.
- Re-export kind annotations: `pub use` lines show `// struct`, `// trait`, etc.
- **Examples subcommand**: `cargo brief examples <target> [pattern]` greps example/test/bench source files. List mode (no pattern) shows files with `//!` docs; grep mode shows matches with `*` markers, dynamic line numbers, context control. `--tests [DEPTH]` / `--benches [DEPTH]` extend scope. Smart-case matching.
- **Tree-sitter subcommand**: `cargo brief ts <target> '<query>'` runs S-expression structural queries against `.rs` source files. Supports verbatim output, `--captures` mode (capture name + text pairs), `--context N` (surrounding lines with `*` markers). Capture-less queries auto-augmented with `@_match`. Scans `src/`, `examples/`, `tests/`, `benches/`. Remote crate support (`-C`) not yet implemented.
- **Code subcommand**: `cargo brief code <target> [kind] <name>` looks up code definitions by kind and name using pre-crafted tree-sitter queries. Three dep modes: default (accessible-path BFS via rustdoc JSON, recursive), `--no-deps` (target crate only), `--all-deps` (cargo metadata direct deps, no nightly needed). Smart-case matching. `--quiet`/`-q` for location-only, `--limit` pagination, `--src-only`. Remote crate support (`-C`) with dep recursion. `discover_accessible_deps()` is a standalone BFS separate from `pre_warm_cross_crate_json()`. `load_dep_package_dirs()` maps crate names to source dirs.
- Dependencies: `clap` 4, `rustdoc-types` 0.57, `serde_json` 1, `anyhow` 1, `tempfile` 3, `bincode` 1, `semver` 1, `ureq` 2, `tree-sitter` 0.25, `tree-sitter-rust` 0.23, `streaming-iterator` 0.1, `libc` 0.2 (unix-only), `notify` 6.
- Test fixture (`test_fixture/`) covers all supported item types. Workspace with `glob-source`/`glob-inner` sub-crates for cross-crate glob chain testing and `named-source` sub-crate for named re-export expansion testing.

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
| `lsp-daemon.md` | LSP daemon: re-exec contract, FIFO IPC, flock serialization, idle timeout |

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

No active items.

## Backlog

No active items.
