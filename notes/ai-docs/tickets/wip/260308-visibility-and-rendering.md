# Visibility Auto-Detection, Resolution Priority, Rendering Fixes

## Context

After implementing flexible package name resolution (self, crate::module, file paths),
several issues were discovered during manual testing:

1. `same_crate` is always `true` → `pub(crate)` items shown for external crates
2. Single-arg resolution: unknown names become self-module, but should try as package first
3. `impl Trait for Type;` breaks Rust syntax highlighters
4. Multi-target crates (like clap) fail without `--lib`
5. External (non-workspace) dependencies have sparse rustdoc JSON index

### Root Cause Analysis

**Issue 5 is the critical blocker.** When running `cargo rustdoc -p hecs`, rustdoc
generates JSON with the full index because `hecs` is a workspace dependency with source
available. But `--document-private-items` may affect what goes into the index for
external deps. Need to verify:

- Does `cargo rustdoc -p <dep>` work the same with and without `--document-private-items`?
- Is the sparse index only for crates not in the dependency tree?
- Does adding `--lib` change the output?

**Issue 1 (same_crate)** was attempted but reverted because it interacted with issue 5.
The fix itself is correct but needs issue 5 resolved first to avoid hiding everything.

### What Was Tried and Reverted

- `same_crate` auto-detection: compared `resolved.package_name` against
  `metadata.current_package`. Correct logic but broke external crate viewing
  because external crate JSON had no items (separate issue).
- Single-arg always-package: removed self-module fallback entirely. Too aggressive —
  breaks `cargo brief cli` as shorthand for `cargo brief self::cli`.
- `--lib` flag: added to `cargo rustdoc` command. Needed for multi-target packages
  but untested whether it changes JSON content.
- `impl Trait for Type { .. }`: correct change, was bundled with same_crate and reverted together.

---

## Implementation Plan

### Phase 1: Comprehensive subprocess integration tests

**Goal:** Establish a safety net of subprocess-based integration tests covering all
resolution and visibility scenarios BEFORE changing any behavior. Tests define the
**desired** behavior — some may initially fail for known bugs. Failing tests are
marked `#[ignore]` with a comment explaining why.

**Approach:** Run the `cargo-brief` binary via `std::process::Command` with explicit
cwd and args. This tests the full pipeline including cwd detection, `self` resolution,
and arg parsing — things `run_pipeline()` can't exercise in-process.

**Test file:** `tests/subprocess_integration.rs`

**Fixture:** Existing `test_workspace/` (core-lib + app + either dependency).
Expand fixture if needed (e.g., add `pub(in crate::utils)` items).

#### Scenario categories:

**A. Explicit crate name (workspace member)**
- `cargo brief core-lib` from workspace root → shows core-lib API
- `cargo brief app` from workspace root → shows app API
- `cargo brief core_lib` (underscore) → normalizes to core-lib

**B. `self` keyword resolution (cwd-dependent)**
- `cargo brief self` from `test_workspace/core-lib/` → core-lib API
- `cargo brief self` from `test_workspace/app/` → app API
- `cargo brief self::utils` from `test_workspace/core-lib/` → utils module
- `cargo brief self` from `test_workspace/` (virtual root) → error

**C. `crate::module` syntax**
- `cargo brief core-lib::utils` from workspace root → utils module of core-lib

**D. File path as module**
- `cargo brief src/utils.rs` from `test_workspace/core-lib/` → utils module
- `cargo brief self src/utils.rs` from `test_workspace/core-lib/` → utils module
- `cargo brief core-lib src/utils.rs` from workspace root → utils module

**E. External crate (dependency, not workspace member)**
- `cargo brief either` from workspace root → either's pub API
- Should show `pub` items only (same_crate=false for deps)

**F. Visibility auto-detection (no explicit `--at-package`)**
- `cargo brief core-lib` from `test_workspace/app/` → should auto-detect
  observer=app, hide `pub(crate)` items of core-lib
- `cargo brief core-lib` from `test_workspace/core-lib/` → observer=core-lib,
  show `pub(crate)` items (same crate)
- `cargo brief app` from `test_workspace/core-lib/` → observer=core-lib,
  hide `pub(crate)` items of app

**G. `--at-package` / `--at-mod` explicit override**
- `cargo brief core-lib --at-package app` → cross-crate view
- `cargo brief core-lib --at-package core-lib --at-mod utils` → same-crate, from utils

**H. Depth and recursion**
- `cargo brief core-lib --depth 0` → modules collapsed
- `cargo brief core-lib --recursive` → all modules expanded

**I. Item filtering**
- `cargo brief core-lib --no-structs` → no structs in output
- `cargo brief core-lib --no-functions --no-traits` → combined exclusion

**J. Error cases**
- `cargo brief nonexistent-crate` → meaningful error
- `cargo brief self` from non-package directory → error about no package

#### Test helper design:

```rust
fn cargo_brief(cwd: &str, args: &[&str]) -> (String, String, bool) {
    // Returns (stdout, stderr, success)
    // cwd relative to project root, e.g., "test_workspace/core-lib"
    // Builds binary path from CARGO_BIN_EXE_cargo-brief or cargo build
}
```

#### Initially `#[ignore]` tests (known bugs, un-ignore as fixed):

- **F scenarios** (visibility auto-detection) — blocked by same_crate always=true
- **E scenarios** (external crate) — blocked by sparse JSON / --document-private-items issue
- Possibly some file path scenarios depending on cwd behavior

---

### Phase 2: Investigation — external crate JSON

Before writing fixes, answer these questions by experimentation:

1. Run `cargo rustdoc -p either -- --output-format json -Z unstable-options`
   (no `--document-private-items`) from `test_workspace/`. Inspect JSON index size.
2. Compare with `--document-private-items`. Does it reduce the index?
3. Run with `--lib` flag added. Does it change anything?
4. Test from workspace root vs package directory. Any difference?

Document findings in a `### Result` subsection.

---

### Phase 3: External crate rustdoc JSON fix

Based on Phase 2 findings, fix `generate_rustdoc_json()`:

- If `--document-private-items` causes sparse index for deps, make it conditional:
  use it only for workspace packages (where we need visibility filtering),
  omit it for external dependencies (where everything visible is `pub` anyway).
- Add `--lib` flag to avoid multi-target errors.
- `run_pipeline()` needs to know whether the target is a workspace package to decide
  the `document_private_items` flag.
- Un-ignore Phase 1 external crate tests (category E).

**Files:** `src/rustdoc_json.rs`, `src/lib.rs`, `src/resolve.rs` (expose `is_workspace_package`)

---

### Phase 4: same_crate auto-detection

Re-apply the reverted logic, now safe because external crates generate proper JSON:

- No `--at-package` → compare `resolved.package_name` against `metadata.current_package`
- Same package → `same_crate = true`
- Different package (workspace sibling or external) → `same_crate = false`
- `--at-package` explicit → override as before
- Un-ignore Phase 1 visibility auto-detection tests (category F).

**Files:** `src/lib.rs`

---

### Phase 5: Single-arg resolution priority

Change the fallback for unknown single-arg names:

- Current: unknown → self module (breaks external crates like `hecs`)
- Desired: unknown → try as package first (workspace + dependency lookup),
  if no match then try as self module
- Alternative (simpler): unknown → always package. Users must use `self::mod`
  or `self mod` for self modules.

Decision needed: which approach? The simpler "always package" is more predictable
and `self::module` / file paths cover the self-module use case well.

**Files:** `src/resolve.rs`

---

### Phase 6: Rendering fixes

- `impl Trait for Type;` → `impl Trait for Type { .. }` (syntax highlighter compat)
- Update test assertions

**Files:** `src/render.rs`, `tests/integration.rs`

---

### Phase 7: Version bump and docs

Update version, update mental model docs, update `_index.md` operational state.

---

### Result — Phase 1 (subprocess integration tests)

**Implemented:** `tests/subprocess_integration.rs` — 23 subprocess-based integration tests
covering all resolution and visibility scenarios (A–J) using `test_workspace/`.

**Test results:** 19 passing, 4 ignored:
- 3 ignored in category F (auto-visibility): blocked by same_crate always=true (Phase 4)
- 1 ignored in category D (`pkg_with_file_path`): file path not resolved relative to
  package dir when cwd differs from package dir — discovered bug

**Key findings:**
- External crate support (`either`) works out of the box — no `#[ignore]` needed for category E
- `--at-package` override works correctly for both same-crate and cross-crate views
- `self`, `self::module`, `crate::module`, file path, underscore normalization all work
- The `pkg_with_file_path` bug: `cargo brief core-lib src/utils.rs` from workspace root
  fails because file path is resolved relative to cwd, not the target package directory

### Result — Phase 4 (same_crate auto-detection)

**Implemented:** `src/lib.rs` — use `metadata.current_package` (cwd-based) as the
default observer when no `--at-package` is provided. If cwd package matches target →
`same_crate = true`. If different or no cwd package → `same_crate = false`.

**Change:** 3-line replacement in `run_pipeline()` (lines 44-53).

**Test updates:**
- Un-ignored 3 auto-visibility subprocess tests (category F) — all pass
- Updated 5 `external_crate_integration.rs` tests: `either` is now correctly viewed
  as cross-crate, hiding `pub(crate)` modules (`iterator`, `into_either`)
- Updated 3 `workspace_integration.rs` tests: added explicit `at_package` for
  same-crate test scenarios (since in-process tests run from cargo-brief cwd)

**Phases 2-3 skipped:** External crate JSON issue (sparse index) was not observed —
`either` works correctly in both subprocess and in-process tests.

---

## Open Questions

1. Should `cargo brief <unknown>` be package-first or self-module-first?
   → Leaning toward package-first (simpler, more predictable)
2. Should `--document-private-items` be conditional per-target?
   → Likely yes, but need Phase 2 investigation to confirm
3. Is `--lib` always safe to add?
   → Need to verify with crates that only have bin targets
