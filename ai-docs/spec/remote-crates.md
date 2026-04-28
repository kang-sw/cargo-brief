---
title: Remote Crates
summary: How cargo-brief fetches and queries crates from crates.io — covering the -C flag, crate spec syntax, version resolution, cache directory structure and TTL semantics, feature selection, nightly toolchain detection, multi-version disambiguation, and cross-crate facade expansion.
---

# Remote Crates

`cargo brief` can query any crate from crates.io without adding it to the local workspace. The `-C` flag activates remote mode: cargo-brief resolves the crate version, creates a workspace in a persistent cache directory, generates rustdoc JSON via `cargo +nightly rustdoc`, and runs the same rendering pipeline used for local crates.

## The `-C` Flag {#260423-remote-mode-flag}

`-C` (long form `--crates`) is a global boolean flag that activates remote mode. When set, the TARGET positional argument is interpreted as a **crate spec** (see [Crate Spec Syntax](#260423-crate-spec-syntax)) rather than a local workspace package name.

```
cargo brief -C api serde
cargo brief -C search tokio@1 spawn
cargo brief -C summary bevy
```

`-C` is a global flag and applies to every subcommand. It must precede the subcommand name when used with `cargo brief -C <subcommand>`.

## Subcommand Compatibility {#260423-subcommand-compatibility}

| Subcommand | `-C` support |
|---|---|
| `api` | Yes |
| `search` | Yes |
| `summary` | Yes |
| `code` | Yes |
| `examples` | Yes — scans source files of the fetched crate |
| `ts` | Yes |
| `features` | Yes — shows the remote crate's feature graph |
| `lsp` | **No** — rejected at startup with an error |
| `clean` | N/A — `clean` manages the cache itself; `-C` is not used |

## Crate Spec Syntax {#260423-crate-spec-syntax}

Three spec forms are accepted:

| Form | Example | Version behavior |
|---|---|---|
| Bare name | `serde` | Latest non-yanked version |
| Major or major.minor pin | `tokio@1`, `tokio@1.0` | Latest matching `^1` / `^1.0` SemVer range |
| Exact pin | `serde@1.0.200` | Exact version — three numeric components trigger `=1.0.200` pinning |

Three-component versions are automatically exact-pinned. One- and two-component versions use SemVer range matching. Yanked versions are never selected.

## Module Path in Crate Spec {#260423-module-path-in-spec}

For `api` and `summary`, a module path may be appended to the crate spec using `::`:

```
cargo brief -C api tokio@1::net
cargo brief -C summary bevy::render
```

The segment before `::` is the crate spec; the segment after is the module path passed to the rendering pipeline. For `search`, the pattern is a separate positional argument — the `::` form is not used.

## Feature Flags {#260423-feature-flags}

Two flags control feature selection for remote crates. Both require `-C`.

`-F <FEATURES>` (long: `--features`) — comma-separated list of Cargo feature names to enable:

```
cargo brief -C api serde -F derive,alloc
```

`--no-default-features` — disables the crate's default features:

```
cargo brief -C api serde --no-default-features -F alloc
```

Feature names are sorted alphabetically when building the cache directory name, so `-F rt,net,macros` and `-F macros,net,rt` resolve to the same cache entry. There is no `--all-features` flag — feature names must be spelled out explicitly.

Feature names are validated against the remote crate's feature graph before invoking `cargo rustdoc`. See [-F pre-validation](cli-surface.md#260423-feature-flag-validation) in the CLI Surface spec for error format and did-you-mean behavior.

## `--no-cache` Flag {#260423-no-cache-flag}

`--no-cache` forces a temporary workspace that is discarded when the process exits. No files persist to the cache directory. Version resolution still runs best-effort for the output header; any resolution failure is silently ignored and the process continues with whatever version cargo selects.

This flag requires `-C`.

## Version Resolution {#260423-version-resolution}

For non-exact specs, cargo-brief resolves the concrete version before creating the workspace:

1. **Exact spec** (`=` prefix or three-component) — no network call; the version is used as-is.
2. **24-hour version cache hit** — reads `$CACHE_DIR/versions/{name}.json`; selects the newest non-yanked version matching the SemVer requirement.
3. **Cache miss or expired** — queries `https://crates.io/api/v1/crates/{name}` and writes the response to the version cache (best-effort; cache write failures are silent).
4. **API failure with stale cache** — uses the stale cached data with a stderr warning: `Warning: using stale version cache for '{name}' (API unavailable: …)`.
5. **No cache and API failure** — fails with: `Cannot resolve version for '{name}': … Try specifying an exact version (e.g., {name}@1.0.0) or check your internet connection.`

Only crates.io is supported. Private registries are not.

## Cache Directory Location {#260423-cache-dir-location}

The cache root is resolved in priority order:

1. `$CARGO_BRIEF_CACHE_DIR` — used verbatim when set.
2. `$XDG_CACHE_HOME/cargo-brief/crates` — used when `XDG_CACHE_HOME` is set.
3. `$HOME/.cache/cargo-brief/crates` — default fallback; uses `/tmp` if `HOME` is unset.

`CARGO_BRIEF_CACHE_DIR` is the standard override for testing and CI environments.

## Cache Directory Naming {#260423-cache-dir-naming}

Each cached workspace occupies one directory under the cache root:

```
$CACHE_DIR/
  versions/
    serde.json                       # crates.io API response, 24-hour TTL
  serde[1.0.217]/                    # workspace — no additional features
  tokio[1.44.1]+macros+net+rt/       # features sorted alphabetically, joined with +
    Cargo.toml                       # exact-pinned dependency (=1.44.1)
    src/lib.rs                       # empty dummy crate
    Cargo.lock                       # written by cargo on first build
    target/doc/tokio.json            # rustdoc JSON output
    target/doc/tokio.bin             # bincode parse cache
```

Directory name format: `{name}[{resolved_version}]` with features appended as `+feat1+feat2` in alphabetical order. Different crate specs that resolve to the same version and feature set share one directory. The workspace `Cargo.toml` pins the version exactly (`={resolved_version}`) to prevent cargo from selecting a different version on subsequent builds.

## Cache Invalidation and TTL {#260423-cache-invalidation-ttl}

| Cache layer | TTL / invalidation |
|---|---|
| Version response (`versions/{name}.json`) | 24 hours from file mtime |
| rustdoc JSON (`target/doc/{name}.json`) | No TTL — reused indefinitely |
| Bincode parse cache (`target/doc/{name}.bin`) | Regenerated when absent or older than the `.json` |

rustdoc JSON is never automatically invalidated when a new crate version is released. The only invalidation paths are `cargo brief clean <name>` (removes the named workspace) or `cargo brief clean` (removes the entire cache). See the `clean` subcommand in [CLI Surface](cli-surface.md#260423-clean-subcommand).

## Nightly Toolchain Detection {#260423-toolchain-detection}

Before the first `cargo rustdoc` call per process, cargo-brief checks whether the required toolchain (default: `nightly`) is installed:

- **TTY mode** — if stderr is a terminal and the toolchain is missing, an interactive prompt (`[y/N]`) offers to run `rustup toolchain install {toolchain}` immediately.
- **Non-TTY mode** — fails with: `The '{toolchain}' toolchain is not installed. Install it with: rustup toolchain install {toolchain}`.
- **`rustup` absent** — fails with: `rustup is not installed. Install it from https://rustup.rs/ then run: rustup toolchain install {toolchain}`.

The check runs at most once per process invocation.

## Multi-Version Disambiguation {#260423-multi-version-disambiguation}

Workspaces with multiple versions of the same crate in their dependency tree (common in facade crates like `bevy`) cause `cargo rustdoc -p {name}` to fail with an ambiguous-specification error. cargo-brief handles this automatically:

1. Detects the ambiguous-specification error in cargo's stderr output.
2. Parses the candidate `name@version` specs from stderr.
3. Selects the highest semver match and retries with the version-qualified spec.

If auto-selection also fails, cargo-brief emits: `Multiple versions of '{name}' exist and auto-resolution failed. Use \`name@version\` to disambiguate.`

## Cross-Crate Facade Expansion {#260423-cross-crate-facade-expansion}

Facade crates (e.g., `bevy`, `axum`, `serde`) re-export items from sub-crates. cargo-brief follows these re-exports automatically for `api`, `search`, and `summary` queries:

1. The primary crate's public API is walked top-down.
2. Both glob (`pub use sub_crate::*`) and named (`pub use sub_crate::Item`) re-exports into external crates are followed.
3. Each reachable item is assigned an **accessible path** reflecting how a user would write the `use` statement through the facade (e.g., `bevy::render::render_resource::AsBindGroup` rather than `bevy_render::render_resource::bind_group::AsBindGroup`).
4. When an item is reachable via multiple paths, the shortest non-prelude path wins.
5. Sub-crate rustdoc JSON files are generated and cached on demand, with cache hits reused transparently.

The re-export follow depth is capped at 8 levels with cycle detection. `--no-expand-glob` disables cross-crate glob expansion. See [Output Format](output-format.md#260423-glob-reexport-rendering) for rendering rules and [Visibility Semantics](visibility.md#260423-cross-crate-depth-guard) for the depth guard.

## Remote Feature Graph {#260423-remote-feature-graph}

When querying a remote crate, cargo-brief fetches the crate's feature graph from the crates.io API alongside the version resolution call. The feature graph is used for two purposes:

1. **Feature-gate annotations** in `api` output — items behind `#[cfg(feature = "...")]` are annotated (see [Feature-Gate Annotations](output-format.md#260423-feature-gate-annotations)).
2. **`-F` pre-validation** — requested feature names are validated before invoking `cargo rustdoc`.

The feature graph is built by merging the `features` and `features2` fields from the crates.io API response. `features2` stores weak-dependency aliases that use the `dep:` prefix syntax; without merging, those aliases would be silently absent. {#260423-feature-graph-offline-fallback}

### Offline Fallback

If the crates.io API call fails (network unavailable, rate-limited, or malformed response), cargo-brief continues without a feature graph:

- Feature-gate annotations are silently omitted from `api` output — no error is raised.
- `-F` pre-validation is skipped — the raw feature list is forwarded to cargo unchanged.
- The `features` subcommand in remote mode exits with an error (a feature graph is its only output).

## Verbose Progress Reporting {#260423-verbose-remote-progress}

With `--verbose`, cargo-brief reports remote pipeline activity to stderr:

- Before a multi-crate `cargo doc` invocation for cross-crate sub-crate discovery: `[cargo-brief] Batch generating rustdoc JSON for N crate(s): crate1, crate2, …`
- `cargo doc` and `cargo rustdoc` stderr is inherited, streaming real-time compilation output.
- Cache hits for previously-generated rustdoc JSON are reported per crate.
