---
title: "rustdoc_json: harden lib target name JSON lookup from PR #4"
spec:
  - 260423-remote-mode-flag
  - 260423-cross-crate-facade-expansion
related-mental-model:
  - remote-pipeline
  - target-resolution
---

# rustdoc_json: harden lib target name JSON lookup from PR #4

## Background

PR #4 fixes a real rustdoc JSON lookup bug: crates can set `[lib] name` to a
different Rust crate name than the Cargo package name, and rustdoc writes
`target/doc/<lib_name>.json`, not `target/doc/<package_name>.json`.

The PR direction is correct and belongs near rustdoc JSON path resolution, not
in each caller. The integration branch already includes the upstream PR. This
ticket tracks the review hardening needed before shipping it from the local
integration branch.

## Decisions

- Keep the lookup abstraction in `rustdoc_json`; callers should not hand-roll
  package-name-to-json-name derivation.
- Preserve the package-derived fast path for common crates.
- Treat extra `cargo metadata` subprocesses as a hot-path risk. Repeated cache
  hits for crates with mismatched package/lib names should not repeatedly pay
  the fallback discovery cost if the mapping can be retained cheaply.

## Phases

### Phase 1: Add fixture coverage for package/lib name mismatch

Add a focused fixture or integration case where the Cargo package name differs
from `[lib] name`. The test should prove that rustdoc JSON is found under the
lib target name after generation and through a cached lookup.

Success criteria:

- The failing pre-PR shape is represented.
- The test exercises the public pipeline or a narrowly public helper without
  duplicating rustdoc internals.
- Normal package-name-equals-lib-name behavior remains covered by existing
  tests.

### Phase 2: Bound fallback metadata cost and error behavior

Review `find_lib_json_path` and its fallback metadata lookup. Avoid repeated
metadata subprocesses for the same package/doc-dir context when the expected
package-derived JSON path is absent. Keep failure reporting useful enough that
metadata failure is not indistinguishable from a missing rustdoc output file
when the fallback was required.

Success criteria:

- Repeated mismatched-lib-name lookups do not repeatedly shell out in the same
  operation when a cheap cache or caller-owned mapping is practical.
- Error context still points users at the expected package-derived path while
  preserving enough fallback context for diagnosis.
- `cargo test` passes.

