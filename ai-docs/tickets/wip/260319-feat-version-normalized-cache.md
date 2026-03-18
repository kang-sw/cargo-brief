---
title: "Version-normalized cache directories + crates.io version resolution"
status: wip
started: 2026-03-19
---

# Version-normalized cache directories + crates.io version resolution

## Priority: P1

## Motivation

Different spec forms for the same crate resolve to separate cache directories:

```
cargo brief --crates hecs        → CACHE_DIR/hecs/
cargo brief --crates hecs@0.14   → CACHE_DIR/hecs@0.14/
cargo brief --crates hecs@0.14.2 → CACHE_DIR/hecs@0.14.2/
```

All three may resolve to hecs 0.14.2 but each creates its own workspace
(several GB for large crates like bevy). Additionally, bare specs (`hecs`)
are permanently pinned by `Cargo.lock` — the user never gets a newer version
unless they manually `--clean`.

AI agents call `cargo brief --crates` repeatedly with varying spec forms
in short sessions. Cache deduplication directly reduces disk usage and
generation time.

## Design

### Cache directory naming

```
CACHE_DIR/<name>[<resolved_version>]              # no features
CACHE_DIR/<name>[<resolved_version>]+feat1+feat2  # with features, alpha-sorted
```

Examples:
```
CACHE_DIR/hecs[0.14.2]/
CACHE_DIR/bevy[0.18.1]+bevy_winit+default/
CACHE_DIR/tokio[1.44.1]+rt+net+macros/
```

### Version resolution via crates.io API

Before creating or reusing a workspace, resolve the exact version by calling
the crates.io REST API:

```
GET https://crates.io/api/v1/crates/{name}
```

The response `versions` array (newest-first) is filtered with
`semver::VersionReq` matching against the user's spec:

| Spec | version_req (from `parse_crate_spec`) | semver match |
|---|---|---|
| `hecs` | `*` | any non-yanked |
| `tokio@1` | `1` | `>=1.0.0, <2.0.0` |
| `bevy@0.18` | `0.18` | `>=0.18.0, <0.19.0` |
| `serde@1.0.200` | `=1.0.200` | exact |

First match = resolved version. Cache dir is then deterministic.

### API response caching

Store raw API responses at `CACHE_DIR/versions/<crate_name>.json`.
Reuse if file mtime is less than 24 hours old. Benefits:

- Repeated calls with different version_req for the same crate: 0 network calls
- Offline fallback: stale cache is better than no cache
- `--clean` removes `versions/` directory too
- `--no-cache` bypasses version cache as well

```rust
fn fetch_resolved_version(name: &str, version_req: &str) -> Result<String> {
    let cache_path = cache_dir().join("versions").join(format!("{name}.json"));

    // Reuse if < 24h old
    if let Ok(meta) = cache_path.metadata() {
        if meta.modified()?.elapsed()? < Duration::from_secs(86400) {
            let cached = std::fs::read_to_string(&cache_path)?;
            return find_matching_version(&cached, version_req);
        }
    }

    // API call + cache
    let resp = ureq::get(&format!("https://crates.io/api/v1/crates/{name}"))
        .set("User-Agent", "cargo-brief")
        .call()?.into_string()?;
    let _ = std::fs::create_dir_all(cache_path.parent().unwrap());
    let _ = std::fs::write(&cache_path, &resp);
    find_matching_version(&resp, version_req)
}
```

### Updated `resolve_workspace` flow

```
resolve_workspace(spec, features, no_cache):
  (name, version_req) = parse_crate_spec(spec)
  if no_cache → TempDir (unchanged)

  version = fetch_resolved_version(name, version_req)?
  dir_name = format!("{name}[{version}]{features_suffix}")
  dir = cache_dir().join(dir_name)

  if dir exists → reuse
  else → create_dir_all, write_workspace_files
  return Cached(dir)
```

No two-phase rename needed — version is known before workspace creation.

### `--clean` glob matching

```
--clean hecs       → glob CACHE_DIR/hecs[*]* → delete all matching
--clean hecs@0.14  → resolve version first, delete exact dir
--clean            → delete entire CACHE_DIR (unchanged)
```

### Offline / API failure fallback

1. If API response cache exists (even stale): use it
2. If no cache at all: fall back to `cargo generate-lockfile` in a temp
   workspace, read Cargo.lock for version, then proceed
3. If both fail: error with actionable message

### Stale detection for bare specs

With the 24h API cache, bare specs (`hecs`) automatically pick up new
versions after the cache expires. No manual `--clean` needed for version
bumps (unless the user wants it sooner).

## Dependencies

```toml
semver = "1"
ureq = "2"
```

`ureq` is a minimal blocking HTTP client (~100KB). `semver` is Cargo's own
version matching library.

## Files Modified

| File | Changes |
|---|---|
| `Cargo.toml` | Add `semver`, `ureq` |
| `src/remote.rs` | `fetch_resolved_version()`, `cache_dir_name()`, refactor `resolve_workspace()`, update `clean_cache()` |
| `src/lib.rs` | Remove `build_remote_crate_header` version logic (version now known from spec resolution) |
| `tests/` | Update cache-related tests |

## Complexity

~80-100 lines of new/changed code. Core logic:
- `fetch_resolved_version()`: ~30 lines (API call + semver match + file cache)
- `cache_dir_name()`: ~10 lines (format name + version + sorted features)
- `resolve_workspace()` changes: ~20 lines
- `clean_cache()` glob matching: ~10 lines
- Offline fallback: ~10 lines

## Testing

- Unit: `fetch_resolved_version` with mocked JSON response, semver matching edge cases
- Unit: `cache_dir_name` formatting with various feature combos
- Integration: `--clean hecs` glob matching against versioned dirs
- Manual: `cargo brief --crates tokio@1` twice → same dir, `cargo brief --crates tokio` → same dir if version matches

### Result - 26-03-19

Implemented as designed. Key changes:

- `src/remote.rs`: Added `cache_dir_name()`, `find_matching_version()`, `fetch_resolved_version()` with 24h file cache + offline fallback. Removed `sanitize_spec()`. Refactored `resolve_workspace()` to return `(WorkspaceDir, Option<String>)`. Refactored `clean_cache()` to glob-match on `name[` prefix.
- `src/lib.rs`: Updated `run_remote_pipeline()` to destructure tuple return. `build_remote_crate_header()` now accepts `resolved_version: Option<&str>` with Cargo.lock fallback for `--no-cache`.
- New deps: `semver = "1"`, `ureq = "2"`.
- 11 new unit tests for `cache_dir_name`, `find_matching_version`, `fetch_resolved_version` exact shortcut.
- Deviation: ureq 2.x uses `.set()` not `.header()` for setting headers — discovered during build.
- Deviation: Offline fallback omits `cargo generate-lockfile` approach from ticket (simpler: stale cache → error). The stale-cache fallback covers the primary offline scenario.
