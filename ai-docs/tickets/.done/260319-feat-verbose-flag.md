---
title: "Add --verbose flag for pipeline progress output"
status: done
started: 2026-03-19
completed: 2026-03-19
---

## Summary

Add a `--verbose` (`-v`) flag that prints pipeline progress to stderr, so users
can see what `cargo brief` is doing during long-running operations.

## Approach

Option A: simple `--verbose` flag + `eprintln!` to stderr.

1. Add `--verbose` / `-v` to `BriefArgs` in `cli.rs`.
2. Thread the flag through `run_pipeline()` and into subsystems that perform
   slow operations.
3. Print progress messages via `eprintln!` at these points:
   - Target resolution: `"Resolving target: <name>..."`
   - rustdoc JSON generation: `"Running cargo rustdoc for <crate>..."`
   - Cache hit: `"Loading cached rustdoc JSON from <path>..."`
   - crates.io fetch: `"Fetching version info from crates.io..."`
   - Cross-crate re-export following: `"Following cross-crate re-export: <path> → <dep>..."`
   - Workspace creation (remote): `"Creating temp workspace for <spec>..."`
4. stderr keeps stdout clean for piping / AI agent consumption.

## Design Notes

- No new dependencies. `eprintln!` is sufficient for this project's scale.
- `log` crate considered but rejected — overkill for a CLI tool primarily
  consumed by AI agents.
- Callback/event pattern rejected — only one consumer (CLI `main.rs`).
- Threading approach: pass `verbose: bool` as a field alongside existing args,
  or add it to `BriefArgs` directly (already threaded everywhere).

## Acceptance Criteria

- `cargo brief self -v` prints progress lines to stderr, result to stdout.
- `cargo brief --crates serde -v` shows fetch/build/cache status on stderr.
- Without `-v`, behavior is unchanged (no stderr output on success).
- Integration tests remain green (they capture stdout only).

### Result - 26-03-19

Implemented Option A: `--verbose` / `-v` flag with `eprintln!` to stderr.

Progress messages at 6 points in both local and remote pipelines:
- Target resolution, cargo rustdoc invocation, JSON parsing (local pipeline)
- Workspace resolution, cargo rustdoc invocation, JSON parsing, cross-crate discovery (remote pipeline)
- Cross-crate module resolution fallback

All 105 integration tests pass. No new dependencies.
