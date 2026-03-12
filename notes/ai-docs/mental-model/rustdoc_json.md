# rustdoc_json — JSON Generation & Parsing

**File:** `src/rustdoc_json.rs` (~79 lines)

## Public Functions

### `generate_rustdoc_json(crate_name, toolchain, manifest_path, document_private_items, target_dir) -> Result<PathBuf>`

```rust
pub fn generate_rustdoc_json(
    crate_name: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    document_private_items: bool,
    target_dir: &Path,
) -> Result<PathBuf>
```

Runs: `cargo +{toolchain} rustdoc -p {crate_name} [--manifest-path ...] -- --output-format json -Z unstable-options [--document-private-items]`

Output path: `{target_dir}/doc/{crate_name.replace('-', '_')}.json`

**Error detection** (stderr pattern matching):
- Toolchain not installed → actionable install command
- Package not found → informative message
- Generic failure → full stderr dump
- File not found after success → explicit error

### `parse_rustdoc_json(path: &Path) -> Result<rustdoc_types::Crate>`

Reads file to string, deserializes via `serde_json::from_str()`.

## Design Notes

- Always called with `document_private_items=true` in production (needed for visibility filtering)
- Hyphenated crate names normalized to underscores for output file path
- No caching — regenerates JSON on each invocation
- Self-contained: no internal module dependencies
