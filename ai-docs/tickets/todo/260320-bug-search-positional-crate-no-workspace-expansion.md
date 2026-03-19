---
title: "search: positional crate arg doesn't expand workspace members"
status: todo
---

## Bug

When a crate name is given as a positional argument to `cargo brief search`,
only the top-level crate is searched. When the same name is passed via
`--crates`, all workspace member crates are searched correctly.

### Reproduction

```sh
# Only searches top-level `bevy` crate — misses results in bevy_shader etc.
cargo brief search bevy "ShaderRef" --compact
# => crate bevy — search: "ShaderRef" (0 results)

# Correctly searches all workspace members
cargo brief search --crates bevy "ShaderRef" --compact
# => finds 16 results in bevy_shader
```

### Expected

Both invocations should produce the same result — all workspace member crates
of `bevy` should be searched regardless of whether the crate name is passed
positionally or via `--crates`.

### Root Cause (to investigate)

Likely the positional-to-`--crates` normalization path (added in v0.5.0,
commit 38010c3) feeds a single exact crate name into the search pipeline
without triggering workspace member expansion.
