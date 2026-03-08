# Project: Visibility Auto-Detection, Resolution Priority, Rendering Fixes

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

### Phase 1: Investigation (plan mode)

Before writing any code, answer these questions by experimentation:

1. Run `cargo rustdoc -p hecs -- --output-format json -Z unstable-options` (no `--document-private-items`)
   and inspect the JSON. Compare index size with `--document-private-items`.
2. Run with `--lib` flag added. Does it change anything?
3. Test from a virtual workspace root vs a package directory. Any difference?
4. Check if the issue is specific to certain crates or universal for all deps.

### Phase 2: External crate rustdoc JSON fix

Based on Phase 1 findings, fix `generate_rustdoc_json()`:

- If `--document-private-items` causes sparse index for deps, make it conditional:
  use it only for workspace packages (where we need visibility filtering),
  omit it for external dependencies (where everything visible is `pub` anyway).
- Add `--lib` flag to avoid multi-target errors.
- `run_pipeline()` needs to know whether the target is a workspace package to decide
  the `document_private_items` flag.

**Files:** `src/rustdoc_json.rs`, `src/lib.rs`

### Phase 3: same_crate auto-detection

Re-apply the reverted logic, now safe because external crates generate proper JSON:

- No `--at-package` → compare `resolved.package_name` against `metadata.current_package`
- Same package → `same_crate = true`
- Different package (workspace sibling or external) → `same_crate = false`
- `--at-package` explicit → override as before

**Files:** `src/lib.rs`

### Phase 4: Single-arg resolution priority

Change the fallback for unknown single-arg names:

- Current: unknown → self module (breaks external crates like `hecs`)
- Desired: unknown → try as package first (run `cargo rustdoc`), if that fails
  then try as self module
- Alternative (simpler): unknown → always package. Users must use `self::mod`
  or `self mod` for self modules. This is cleaner but changes existing behavior.

Decision needed: which approach? The simpler "always package" is more predictable
and `self::module` / file paths cover the self-module use case well.

**Files:** `src/resolve.rs`

### Phase 5: Rendering fixes

- `impl Trait for Type;` → `impl Trait for Type { .. }` (syntax highlighter compat)
- Update test assertions

**Files:** `src/render.rs`, `tests/integration.rs`

### Phase 6: Test workspace

Create `test_workspace/` with multiple packages to test:

- Virtual workspace root with 2+ member packages
- Packages that depend on each other (workspace sibling visibility)
- Integration tests exercising: self resolution, sibling visibility,
  pub(crate) filtering, file path resolution across packages

**Files:** `test_workspace/`, `tests/workspace_integration.rs`

### Phase 7: CHANGELOG and version bump

Update CHANGELOG.md, bump to 0.1.2, update mental model.

---

## Open Questions

1. Should `cargo brief <unknown>` be package-first or self-module-first?
   → Leaning toward package-first (simpler, more predictable)
2. Should `--document-private-items` be conditional per-target?
   → Likely yes, but need Phase 1 investigation to confirm
3. Is `--lib` always safe to add?
   → Need to verify with crates that only have bin targets
