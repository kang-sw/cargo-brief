---
title: "rustdoc_json: harden lib target name JSON lookup from PR #4"
spec:
  - 260423-remote-mode-flag
  - 260423-cross-crate-facade-expansion
related-mental-model:
  - remote-pipeline
  - target-resolution
completed: 2026-05-05
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

The upstream PR did not perform the repository documentation step: no spec,
mental-model, README, or changelog updates were included with the external
change. Treat any documentation impact as local integration work, not as already
handled by PR #4.

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

### Result (81f84b0) - 2026-05-05

Added `renamed-lib-package` to the integration fixture workspace with Cargo
package name `renamed-lib-package` and Rust lib target name
`renamed_lib_actual`. The regression test exercises
`generate_rustdoc_json()` with `use_cache=false` to verify post-generation JSON
discovery and then with `use_cache=true` to verify cached lookup through the lib
target filename.

### Phase 2: Bound fallback metadata cost and error behavior

Review `find_lib_json_path` and its fallback metadata lookup. Avoid repeated
metadata subprocesses for the same package/doc-dir context when the expected
package-derived JSON path is absent. Keep failure reporting useful enough that
metadata failure is not indistinguishable from a missing rustdoc output file
when the fallback was required.

Also decide whether this bugfix requires spec or mental-model documentation
after the implementation is hardened. The upstream PR provided no documentation
pass, so absence of docs in the PR must not be treated as an intentional no-op.

Success criteria:

- Repeated mismatched-lib-name lookups do not repeatedly shell out in the same
  operation when a cheap cache or caller-owned mapping is practical.
- Error context still points users at the expected package-derived path while
  preserving enough fallback context for diagnosis.
- `cargo test` passes.

### Result (81f84b0) - 2026-05-05

Cached the manifest-level Cargo package name to Rust lib/proc-macro target name
map in process, so batch and BFS callers do not repeatedly shell out for every
mismatched package lookup. The implementation deliberately does not cache JSON
file existence, because batch generation can create files after an earlier miss
in the same operation.

Failure context still reports the package-derived expected path and now includes
the fallback metadata result when available. Documentation follow-up landed in
`228bf0d`, updating the remote cache spec and mental models for lib-target JSON
filenames and the cached metadata fallback.
