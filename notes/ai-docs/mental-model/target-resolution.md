# Target Resolution

## Entry Points
- `src/resolve.rs` — `resolve_target()` (4-case algorithm), `load_cargo_metadata()`.
- `src/main.rs` — dual invocation dispatch (`cargo brief` vs `cargo-brief`).

## Module Contracts
- `resolve_target()` guarantees: given CLI args and metadata, returns a `ResolvedTarget` with `package_name` (always a package name, never a module) and `module_path` (optional). For workspace package matches, `find_workspace_package()` returns the actual workspace package name (e.g., `my-crate`), not the user's raw input (e.g., `my_crate`).
- `load_cargo_metadata()` guarantees: single `cargo metadata --no-deps` call; `current_package` is detected by matching cwd against package manifest directories.

## Coupling
- `ResolvedTarget.package_name` ↔ `CrateModel.crate_name()`: These use different naming conventions. Package names use hyphens (`my-crate`), crate names use underscores (`my_crate`). `lib.rs:68` normalizes with `replace('-', "_")` for `same_crate` detection, but other comparisons may not normalize.
- `--crates` bypass (lib.rs:27-30): When `args.crates` is `Some`, the entire resolve pipeline is skipped. `args.crate_name` is silently ignored. No `conflicts_with` in clap prevents passing both.
- File path detection: `is_file_path()` triggers on `/` or `.rs` suffix. False positives possible for crate names containing `.rs` (unlikely but not validated).

## Extension Points & Change Recipes
- **Add a new resolution case**: Add to the match chain in `resolve_target()`. Cases are evaluated in priority order: `"self"` → contains `"::"` → two-arg → single-arg fallback. New cases must slot into this chain.
- **Change `self` detection**: Modify `load_cargo_metadata()`. The `current_package` field is set by matching cwd against manifest directories. Virtual workspace roots produce `current_package: None`.

## Common Mistakes
- Passing `cargo brief serde --crates tokio`: `serde` stored in `args.crate_name` is silently ignored; `tokio` from `--crates` is used. No error or warning.
- Running from virtual workspace root without `--at-package`: `current_package` is `None`, `same_crate` becomes unconditionally `false`. All `pub(crate)` items hidden.
- File path resolution uses three fallbacks: (1) cwd-relative, (2) package `src/`-relative, (3) package-root-relative. If a file exists at multiple locations, the first match wins — potentially resolving to the wrong module.

## Technical Debt
- For non-workspace-match cases (e.g., external crate names in Case 4 fallback), `resolve_target()` returns the raw user input as `package_name`. If the user passes an underscore variant for an external crate, `same_crate` detection at `lib.rs:68` handles it via `replace('-', "_")`, but other downstream comparisons may not normalize.
