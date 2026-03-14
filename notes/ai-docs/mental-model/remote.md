# remote — Remote Crate Fetching

**File:** `src/remote.rs`

## Entry Points

`src/remote.rs` — two public functions used exclusively by `lib.rs::run_remote_pipeline`.

## Module Contracts

- `parse_crate_spec(spec)` guarantees: bare name → `version_req = "*"`; `name@major` or `name@major.minor` → version req passed verbatim; `name@x.y.z` (3 components) → `"=x.y.z"` (exact pin).
- `create_temp_workspace(name, version_req)` guarantees: returned `TempDir` contains a valid `Cargo.toml` with the crate as a dependency and an empty `src/lib.rs`. The workspace is deleted when `TempDir` is dropped.

## Common Mistakes

- `TempDir` must be kept alive for the entire duration of the remote pipeline. `lib.rs::run_remote_pipeline` holds it as a local variable — any refactor that drops it before `rustdoc_json::generate_rustdoc_json` or `resolve::load_cargo_metadata` complete will cause those calls to fail with a "no such file" error, not a meaningful domain error.
- The generated `Cargo.toml` uses `edition = "2021"` and package name `"brief-tmp"`. If cargo-brief itself is ever run against the temp workspace as a target (not the dependency), it will find `brief-tmp`, not the intended crate.
