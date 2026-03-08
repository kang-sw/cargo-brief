# Changelog

All notable changes to this project will be documented in this file.

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

### Changed

- `run_pipeline()` now loads cargo metadata once and uses it for both target resolution and target directory lookup, eliminating a redundant `cargo metadata` call.
- `generate_rustdoc_json()` accepts a `target_dir` parameter instead of calling `cargo metadata` internally.

## [0.0.2] - 2026-03-05

### Added

- Condensed trait impl rendering: simple trait impls shown as one-liners (`impl Trait for Type;`), impls with associated types show only the types.
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
