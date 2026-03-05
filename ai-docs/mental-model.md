# cargo-brief — Mental Model

## What This Tool Is

`cargo-brief` is a Cargo subcommand that extracts and formats a Rust crate's API surface as
pseudo-Rust documentation. It is a visibility-aware, context-sensitive extension of
`cargo-public-api`.

Primary consumer: **AI coding agents** (e.g., Claude Code) that need to understand a crate's
interface without reading full source files — saving context window tokens.

Output style: "text-document-like" pseudo-Rust (not machine-readable JSON). Function bodies
are replaced with `;`, doc comments are preserved verbatim, private items are filtered by
perspective.

---

## Core Concept: Visibility-Aware Perspective

The tool's key differentiator from `cargo-public-api` is **`--at-mod`**: rather than dumping
everything that is technically `pub`, it shows only what would compile if `use`d from the
specified module. This includes re-exports and respects `pub(crate)`, `pub(super)`,
`pub(in path)` appropriately.

For **external crates**, `--at-mod` degenerates to "show `pub` items only" (since cross-crate
visibility is always `pub`-only). This makes external dep support architecturally simpler.

---

## CLI Interface (Designed, Not Yet Implemented)

```
cargo brief [OPTIONS] <crate_name> <module_path>
```

### Positional Arguments
| Argument       | Description                                      |
|----------------|--------------------------------------------------|
| `<crate_name>` | Name of the crate to inspect                     |
| `<module_path>`| Module path within that crate to inspect         |

### Options
| Flag                    | Description                                                    |
|-------------------------|----------------------------------------------------------------|
| `--at-package <pkg>`    | Caller's package (for visibility resolution)                   |
| `--at-mod <mod_path>`   | Caller's module (determines what is visible)                   |
| `--depth <n>`           | How many submodule levels to recurse into                      |
| `--recursive`           | Recurse into all submodules (no depth limit)                   |
| `--all`                 | Show all item kinds (equivalent to enabling all item flags)    |
| `--structs`             | Include structs                                                |
| `--enums`               | Include enums                                                  |
| `--traits`              | Include traits                                                 |
| `--functions`           | Include free functions                                         |
| `--aliases`             | Include type aliases                                           |
| `--constants`           | Include constants and statics                                  |
| `--macros`              | Include macros                                                 |
| `--no-structs`          | Exclude structs (override default)                             |
| `--no-enums`            | Exclude enums                                                  |
| `--no-traits`           | Exclude traits                                                 |
| `--no-functions`        | Exclude free functions                                         |
| `--no-aliases`          | Exclude type aliases                                           |
| `--no-constants`        | Exclude constants and statics                                  |
| `--no-macros`           | Exclude macros                                                 |

*(Exact flag names are provisional and can be revised before implementation.)*

### Default Behavior: "Everything Minus Exclusions"

By default, all common item kinds are shown. The `--all` flag additionally includes noisy
categories that are excluded by default. Per-kind `--no-<kind>` flags allow subtractive
filtering from the default set.

**Default exclusions** (shown only with `--all`):
- Blanket impls (`impl<T: Foo> Bar for T`) — detected via `rustdoc-types` `Impl::blanket_impl`
- Auto-trait / synthetic impls (`Send`, `Sync`, `Unpin`, `UnwindSafe`, ...) — detected via `Impl::synthetic`

**Default inclusions** (everything else):
- Structs, enums, unions, traits, free functions, type aliases, constants, statics, macros
- Inherent impls (`impl MyStruct { ... }`)
- Concrete trait impls (`impl Display for MyStruct { ... }`)

---

## Output Format

```rust
// crate my_proj
mod my_group::other_mod {
    mod submod_1 {
        mod submod_2 {
            struct MyStruct {
                pub(crate) visible_field: i32,
                // ... (private fields hidden)
            }
            impl MyStruct {
                /// Doc comment preserved as-is.
                pub fn my_func(arg: ()) -> i32;
            }
        }
    }
}
```

Rules:
- Module hierarchy is preserved as nested `mod` blocks.
- Struct/enum fields: only those visible from `--at-mod` are shown; others replaced with `// ...`.
- Function/method bodies: replaced with `;`.
- Doc comments (`///`, `//!`): dumped verbatim above the item.
- Trait implementations: shown inline in `impl` blocks.
- Blanket impls: omitted by default; shown with `--all`. Detected via `Impl::blanket_impl`.
- Auto-trait impls (Send, Sync, ...): omitted by default; shown with `--all`. Detected via `Impl::synthetic`.

---

## Implementation Architecture

### Backend: rustdoc JSON (Primary)

`cargo rustdoc -p <crate> -- --output-format json --document-private-items`

- Rustdoc JSON is post-macro-expansion, so proc-macro and derive outputs are naturally
  included — matching LSP-equivalent behavior.
- Parsed via the `rustdoc-types` crate (official Rust project, tracks rustdoc JSON format).
- `--document-private-items` is required to obtain full visibility metadata for
  `--at-mod` filtering. Filtering is then applied in our code, not by rustdoc.

### Loose Backend Coupling

The backend (rustdoc JSON) should be isolated behind a trait/abstraction so that alternative
backends (rust-analyzer, syn) can be plugged in if rustdoc JSON proves insufficient for any
case. However, do not over-engineer this abstraction upfront — extract it when a second
backend is actually needed.

### Visibility Resolution

To determine what is visible from `--at-mod my_mod`:
1. Compute the module path of `my_mod` within `--at-package`.
2. For each item in the target module tree:
   - `pub` → always visible
   - `pub(crate)` → visible if `--at-package` == target crate
   - `pub(super)` → visible if `my_mod` is a descendant of the item's parent module
   - `pub(in path)` → visible if `my_mod` is within the specified path
   - Re-exports (`pub use`) → trace through to the original item's visibility
3. For external crates: only `pub` items are ever visible (skip steps 2–3).

### Phase Plan

**Phase 1 — Workspace crates only**
- `cargo rustdoc -p <crate>` invocation
- rustdoc JSON parsing via `rustdoc-types`
- Visibility filtering for workspace crates
- Output formatting (pseudo-Rust text)
- All item-kind flags (`--structs`, `--functions`, etc.) and `--all`
- `--depth` and `--recursive`

**Phase 2 — External dependencies**
- Feature-flag-aware rustdoc invocation (read from `cargo metadata`)
- Caching layer: store `target/doc/<dep>.json`, invalidate on `Cargo.lock` change
- External dep visibility is `pub`-only (simpler filtering)
- Estimated complexity: ~30% additive on top of Phase 1

---

## Current State (as of 2026-03-05)

- **Core pipeline implemented and working**: CLI → rustdoc invocation → JSON parsing → visibility filtering → pseudo-Rust rendering.
- Dependencies: `clap` 4, `rustdoc-types` 0.57, `serde_json` 1, `anyhow` 1.
- Requires nightly toolchain (`cargo +nightly rustdoc -- --output-format json -Z unstable-options --document-private-items`).
- Test fixture (`test_fixture/`) with various visibility levels validates output.
- Module structure: `cli.rs`, `rustdoc_json.rs`, `model.rs`, `render.rs`, `main.rs`.
- Remaining: more tests, `--at-mod` with specific module path, error handling polish.

---

## Key Decisions Made

| Decision | Choice | Rationale |
|---|---|---|
| Primary backend | rustdoc JSON + `rustdoc-types` | Post-macro-expansion, official, matches LSP-level output |
| `--at-mod` semantics | "compiles when `use`d from here" | Matches developer mental model; includes re-exports |
| Output format | Pseudo-Rust text (not JSON) | LLM consumption; readable as documentation |
| Item-kind filtering | Default=show all common items; `--no-*` to exclude; `--all` adds blanket/auto-trait impls | Subtractive model is more ergonomic |
| External deps | Phase 2 | Adds ~30% complexity; architecture supports it cleanly |
