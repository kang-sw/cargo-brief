# cli — CLI Argument Definitions

**File:** `src/cli.rs`

## Entry Points

`src/cli.rs` — `BriefArgs` struct (clap derive). Read `src/main.rs` for dual-invocation dispatch logic.

## Module Contracts

`cli` guarantees `BriefArgs` is `Clone` and carries all configuration consumed by every pipeline stage. No pipeline function modifies args — they are read-only inputs.

## Extension Points & Change Recipes

Adding a new flag to `BriefArgs` requires updating **all** explicit struct constructions in tests (`tests/integration.rs`, `tests/subprocess_integration.rs`, `tests/remote_crate_integration.rs`, `tests/workspace_integration.rs`). Missing a site causes a compile error, not a silent failure — but the list grows with each new test file.

## Common Mistakes

- `crate_name` defaults to `"self"` — it is an optional positional arg. Constructing `BriefArgs` in tests must set `crate_name: "self".to_string()` (or appropriate value) explicitly; the default only applies via clap at the CLI layer.
- When `crates: Some(spec)` is set, `crate_name` is **ignored** by `run_pipeline` — the remote pipeline takes over. Setting `crate_name` to anything other than `"self"` in that case has no effect.
- `--no-constants` also suppresses statics (non-obvious grouping).

## Coupling

- `--crates` triggers `run_remote_pipeline` in `lib.rs` before any local metadata is loaded. The `remote` module owns spec parsing and temp workspace creation.
- Dual invocation (`main.rs`): `cargo brief ...` parses `Cargo`, extracts `BriefArgs`; `cargo-brief ...` parses `BriefArgs` directly. Detection: `args[1] == "brief"`.
