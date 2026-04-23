---
title: "search: glob patterns fail to match short names due to full-path anchoring"
status: idea
reported: 2026-03-29
---

# search: glob patterns fail to match short names due to full-path anchoring

## Problem

Glob patterns like `Camera*` and `Sprite*` return 0 results, while the
equivalent substring search `Camera` / `Sprite` works fine.

**Reproduction:**

```sh
# 0 results — glob
cargo brief -C search bevy@0.15.3 "Camera*" --search-kind struct
cargo brief -C search bevy@0.15.3 "Sprite*" --search-kind struct

# Works — substring
cargo brief -C search bevy@0.15.3 Camera --search-kind struct   # 24 results
cargo brief -C search bevy@0.15.3 Sprite --search-kind struct   # 10+ results
```

## Analysis

The `--help` documents glob as:

> `w*ld` — glob — * matches 0+ chars, ? matches 1 char (full-path anchored)

"Full-path anchored" means `Camera*` is matched against the entire path
(e.g., `render::camera::Camera`), so it fails because there's no leading
wildcard. Users would have to write `*Camera*` — but that's functionally
identical to substring matching, making glob useless for the most common
use case: prefix matching on the item name.

## Expected behavior

Glob should match against the **final path segment** (item name), not the
full module path. This would make:

- `Camera*` match `Camera`, `Camera2d`, `Camera3d`, `CameraPlugin`, etc.
- `*Material` match `StandardMaterial`, `UiMaterial`, etc.
- `Mesh*` match `Mesh`, `Mesh2d`, `Mesh3d`, `MeshMaterial3d`, etc.

Full-path glob matching could remain available via an explicit `::` prefix
or a `--full-path` flag, but the default should target the item name.

## Severity

Medium — workaround exists (substring), but the current behavior contradicts
user expectations and makes glob a dead feature for practical use.
