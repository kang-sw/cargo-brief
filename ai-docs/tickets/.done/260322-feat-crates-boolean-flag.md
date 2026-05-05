---
title: "Restructure --crates as boolean mode switch with clean subcommand"
status: done
started: 2026-03-22
completed: 2026-03-22
---

# Restructure `--crates` as boolean mode switch + `clean` subcommand

## Motivation

Currently `--crates <SPEC>` takes a crate spec as its value and lives on each subcommand's
`RemoteArgs`. This is cognitively backwards — "local vs crates.io" is a top-level decision,
not a per-subcommand option. Additionally, the current design creates ambiguity in `search`
where the first positional (TARGET) gets eaten instead of being treated as a pattern.

## Design

### Phase 1: CLI restructure (breaking — v0.6)

**`-C` / `--crates` becomes a boolean flag on `BriefDirect`** with `global = true`.
The crate spec moves into the existing TARGET positional argument.

Before:
```
cargo brief api --crates serde@1 --compact
cargo brief search --crates axum@0.8 Router route
```

After:
```
cargo brief -C api serde@1 --compact
cargo brief -C search axum@0.8 Router route
```

**`-F` / `--features`** also moves to `BriefDirect` with `global = true`.

**`--no-cache`** same treatment.

**`--clean [SPEC]` replaced by `clean` subcommand:**
```
cargo brief clean              # clear all cached workspaces
cargo brief clean serde        # clear specific crate
```

### Structural changes

- `RemoteArgs` struct removed (or reduced to empty).
- `BriefDirect` gains: `crates: bool`, `features: Option<String>`, `no_cache: bool`.
- `BriefCommand` gains `Clean(CleanArgs)` variant with optional spec positional.
- `parse_command()` returns `(BriefDirect, BriefCommand)` or equivalent — the boolean
  flag lives on `BriefDirect` and must be threaded to pipeline functions.
- Pipeline functions receive the remote-mode boolean + read spec from TARGET.
- `--clean` match arms in main.rs collapse to single `Clean` variant.

### Integration test impact

All tests using `RemoteArgs { crates: Some(...), .. }` need restructuring.
Mechanical but wide (~156 tests).

## Acceptance criteria

- `cargo brief -C api serde@1` works (fetches from crates.io)
- `cargo brief api self` works (local, no -C)
- `-C` works before or after subcommand name (global = true)
- `-F feat1,feat2` works with `-C`
- `cargo brief clean [SPEC]` replaces all `--clean` usage
- `--crates <SPEC>` old syntax is removed
- All existing tests pass (updated to new arg structure)
- Version bumped to 0.6.0 with CHANGELOG entry

### Result — 26-03-22

**Implemented Phase 1 fully.** All structural changes from design doc completed.

Key changes:
- `RemoteArgs` struct removed entirely. `RemoteOpts` (plain struct, `Default`-derivable) replaces it.
- `BriefDirect` has `-C`, `-F`, `--no-cache` as `global = true` clap flags.
- `BriefCommand::Clean(CleanArgs)` added. `--clean` flag removed from all subcommands.
- `parse_command()` returns `BriefDirect` (not `BriefCommand`). `remote_opts()` extracts `RemoteOpts`.
- All 4 pipeline functions take `remote: &RemoteOpts` as second parameter.
- `build_remote_context_api/summary` extract module path from `::` in spec (e.g., `tokio@1::net`).
- Search/examples `Cow`-based workarounds removed — no ambiguity with `-C`, TARGET IS the spec.
- All 14 test files updated. 156 integration tests pass. 5 pre-existing nightly rustdoc failures.

No deviations from the plan.
