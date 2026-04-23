---
title: Remote Crate Support
summary: >
  Fetching, caching, and querying crates.io packages via the -C flag.
  Covers crate spec syntax, version resolution, feature flags,
  cache management, and cross-crate path discovery for facade crates.

features:
  - The `-C` Flag
    - Subcommand Compatibility
  - Crate Spec Syntax
    - Module Path in Crate Specs
  - Version Resolution
    - 24-Hour API Cache
    - Offline Fallback
    - Ambiguous Version Resolution
  - Feature Flags
    - `--no-default-features`
  - Cache Management
    - Cache Location
    - Cache Directory Naming
    - Cache Contents
    - `cargo brief clean [SPEC]`
    - `--no-cache`
  - Cross-Crate Accessible Paths
    - How It Works
    - Batch Pre-Warming
    - Path Deduplication
    - Affected Subcommands
---

# Remote Crate Support

`cargo brief -C` switches the tool into remote mode, where the TARGET positional
argument is interpreted as a crates.io package spec instead of a local workspace
crate. A temporary or cached workspace is created with the requested crate as a
dependency, and then the normal pipeline runs against it.

## The `-C` Flag

`-C` (long form `--crates`) is a boolean global flag on `cargo brief`. It does
not carry a value -- the crate spec comes from the TARGET positional argument of
each subcommand.

```
cargo brief -C api serde@1
cargo brief -C search tokio@1 spawn
cargo brief -C summary bevy@0.15
```

When `-C` is active, TARGET resolution skips local workspace lookup entirely.
The `self` and file-path syntaxes (`src/foo.rs`) are not valid in remote mode.

### Subcommand Compatibility

| Subcommand | `-C` supported | Notes |
|------------|:-:|---|
| `api`      | yes | Full pipeline including cross-crate resolution |
| `search`   | yes | Full pipeline including cross-crate resolution |
| `summary`  | yes | Full pipeline including cross-crate resolution |
| `examples` | yes | Disk-only pipeline -- reads source files directly, no rustdoc JSON |
| `ts`       | yes | Disk-only pipeline -- runs tree-sitter queries on source files |
| `code`     | yes | Supports dep recursion into accessible dependencies |
| `clean`    | n/a | Manages remote crate cache; does not take `-C` |
| `lsp`      | no  | Rejects `-C` with an error |

## Crate Spec Syntax

The TARGET positional doubles as the crate spec when `-C` is active. Three
forms are supported:

| Form | Example | Version requirement | Resolution |
|------|---------|---------------------|------------|
| Bare name | `serde` | `*` (latest non-yanked) | Resolved via crates.io API |
| Partial version | `serde@1`, `tokio@1.0` | Semver range (`>=1.0.0, <2.0.0`) | Resolved via crates.io API |
| Exact version | `serde@1.0.200` | Exact pin (`=1.0.200`) | No network call needed |

A version string with fewer than two dots is treated as a semver range (e.g.,
`@1` matches `>=1.0.0, <2.0.0`; `@1.0` matches `>=1.0.0, <1.1.0`). A version
with two or more dots is an exact pin.

### Module Path in Crate Specs

A module path can be appended to the crate spec with `::`:

```
cargo brief -C api tokio@1::net       # browse tokio's net module
cargo brief -C api bevy@0.15::ecs     # browse bevy's ecs module (cross-crate)
```

Alternatively, the module path can be passed as a separate positional argument:

```
cargo brief -C api tokio@1 net
cargo brief -C -F net api tokio@1 net
```

## Version Resolution

When the crate spec is not an exact pin, cargo-brief queries the crates.io REST
API (`/api/v1/crates/{name}`) to find the newest non-yanked version matching the
requirement.

### 24-Hour API Cache

API responses are cached at `<cache-root>/versions/{name}.json`. A cached
response younger than 24 hours is used without contacting crates.io. This means:

- Bare specs (`serde`) and partial specs (`serde@1`) may serve a version that is
  up to 24 hours stale.
- Exact specs (`serde@1.0.200`) skip the API entirely -- no network call, no
  cache TTL concern.
- Running `cargo brief clean serde` removes the version cache for that crate,
  forcing a fresh API call on the next invocation.

### Offline Fallback

If the crates.io API is unreachable, a stale cache (older than 24 hours) is used
with a warning on stderr. If no cache exists and the network fails, the command
errors with a suggestion to specify an exact version.

### Ambiguous Version Resolution

When multiple versions of a dependency exist in a workspace's `Cargo.lock`
(e.g., `hashbrown 0.14` and `hashbrown 0.15`), cargo-brief auto-picks the
highest semver version. If cargo itself reports a "specification is ambiguous"
error during rustdoc JSON generation, the tool retries with a version-qualified
spec (`name@version`).

## Feature Flags

`-F` / `--features` enables specific crate features. It requires `-C`.

```
cargo brief -C -F rt,net,io-util api tokio@1
cargo brief -C -F derive api serde@1
```

Features are comma-separated. They are passed to the generated workspace's
`Cargo.toml` as `features = ["rt", "net", "io-util"]`.

### `--no-default-features`

Disables the crate's default feature set. Requires `-C`. Can be combined with
`-F` to enable only specific features:

```
cargo brief -C --no-default-features -F derive api serde@1
```

Feature-gated items are invisible in the output unless the appropriate features
are enabled. If an expected item is missing, try adding `-F full` or the
relevant feature name.

## Cache Management

### Cache Location

Resolved workspaces are stored on disk so that subsequent invocations reuse
build artifacts. The cache root is determined by:

1. `$CARGO_BRIEF_CACHE_DIR` (if set)
2. `$XDG_CACHE_HOME/cargo-brief/crates/` (if `$XDG_CACHE_HOME` is set)
3. `$HOME/.cache/cargo-brief/crates/` (default)

### Cache Directory Naming

Each cached workspace uses a version-normalized directory name:

```
<cache-root>/serde[1.0.200]/
<cache-root>/tokio[1.44.1]+macros+net+rt/
<cache-root>/bevy[0.15.1]+bevy_winit+default/
```

Format: `name[version]` with optional `+feature` suffixes (alphabetically
sorted). Changing the version or feature set produces a new directory --
different specs never collide.

### Cache Contents

Each cached directory contains:
- `Cargo.toml` -- generated workspace manifest with the crate as a dependency
- `src/lib.rs` -- empty placeholder
- `Cargo.lock` -- generated by cargo on first build
- `target/` -- cargo build artifacts, including rustdoc JSON output

### `cargo brief clean [SPEC]`

Manages cached workspaces.

```
cargo brief clean            # remove entire cache directory
cargo brief clean serde      # remove all serde versions + version cache
cargo brief clean tokio      # remove all tokio versions + version cache
```

When a SPEC is given, all directories matching the crate name prefix are removed
(e.g., `clean serde` removes `serde[1.0.200]`, `serde[1.0.228]`, etc.). The
version API cache (`versions/{name}.json`) is also removed, forcing a fresh
lookup on the next invocation.

When no SPEC is given, the entire cache root is removed.

Removed paths and their sizes (in MB) are printed to stderr.

### `--no-cache`

The `--no-cache` flag (requires `-C`) uses a temporary directory instead of the
persistent cache. The workspace is cleaned up when the command finishes.

Version resolution is best-effort in `--no-cache` mode -- if the API call fails,
the command proceeds with whatever version cargo resolves, rather than erroring
immediately.

## Cross-Crate Accessible Paths

Facade crates like `bevy` and `axum` re-export items from internal sub-crates.
Without cross-crate resolution, users would see raw internal paths like
`bevy_render::render_resource::bind_group::AsBindGroup`. With it, items appear
under their user-facing paths: `render::render_resource::AsBindGroup`.

### How It Works

When cargo-brief detects that a crate's root module has public re-exports
pointing to external sub-crates (glob `pub use bevy_internal::*` or named
`pub use bevy_ecs as ecs`), it automatically:

1. Generates rustdoc JSON for each referenced sub-crate
2. Walks the re-export tree top-down, tracking the accessible path at each level
3. Builds a unified index where each item has its shortest non-prelude path

This runs automatically for both local and remote crates -- no user flag is
needed.

### Batch Pre-Warming

For crates with many sub-crate dependencies (e.g., bevy has dozens), cargo-brief
batches rustdoc JSON generation. Instead of running `cargo rustdoc` once per
sub-crate, it runs `cargo doc -p a -p b -p c ...` in a single invocation per
BFS level (max depth 8). This is significantly faster for large facade crates.

Sub-crate names are validated against the workspace's `Cargo.lock` before being
passed to cargo. Crates with multiple versions in the lockfile are
disambiguated with version-qualified specs.

Individual sub-crate generation remains as a fallback if batch generation fails
for any package.

### Path Deduplication

When the same item is reachable through multiple paths (e.g., via the prelude
and via the module tree), the shortest non-prelude path is kept. Prelude paths
are deprioritized -- a path through `ecs::system::Query` is preferred over
`prelude::Query`.

### Affected Subcommands

Cross-crate path resolution applies to `api`, `search`, and `summary`. The
`code` subcommand uses a separate BFS (`discover_accessible_deps`) to find
accessible dependency source directories. The `examples` and `ts` subcommands
operate on source files only and do not use the cross-crate index.
