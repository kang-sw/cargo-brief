# Remote Crate Support (`--crates` flag) & Default Target

## Goal

1. Allow `cargo brief` to generate API docs for crates not in the local
   workspace/dependencies via `--crates`.
2. Make `<TARGET>` optional — bare `cargo brief` defaults to `self`.

## Motivation

Currently cargo-brief only works with crates reachable via `cargo metadata`
(workspace members and their dependencies). Users may want to quickly inspect
any crate on crates.io without adding it to their project. Additionally,
`cargo brief` with no arguments should "just work" in a package directory.

---

## Phase 1: Make `<TARGET>` optional (separate commit)

**Change:** `crate_name` positional arg gets `default_value = "self"`.

```
cargo brief              → self (current package)
cargo brief utils        → self::utils (current package module)  [existing]
cargo brief serde        → local dependency serde                [existing]
cargo brief --crates serde  → fetch from crates.io               [Phase 2]
```

Virtual workspace root with no `<TARGET>`: same error as today ("no package
found"), with available packages listed.

**Files:** `src/cli.rs` (one-line change), test fixtures if needed.

**Tests:**
- Subprocess test: `cargo brief` from package dir = `cargo brief self`
- Subprocess test: `cargo brief` from virtual workspace root = error

---

## Phase 2: `--crates` flag (cargo-delegated resolution)

### CLI

```
cargo brief --crates <spec> [module_path] [OPTIONS]
```

- `<spec>` uses existing `name@version` convention:
  - `serde` → `serde = "*"` (latest)
  - `serde@1` → `serde = "1"`
  - `serde@1.0.200` → `serde = "=1.0.200"` (exact pin)
- All existing flags (`--expand-glob`, `--no-*`, `--depth`, etc.) apply.
- `--crates` and `<TARGET>` are mutually exclusive: if `--crates` is set,
  positional `<TARGET>` is ignored (or defaults to `self` harmlessly).

### Pipeline

```
run_pipeline()
  ├─ args.crates is Some?
  │   ├─ parse_crate_spec(spec) → (name, version_req)
  │   ├─ create_temp_workspace(name, version_req)   // tempfile::TempDir
  │   │   ├─ write Cargo.toml with [dependencies] name = "version_req"
  │   │   └─ write empty src/lib.rs
  │   ├─ load_cargo_metadata(tmpdir/Cargo.toml)
  │   ├─ generate_rustdoc_json(name, ..., tmpdir target_dir)
  │   ├─ parse → model → render (existing pipeline)
  │   ├─ expand glob re-exports (existing, incl. --expand-glob)
  │   └─ TempDir drop → automatic cleanup
  │
  └─ existing local pipeline (no changes)
```

Version resolution is fully delegated to cargo — no HTTP client, no `semver`
crate. Cargo handles crates.io registry queries, version matching, and
download via the temp workspace's `Cargo.toml`.

### New file: `src/remote.rs`

```rust
pub fn parse_crate_spec(spec: &str) -> (String, String);
pub fn create_temp_workspace(name: &str, version_req: &str) -> Result<TempDir>;
```

- `parse_crate_spec`: splits `name@version` → `(name, version_req_string)`.
  No version → `"*"`. Exact version (3 dots) → `"=x.y.z"`.
- `create_temp_workspace`: writes minimal `Cargo.toml` + empty `src/lib.rs`.
  Returns `TempDir` (auto-cleaned on drop).

### Dependencies

| Crate | Purpose |
|-------|---------|
| `tempfile` | Temp directory with RAII cleanup |

No `semver`, no HTTP client.

### Tests (`tests/remote_crate_integration.rs`)

All tests `#[ignore]` by default (network + build required).
Run manually: `cargo test -- --ignored`.

| Test | Asserts |
|------|---------|
| `remote_serde_latest` | output contains `pub trait Serialize`, `pub trait Deserialize` |
| `remote_serde_pinned_version` | `serde@1.0.200` produces output, crate header present |
| `remote_nonexistent_crate` | clear error message |
| `remote_with_expand_glob` | facade crate via `--crates` + `--expand-glob` works |
| `remote_with_module_path` | `--crates tokio` + module path filters correctly |

### Subprocess tests

| Test | Command |
|------|---------|
| `cli_crates_flag` | `cargo brief --crates serde` — success, output non-empty |
| `cli_crates_version` | `cargo brief --crates serde@1` — success |

---

## Future (out of scope)

- **Caching:** Persist temp workspace or reuse `~/.cargo/registry` artifacts.
  Separate ticket if repeated queries become a pain point.
- **Feature flags:** `--crates serde --features derive`. Needs Cargo.toml
  `features = [...]` support in temp workspace.
- **Multiple crates:** `--crates "serde tokio"` in one invocation.

## Open Questions

- Should `--crates` conflict with `<TARGET>` via clap's `conflicts_with`,
  or silently ignore the positional arg?
  - **Resolved:** silently ignored (TARGET defaults to "self" harmlessly)
- Timeout for cargo build in temp workspace? Large crates (e.g., `bevy`)
  may take minutes on first build.
  - **Deferred:** no timeout implemented; user can Ctrl-C

---

### Result (280c9a2)

**Phase 1 (197f8bf):** Made `<TARGET>` optional with `default_value = "self"`.
Bare `cargo brief` from a package dir now works like `cargo brief self`.
Two subprocess tests added.

**Phase 2 (280c9a2):** Implemented `--crates` flag with temp workspace approach.
- `src/remote.rs`: `parse_crate_spec()` + `create_temp_workspace()` with unit tests
- `src/lib.rs`: `run_remote_pipeline()` branches early, `apply_glob_expansions()` extracted
- `src/render.rs`: Fixed rendering of `Use` items with external targets (serde facade fix)
- `tempfile` added as dependency
- 6 ignored integration/subprocess tests (network), 5 unit tests

**Deviations from plan:**
- Test assertions for serde adapted: serde 1.0.228 is now a facade over `serde_core`,
  so output shows `pub use serde_core::Serialize;` instead of `pub trait Serialize`.
- `remote_with_module_path` test uses `either` instead of `serde::de` (serde modules
  are re-exported, not directly available).
- Added external-target Use item rendering fix (not in original plan) — discovered
  during serde testing.
