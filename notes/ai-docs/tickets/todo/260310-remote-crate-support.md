# Remote Crate Support (`--crates` flag)

## Goal

Allow `cargo brief` to generate API docs for crates not in the local workspace/dependencies.
Example: `cargo brief --crates quinn` or `cargo brief --crates quinn[v0.1]`.

## Motivation

Currently cargo-brief only works with crates reachable via `cargo metadata` (workspace members
and their dependencies). Users may want to quickly inspect any crate on crates.io without
adding it to their project.

## Design Sketch

### CLI

```
cargo brief --crates <name>[v<semver>] [module_path] [OPTIONS]
```

- `<name>` — crate name on crates.io
- `[v<semver>]` — optional version constraint (semver range). Omitted = latest.
- Examples: `quinn`, `quinn[v0.1]`, `serde[v1.0.200]`

### Pipeline

1. **Version resolution** — Query `https://crates.io/api/v1/crates/<name>` for available
   versions. Apply semver matching to find the best candidate.
2. **Temp workspace** — Create a temporary directory with `cargo init --lib`, add the
   resolved crate as a dependency in `Cargo.toml`.
3. **Rustdoc JSON generation** — Run `cargo +nightly rustdoc -p <crate>` in the temp
   workspace. Reuse existing `rustdoc_json.rs` logic with the temp target dir.
4. **Parse & render** — Feed the JSON into the existing model/render pipeline.
   External crate = pub-only visibility (no `--at-mod` needed).
5. **Cleanup** — Remove temp directory.

### Dependencies

- `semver` crate for version matching
- HTTP client (`ureq` or `reqwest` blocking) for crates.io API
- `tempfile` crate for temp workspace

### Open Questions

- Cache downloaded crate docs to avoid re-building on repeated queries?
- Support multiple crates in a single invocation?
- How to handle crates with complex feature flags?
