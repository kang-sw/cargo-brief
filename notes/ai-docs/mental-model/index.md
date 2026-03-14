# cargo-brief — Mental Model

## Overview

`cargo-brief` is a Cargo subcommand that extracts a Rust crate's API surface as
visibility-aware pseudo-Rust documentation. Primary consumer: AI coding agents
that need to understand crate interfaces without reading full source.

## Pipeline

```
CLI args (BriefArgs)
  │
  ├─[if --crates set]──────────────────────────────────────────────────────────┐
  │  remote::parse_crate_spec()           → (name, version_req)                │
  │  remote::create_temp_workspace()      → TempDir (dropped at end)           │
  │  resolve::load_cargo_metadata(tmp)    → CargoMetadataInfo                  │
  │  rustdoc_json::generate_rustdoc_json()→ PathBuf (external, pub-only JSON)  │
  │  (no resolve_target — module_path used directly)                           │
  └─[else]────────────────────────────────────────────────────────────────────┤
     resolve::load_cargo_metadata()       → CargoMetadataInfo                  │
     resolve::resolve_target()            → ResolvedTarget                     │
     rustdoc_json::generate_rustdoc_json()→ PathBuf (private-items JSON)       │
                                                                                │
  ├─ rustdoc_json::parse_rustdoc_json()   → rustdoc_types::Crate              ─┘
  ├─ model::CrateModel::from_crate()      → CrateModel
  ├─ render::render_module_api()          → String (pseudo-Rust output)
  └─ lib.rs::apply_glob_expansions()      → expands `pub use src::*;` lines
```

Orchestrated by `lib.rs::run_pipeline(&BriefArgs) -> Result<String>`.
Remote path: `lib.rs::run_remote_pipeline` (private, called from `run_pipeline`).

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `cli` | `src/cli.rs` | `BriefArgs` struct (clap derive), `Cargo`/`CargoCommand` for subcommand dispatch |
| `resolve` | `src/resolve.rs` | Cargo metadata loading, flexible target resolution (self, crate::mod, file paths) |
| `rustdoc_json` | `src/rustdoc_json.rs` | Subprocess invocation of `cargo +nightly rustdoc`, JSON parsing |
| `model` | `src/model.rs` | `CrateModel` with module index, `is_visible_from()` visibility logic |
| `render` | `src/render.rs` | Recursive pseudo-Rust renderer with visibility filtering and depth control |
| `remote` | `src/remote.rs` | Crate spec parsing (`name@version`) and temp workspace creation for crates.io fetches |

Entry points: `src/lib.rs` (library, re-exports + `run_pipeline`), `src/main.rs` (binary, arg parsing).

## Dependency Flow (internal)

```
main.rs → lib.rs → resolve → rustdoc_json → model → render
                 ↘ remote ↗                   ↑        ↑
                                            cli.rs   cli.rs + model
```

- `resolve` depends on nothing internal (pure utility)
- `rustdoc_json` depends on nothing internal
- `remote` depends on nothing internal (pure utility — temp workspace + spec parsing)
- `model` depends on `rustdoc_types` (external)
- `render` depends on `model` + `cli`
- `lib.rs` depends on all modules; `apply_glob_expansions` and `expand_glob_reexports` are private helpers there, not in `render`

## Key External Dependencies

| Crate | Version | Role |
|-------|---------|------|
| `rustdoc-types` | 0.57 | Type definitions for rustdoc JSON (`Crate`, `Item`, `ItemEnum`, `Visibility`) |
| `clap` | 4 | CLI argument parsing (derive mode) |
| `serde_json` | 1 | JSON deserialization for cargo metadata + rustdoc output |
| `anyhow` | 1 | Error handling with `.context()` chaining |

## Test Infrastructure

- `tests/integration.rs` — 30+ tests using `test_fixture/` crate
- `tests/subprocess_integration.rs` — binary invocation tests using `test_workspace/`
- `tests/remote_crate_integration.rs` — `#[ignore]` tests for `--crates` (require network)
- `test_fixture/` — Sample crate exercising all item types, visibility levels, generics
- Helper: `fixture_model()` builds `CrateModel` from fixture; `render_full()`/`render_module()` for output assertions
- See `testing.md` for details
