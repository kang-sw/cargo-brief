# resolve — Target Resolution & Cargo Metadata

**File:** `src/resolve.rs`

## Public Types

### `CargoMetadataInfo`
```rust
pub struct CargoMetadataInfo {
    pub workspace_packages: Vec<String>,
    pub current_package: Option<String>,              // cwd-based detection
    pub current_package_manifest_dir: Option<PathBuf>,
    pub target_dir: PathBuf,
}
```
Single source of truth from one `cargo metadata --format-version=1 --no-deps` call.

### `ResolvedTarget`
```rust
pub struct ResolvedTarget {
    pub package_name: String,
    pub module_path: Option<String>,  // e.g., Some("foo::bar")
}
```

## Public Functions

### `load_cargo_metadata(manifest_path: Option<&str>) -> Result<CargoMetadataInfo>`
Invokes `cargo metadata`, parses JSON, detects current package by matching cwd
against package manifest directories.

### `resolve_target(first_arg: &str, second_arg: Option<&str>, metadata: &CargoMetadataInfo) -> Result<ResolvedTarget>`

**Resolution priority (4 cases):**

1. **`"self"`** — current package; second arg as module (strips `self::` prefix)
2. **Contains `"::"`** — splits at first `::`: prefix is package (or `self`), rest is module
3. **Two args** — first=package, second=module (backward compat)
4. **Single arg fallback:**
   - File path detected (`/` or `.rs`) → convert to module, use current package
   - Workspace package match (hyphen/underscore normalized) → that package
   - Has current package → treat as self module
   - Else → assume external package name

## Key Helpers

- `is_file_path(s) -> bool` — contains `/` or ends with `.rs`
- `file_path_to_module_path(input, metadata) -> Result<Option<String>>` — 2-level fallback: cwd-relative, then package `src/`-relative
- `path_components_to_module(relative: &Path) -> Result<Option<String>>` — converts path to `::` module path (`lib.rs`→None, `mod.rs`→parent, else→stem)
- `find_workspace_package(packages, query) -> Option<String>` — hyphen/underscore normalized lookup
- `strip_self_prefix(s) -> &str` — removes leading `self::`

## Dependencies
- External: `std::process::Command`, `serde_json`, `anyhow`
- Internal: none (pure utility module)
- Used by: `lib.rs::run_pipeline()`
