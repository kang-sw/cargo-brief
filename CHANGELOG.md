# Changelog

All notable changes to this project will be documented in this file.

## [0.5.3] - 2026-03-21

### Fixed

- **Multi-version crate disambiguation**: Workspaces with multiple versions of the same crate (e.g., bevy pulling in `hashbrown` 0.15+0.16, `foldhash` 0.1+0.2) no longer fail with "specification is ambiguous" during batch pre-warming or cross-crate index building. `LockfilePackages` tracks all versions from `Cargo.lock` and resolves to `name@latest_version` when multiple exist.
- **Cross-crate index version resolution**: `load_or_find_source_crate` now resolves version-qualified specs upfront via `LockfilePackages::resolve_spec()`, preventing ambiguity errors for crates discovered during deep module walks (not found by pre-warming BFS).
- **Non-workspace dependency caching**: Local context builders (`cargo brief search bevy` from a game project) now cache rustdoc JSON for non-workspace-member targets instead of re-running `cargo rustdoc` on every invocation. Only workspace members (whose source may change) are always regenerated.

## [0.5.2] - 2026-03-21

### Added

- **`summary` subcommand**: `cargo brief summary` shows a compact module-level overview with item counts per kind (e.g., `mod io; // 4 traits, 15 structs, 8 fns`). Supports cross-crate facade merging for umbrella crates like bevy.
- **Search pattern DSL**: Three new operators embedded in pattern tokens, no new CLI flags:
  - `*`/`?` — glob wildcards (full-path anchored): `"Shader*Ref"`, `"*Plugin*"`
  - `=term` — exact name match (final `::` segment): `"=Router"` finds Router, not RouterService
  - `-term` — exclusion (global post-filter): `"spawn -test"` finds spawn items, excludes test-related
  - Operators combine freely: `"*Plugin*,*Resource* -test"`, `"-=Name"`
- **`--search-kind` filter**: `--search-kind fn,struct` limits search results by item kind. Comma-separated.
- **Variadic pattern arguments**: `cargo brief search bevy ShaderRef Material` now works without quotes — multiple positional args are AND-matched.

### Fixed

- Cross-crate glob expansion now follows nested `pub use crate::*` chains recursively (max depth 8, cycle-safe) with underscore/hyphen package name fallback.
- `--methods-of` uses exact parent-type segment matching instead of substring, preventing false positives.
- Zero-result sub-crate headers suppressed in normal search output.
- `--crates` positional arg correctly parses `crate::module` syntax.

### Performance

- **Batch rustdoc JSON pre-warming**: Cross-crate facade expansion (e.g., `bevy` with 44+ sub-crates) now batch-generates rustdoc JSON via a single `cargo doc` invocation with `RUSTDOCFLAGS`, instead of sequential per-crate `cargo rustdoc` calls. Package names are validated against `Cargo.lock` before batching (prevents `cargo doc` abort on invalid names). Recursive BFS discovers transitive sub-crate dependencies level by level (max depth 8). Existing per-crate calls hit the pre-warmed cache.
- Cross-crate glob expansion now caches rustdoc JSON for non-workspace dependencies (identified via `cargo metadata`). External deps locked by `Cargo.lock` are treated as immutable — repeat queries skip `cargo rustdoc` entirely.

## [0.5.1] - 2026-03-20

### Fixed

- **Glob re-export resolution**: Items inside `pub(crate)` modules that are glob-re-exported (`pub use module::*`) are now correctly included in the reachable set. Previously, `compute_reachable_set` skipped these modules entirely, making core types like `bevy_pbr::Material`, `StandardMaterial`, and `MeshMaterial3d` invisible to both `search` and `api` pipelines. This is the dominant Rust crate organization pattern (used by bevy, axum, tokio, serde, etc.).

## [0.5.0] - 2026-03-19

### Breaking

- **Subcommand CLI**: The flat CLI is replaced with subcommands. All invocations now require `api`, `search`, or `examples` as the first argument.
  - `cargo brief self` → `cargo brief api self`
  - `cargo brief --search pattern` → `cargo brief search self pattern`
  - `cargo brief --methods-of Type` → `cargo brief search self --methods-of Type`
  - `--search`, `--search-limit`, `--methods-of` flags removed from `api`; use `search` subcommand instead.

### Added

- **Smart-case search matching**: All-lowercase patterns are case-insensitive; patterns with any uppercase character are case-sensitive. Matches ripgrep/vim `smartcase` semantics.
- **Comma-separated OR search**: `"Foo,Bar"` finds items matching either term. Within each comma-group, space-separated words are AND-matched. `"World spawn,despawn"` matches (World AND spawn) OR (despawn).
- **`examples` subcommand stub**: Placeholder for future example grepping (exits with "not yet implemented").

### Changed

- `run_pipeline()` split into `run_api_pipeline()` + `run_search_pipeline()` for clearer API.
- `BriefArgs` split into per-subcommand `ApiArgs`/`SearchArgs`/`ExamplesArgs` with shared `FilterArgs`/`RemoteArgs`/`TargetArgs`/`GlobalArgs` groups.
- `--search-limit` renamed to `--limit` (on `search` subcommand).

## [0.4.1] - 2026-03-19

### Changed

- **Version-normalized cache directories**: Remote crate cache dirs now use `name[version]` format (e.g., `serde[1.0.217]/`) instead of echoing the user's spec verbatim. Different spec forms (`serde`, `serde@1`, `serde@1.0.217`) that resolve to the same version now share a single cache directory, eliminating duplicate multi-GB workspaces.
- **crates.io version resolution**: Before creating a workspace, the exact version is resolved via the crates.io REST API with `semver` matching. API responses are cached for 24 hours at `~/.cache/cargo-brief/crates/versions/`. Offline fallback uses stale cache; exact specs (`serde@1.0.200`) skip the network entirely.
- **`--clean` glob matching**: `--clean serde` now removes all `serde[*]` directories and the version cache, instead of requiring the exact spec used at creation time.
- **Bare specs auto-update**: `cargo brief --crates serde` now picks up new versions after the 24h API cache expires, instead of being permanently pinned by `Cargo.lock`.

### Added

- New dependencies: `semver` 1, `ureq` 2.

## [0.4.0] - 2026-03-18

### Added

- **Cross-crate module following**: Facade crates like `bevy` that re-export modules from sub-crates now work with module targeting, `--search`, and `--recursive`. `cargo brief --crates bevy ecs` follows the re-export chain (`bevy → bevy_internal → bevy_ecs`) to show the actual module contents. `--search` and `--recursive` automatically discover all re-exported sub-crates.
- **rustdoc JSON caching**: Remote crate (`--crates`) pipeline now skips `cargo rustdoc` if the JSON already exists in the target directory. Repeat queries are near-instant.
- **bincode binary cache**: Parsed rustdoc JSON is cached as bincode (`.bin` files alongside `.json`), giving 5-10x faster parse times on subsequent runs.
- **`--clean [SPEC]` flag**: Clear cached remote crate workspaces. `--clean` removes all caches; `--clean serde@1` removes only that crate's cache. Reports freed disk space.

## [0.3.11] - 2026-03-18

### Changed

- **Trait impl noise reduction**: Simple trait impls (no associated types/constants) are now collapsed into per-type summary comments instead of individual `impl Trait for Type { .. }` lines. Example: `// Bytes: Clone, Debug, Default, Eq, Hash, Send, Sync, ...`. Rich trait impls with associated types (e.g., `Deref`, `IntoIterator`) remain expanded. Use `--all` to restore individual impl lines. Dramatically reduces output for crates like `bytes` (120 impl lines → 8 + summary comments).

## [0.3.10] - 2026-03-18

### Added

- **Crate-level `//!` documentation**: root module doc comments are now rendered after the `// crate <name>` header using `//!` prefix. Especially useful for proc-macro crates (e.g., `thiserror`, `serde`) where usage docs live entirely in crate-level comments. Respects `--no-docs`, `--compact`, and `--doc-lines N`. Skipped in search mode.

## [0.3.9] - 2026-03-18

### Improved

- **Module not found UX**: the error message now includes a `// TIP: Try --search "<path>" ...` suggestion, helping users (and AI agents) discover search mode when targeting non-existent modules in facade crates.
- **Search zero-result hint**: when `--search` with 4+ words returns 0 results, a hint explains AND matching semantics and suggests using fewer words.
- **`--search` help text**: doc comment now notes "2-3 words work best" to set expectations upfront.

## [0.3.8] - 2026-03-18

### Fixed

- **Empty trait path in qualified types**: `<F as >::Output` now correctly renders as `F::Output` when rustdoc JSON provides an unresolved trait path.
- **`$crate::` macro hygiene leak**: Paths like `$crate::clone::Clone` from derive macros are now normalized to `clone::Clone`, eliminating confusing macro artifacts.
- **`impl Trait` desugar**: Synthetic generic params from `impl Trait` arguments are no longer shown in angle brackets. Instead, they are re-sugared back to `impl Bounds` in the parameter list (e.g., `fn f(x: impl Display)` instead of `fn f<impl Display: Display>(x: impl Display)`).

## [0.3.7] - 2026-03-18

### Added

- **Where clause / generic bound rendering**: all item types (functions, structs, enums, traits, type aliases, unions, impl blocks) now render `where` clauses from rustdoc JSON. Normal mode uses multi-line format for 2+ predicates; search mode uses compact inline format. Covers `BoundPredicate`, `LifetimePredicate`, and `EqPredicate` variants, including higher-ranked trait bounds (`for<'a>`).

## [0.3.6] - 2026-03-17

### Added

- **EXAMPLES section in `--help`**: 10 copy-pasteable example commands covering local browsing, remote crate inspection, module targeting, search, `--methods-of`, and output density flags. Significantly improves first-use success for AI agents.

## [0.3.5] - 2026-03-16

### Fixed

- **`--methods-of` stack overflow**: `run_pipeline()` no longer recurses infinitely when `--methods-of` is used — the flag is now cleared before the recursive call.

### Added

- **`--doc-lines N`**: limit doc comment rendering to the first N lines. `--doc-lines 0` suppresses all docs (like `--no-docs`). Useful for large crates where full docs dominate output but `--no-docs` loses too much context.
- **Re-export kind annotations**: `pub use` lines now show `// struct`, `// trait`, `// fn`, etc. when the target item is resolved, helping LLMs understand what a re-export refers to without drilling deeper.

## [0.3.4] - 2026-03-16

### Added

- **Attribute rendering**: API-affecting attributes are now shown in pseudo-Rust output.
  - Default: `#[deprecated]` (with `since`/`note` fields) and `#[non_exhaustive]` always rendered.
  - `--verbose-metadata` flag: additionally shows `#[must_use]`, `#[repr(...)]`, `#[no_mangle]`, `#[macro_export]`, `#[export_name]`, `#[target_feature]`.
  - Attributes render before doc comments, matching Rust convention.
- **Search mode attribute markers**: search results now show `[deprecated]` and `[non_exhaustive]` inline prefixes on matching items.

## [0.3.3] - 2026-03-15

### Fixed

- **Reexport-aware reachability filtering**: cross-crate views now correctly show only items reachable through the public API. Private modules containing publicly re-exported items (facade pattern, e.g., `hecs`, `either`) are rendered with their reachable contents, while `pub(crate)` items no longer leak. Replaces the `same_crate=true` hotfix from v0.3.2.
- Restored automatic `same_crate` detection from observer package context (was disabled by the v0.3.2 hotfix).

## [0.3.2] - 2026-03-15

### Added

- `--no-docs` flag: suppress all `///` doc comments from output, reducing token cost 30–50% on doc-heavy crates.
- `--compact` flag: dense output mode — collapses struct fields to `{ .. }`, enum variants to name-only one-liners, traits to `{ .. }`, inherent impls to `{ .. }`. Implies `--no-docs`.
- `--methods-of <TYPE>` shorthand: equivalent to `--search TYPE` with all exclusion flags except `--no-functions`, showing only methods, fields, and variants for a given type.
- **Search impl summary**: struct/enum/union search results now include an inline `// impl (N methods), impl Trait1, ...` comment showing available impls.

### Fixed

- `--crates` now generates rustdoc JSON with `--document-private-items`, fixing facade crates (e.g., hecs) that re-export from private modules showing only `pub use` lines with no type definitions. (**Note:** current fix over-exposes `pub(crate)` items; proper reachability-based filtering is planned.)

## [0.3.1] - 2026-03-15

### Added

- **Search mode** (`--search <PATTERN>`): find leaf items by name across the entire crate. Case-insensitive substring matching on full paths (e.g., `world::World::spawn`). Multiple words are AND-matched. Outputs one-line-per-item with kind prefix and full path.
  - Searches functions, methods, struct fields, enum variants, consts, statics, type aliases, macros, associated types/consts, and container types (struct, enum, trait, union).
  - Respects visibility filtering (`--at-mod`, same-crate detection) and `--no-*` exclusion flags.
  - Results sorted by kind (fn → struct → enum → trait → union → field → variant → const → static → type → macro → assoc → use), then alphabetically within each kind.
  - Re-exports (`pub use X as Y`) are surfaced in search results.
- `--search-limit` flag to cap search output. Accepts `N` (first N results) or `OFFSET:N` (skip OFFSET, show N) for paging through large result sets.
- `--features <FEATURES>` flag for `--crates` to enable specific crate features (comma-separated).

### Fixed

- Glob re-export expansion now uses normalized line matching, fixing cases where whitespace differences caused replacement failures.

## [0.3.0] - 2026-03-14

### Added

- **Remote crate support** (`--crates <spec>`): fetch any crate from crates.io and display its API. Supports version specs: `serde`, `tokio@1`, `serde@1.0.200`.
- **Workspace caching** for `--crates`: remote workspaces are persisted at `~/.cache/cargo-brief/crates/` so repeat calls reuse Cargo's build cache. Configurable via `$CARGO_BRIEF_CACHE_DIR` or `$XDG_CACHE_HOME`.
- `--no-cache` flag to force a fresh temporary workspace (skips cache).
- `--expand-glob` flag: inline full definitions from glob re-export sources instead of individual `pub use` lines.
- **Optional TARGET**: `cargo brief` with no arguments now defaults to `self` (current package).

## [0.2.2] - 2026-03-14

### Added

- **Facade crate support**: crates that use `pub use other_crate::*` (like `clap`) now show expanded individual `pub use` items instead of empty output. The source crate's public API is enumerated automatically.

### Fixed

- Crates with multiple targets (lib + bin + examples, like `clap`) no longer fail with "extra arguments to rustdoc can only be passed to one target". The `--lib` flag is now always passed to `cargo rustdoc`.

## [0.2.1] - 2026-03-14

### Fixed

- Versioned package specifiers (`pkg@version`, e.g. `cargo brief rand@0.10.0`) now work correctly. Previously the `@version` suffix was included in the JSON output filename lookup, causing a "file not found" error.
- When multiple versions of the same crate exist in the dependency tree and the bare name is ambiguous, the error message now lists the available `pkg@version` options with an example command, enabling LLM agents to retry without human intervention.

## [0.2.0] - 2026-03-12

### Changed

- **Breaking**: bare unknown names (e.g., `cargo brief hecs`) now always resolve as a package name, not as a module of the current package. Use `self::module` or file paths for own-module access.
- Visibility auto-detection: `same_crate` is now inferred from the cwd package context when `--at-package` is not specified. External crates correctly hide `pub(crate)` items.

## [0.1.3] - 2026-03-10

### Fixed

- CLI `--help` text now accurately reflects the flexible target resolution system added in v0.1.0. The first positional argument is renamed from `<CRATE_NAME>` to `<TARGET>`, and a RESOLUTION RULES section documents all 6 resolution strategies (self, self::mod, crate::mod, file paths, workspace packages, fallback).
- `--toolchain` no longer shows a redundant default value description.
- `--manifest-path` description clarified to "Path to Cargo.toml".

## [0.1.2] - 2026-03-08

### Changed

- Trait impl rendering now uses `{ .. }` instead of `;` for impls without associated items (e.g., `impl Clone for Foo { .. }`).
- Trait impls with associated types/constants that also have methods now show `// ..` after the listed items to indicate omitted methods.

## [0.1.1] - 2026-03-08

### Fixed

- Single-arg package names (e.g., `cargo brief hecs`) no longer error at virtual workspace roots or directories without a package. Unknown names are now passed through as package names instead of failing with "Cannot resolve 'self'".
- Empty struct bodies now render compactly: `{ .. }` when fields exist but are hidden, `{}` when genuinely empty (no dangling newline).
- Structs with a mix of public and private fields now show `// .. private fields` at the end of the field list (previously only triggered by rustdoc's own stripping, which was inactive under `--document-private-items`).

## [0.1.0] - 2026-03-08

### Added

- **Flexible package name resolution**: the first positional argument now supports multiple syntaxes beyond a literal crate name.
  - `self` keyword resolves to the current package (detected via cwd).
  - `crate::module` single-arg syntax (e.g., `cargo brief hecs::world`).
  - `self::module` syntax (e.g., `cargo brief self::cli`).
  - Single-arg fallback: tries workspace package first, then treats as a module of `self`.
  - Hyphen/underscore normalization when matching workspace packages.
- **File path to module path resolution**: module arguments that look like file paths (contain `/` or end with `.rs`) are automatically converted to module paths.
  - `cargo brief src/cli.rs` → resolves to `self::cli`.
  - `cargo brief self src/model.rs` → resolves to `self::model`.
  - `cargo brief cli.rs` → falls back to `src/cli.rs` if not found at cwd.
  - Handles `lib.rs` (crate root), `mod.rs` (parent directory), nested paths.
- New `src/resolve.rs` module containing all resolution logic and cargo metadata handling.
- `--version` flag support (`cargo brief --version`).

### Changed

- `run_pipeline()` now loads cargo metadata once and uses it for both target resolution and target directory lookup, eliminating a redundant `cargo metadata` call.
- `generate_rustdoc_json()` accepts a `target_dir` parameter instead of calling `cargo metadata` internally.

## [0.0.2] - 2026-03-05

### Added

- Condensed trait impl rendering: simple trait impls shown as one-liners, impls with associated types show only the types.
- README with usage documentation and AI agent setup guide (CLAUDE.md snippet).

### Fixed

- Root-level items no longer have spurious indentation.

## [0.0.1] - 2026-03-04

### Added

- Initial release.
- Core pipeline: CLI argument parsing, rustdoc JSON generation and parsing, visibility-aware API extraction, pseudo-Rust rendering.
- Visibility-aware perspective via `--at-mod` and `--at-package` flags.
- Support for all major item types: structs, enums, traits, functions, type aliases, constants, statics, unions, macros, re-exports, inherent impls, trait impls.
- Item-kind filtering with `--no-*` flags and `--all` for blanket/auto-trait impls.
- Depth control with `--depth` and `--recursive` flags.
- Doc comment preservation.
- Actionable error messages for missing toolchain, package not found, module not found.
- Integration tests and CLI smoke tests.
- MPL-2.0 license.
