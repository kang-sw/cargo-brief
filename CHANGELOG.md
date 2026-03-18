# Changelog

All notable changes to this project will be documented in this file.

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
