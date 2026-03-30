---
title: Auto-detect and offer to install nightly toolchain
started: 2026-03-30
completed: 2026-03-30
plans:
  phase-1: 2026-03/30-1600.nightly-toolchain-auto-install
---

# Auto-detect and offer to install nightly toolchain

## Problem

cargo-brief requires `cargo +nightly rustdoc --output-format json -Z unstable-options`
for its core pipeline (`api`, `search`, `summary`, `code` default mode). When nightly
is not installed, the user gets an error only **after** cargo attempts to run — and the
error message just says "install it with: rustup toolchain install nightly" without
offering to do it.

Rustdoc JSON stabilization has no timeline (tracking issue rust#76578, 2/5 tasks done),
so nightly dependency will persist for the foreseeable future.

## Goal

Improve first-run UX: detect missing nightly **before** invoking cargo, and offer
interactive installation when a TTY is available.

## Constraints

- Pre-check must be cheap. `rustup which rustdoc --toolchain nightly` takes ~10ms
  (local filesystem only, no network). Acceptable.
- Non-interactive environments (CI, piped stdin) must not hang on a prompt — fall back
  to an actionable error message.
- The `--toolchain` flag may specify a non-default name (e.g., `nightly-2026-03-15`).
  The check must respect the user-supplied toolchain name.
- Subcommands that don't need nightly (`ts`, `examples`, `code --no-deps`,
  `code --all-deps`, `lsp`, `clean`) must not trigger the check.

## Rejected Alternatives

- **Avoid nightly entirely**: Not feasible. `--output-format json` is gated behind
  `-Z unstable-options` which requires nightly. `syn`-based parsing can't handle
  macro expansion or cross-crate re-exports.
- **`RUSTC_BOOTSTRAP=1` hack**: Officially discouraged, no stability guarantees,
  inappropriate for a distributed tool.
- **Check `~/.rustup/toolchains/` directory directly**: Saves one process spawn
  (~10ms) but loses rustup's toolchain resolution logic (aliases, host triples).
  Not worth the fragility.

### Phase 1: Pre-check and interactive install prompt

**Detection**: Before the first `cargo +{toolchain} rustdoc` invocation, run
`rustup which rustdoc --toolchain {toolchain}`. If it exits non-zero, the
toolchain is missing.

**Interactive flow** (stderr is a TTY):
```
cargo-brief requires the '{toolchain}' toolchain for rustdoc JSON generation,
but it is not installed.

Install it now? [y/N] _
```
On `y`: run `rustup toolchain install {toolchain}` (inheriting stderr for
progress), then retry the original pipeline. On `n` or non-TTY: bail with
the existing actionable error message.

**Where to insert the check**: `rustdoc_json.rs::generate_rustdoc_json()` is the
single entry point for JSON generation. The check should happen once at the start,
before the `Command::new("cargo")` call. For batch pre-warming
(`pre_warm_cross_crate_json`), the same check applies since it also invokes
`cargo +{toolchain}`.

**Scope of check**: Only the functions that invoke `cargo +{toolchain}` need the
guard. A shared helper `ensure_toolchain_available(toolchain: &str) -> Result<()>`
keeps the logic in one place, callable from both `generate_rustdoc_json()` and
`pre_warm_cross_crate_json()`.

**TTY detection**: `std::io::stderr().is_terminal()` (stabilized in Rust 1.70,
`IsTerminal` trait). No external crate needed.

**User input**: Read from `/dev/tty` (Unix) / `CONIN$` (Windows) rather than
stdin, since stdin may be piped. Fall back to non-interactive mode if tty open
fails.

**Success criteria**:
- On a machine without nightly, `cargo brief api serde` prompts and installs.
- In CI (no TTY), the error message includes the install command without hanging.
- `cargo brief ts ...` and other nightly-free subcommands work without the check.
- `--toolchain nightly-2026-03-15` is respected in the check.

### Result (bd37ff0) - 26-03-30

Implemented as planned. `ensure_toolchain_available()` added to `rustdoc_json.rs`
with `AtomicBool` guard, TTY detection via `IsTerminal`, and `/dev/tty`/`CONIN$`
input. Call inserted after cache early-return in `generate_rustdoc_json()`.
`batch_generate_rustdoc_json()` intentionally left unchanged — pre-warming failures
are non-fatal and per-crate fallback triggers the prompt via the guard.

No deviations from the plan. Code review found no critical/important issues.
Minor cleanup: deduplicated `read_tty_line` platform blocks using a const path
per `#[cfg]` target. All 320 tests pass unchanged.
