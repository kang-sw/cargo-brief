# cargo-brief — Mental Model

## Overview

`cargo-brief` is a Cargo subcommand that extracts a Rust crate's API surface as
visibility-aware pseudo-Rust documentation. Primary consumer: AI coding agents
that need to understand crate interfaces without reading full source.

## Pipeline

```
CLI args (BriefArgs)
  │
  ├─ resolve::load_cargo_metadata()       → CargoMetadataInfo
  ├─ resolve::resolve_target()            → ResolvedTarget { package_name, module_path }
  ├─ rustdoc_json::generate_rustdoc_json()→ PathBuf (JSON file)
  ├─ rustdoc_json::parse_rustdoc_json()   → rustdoc_types::Crate
  ├─ model::CrateModel::from_crate()      → CrateModel (indexed module tree)
  └─ render::render_module_api()          → String (pseudo-Rust output)
```

Orchestrated by `lib.rs::run_pipeline(&BriefArgs) -> Result<String>`.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `cli` | `src/cli.rs` | `BriefArgs` struct (clap derive), `Cargo`/`CargoCommand` for subcommand dispatch |
| `resolve` | `src/resolve.rs` | Cargo metadata loading, flexible target resolution (self, crate::mod, file paths) |
| `rustdoc_json` | `src/rustdoc_json.rs` | Subprocess invocation of `cargo +nightly rustdoc`, JSON parsing |
| `model` | `src/model.rs` | `CrateModel` with module index, `is_visible_from()` visibility logic |
| `render` | `src/render.rs` | Recursive pseudo-Rust renderer with visibility filtering and depth control |

Entry points: `src/lib.rs` (library, re-exports + `run_pipeline`), `src/main.rs` (binary, arg parsing).

## Dependency Flow (internal)

```
main.rs → lib.rs → resolve → rustdoc_json → model → render
                                                ↑        ↑
                                              cli.rs   cli.rs + model
```

- `resolve` depends on nothing internal (pure utility)
- `rustdoc_json` depends on nothing internal
- `model` depends on `rustdoc_types` (external)
- `render` depends on `model` + `cli`

## Key External Dependencies

| Crate | Version | Role |
|-------|---------|------|
| `rustdoc-types` | 0.57 | Type definitions for rustdoc JSON (`Crate`, `Item`, `ItemEnum`, `Visibility`) |
| `clap` | 4 | CLI argument parsing (derive mode) |
| `serde_json` | 1 | JSON deserialization for cargo metadata + rustdoc output |
| `anyhow` | 1 | Error handling with `.context()` chaining |

## Test Infrastructure

- `tests/integration.rs` — 30+ tests using `test_fixture/` crate
- `test_fixture/` — Sample crate exercising all item types, visibility levels, generics
- Helper: `fixture_model()` builds `CrateModel` from fixture; `render_full()`/`render_module()` for output assertions
- See `testing.md` for details
