---
title: "LLM Output Polish — Compact Modes & Contextual Hints"
status: todo
---

# LLM Output Polish — Compact Modes & Contextual Hints

## Goal

Maximize information density for LLM consumers. Large crates (tokio, bevy)
currently produce output that exceeds practical context windows. These five
changes reduce token cost and surface "next action" hints so the LLM can
self-navigate without reading source code.

## Motivation

LLM agents use `cargo brief` as their primary API discovery tool. Three pain
points remain after search mode:

1. **Too verbose** — doc comments and full field listings dominate output for
   large crates. Agents often only need signatures.
2. **No "what can I do with this type?"** — search finds a struct, but the
   agent must issue a second call to discover its methods/trait impls.
3. **Feature blindness** — when inspecting remote crates, the agent can't tell
   which features are active or available, leading to confusion when expected
   items are missing.

---

## Phase 1: `--no-docs` flag

Strip all `///` doc comment lines from output. Simplest win — reduces output
30–50% on doc-heavy crates.

**Changes:**
- `src/cli.rs` — add `--no-docs` flag (Filtering group)
- `src/render.rs` — skip doc comment emission when flag is set
- `src/search.rs` — skip doc comment in `render_leaf` when flag is set
- `tests/integration.rs` — test that `--no-docs` suppresses doc lines
- All test files with `BriefArgs` struct literals — add `no_docs: false`

**Estimated scope:** ~20 lines changed.

---

## Phase 2: `--compact` mode

Collapse struct fields, enum variant internals, and trait method bodies into
minimal one-liners. Goal: entire crate API in <200 lines for mid-size crates.

**Rendering rules when `--compact` is active:**
- Structs: `struct Name<G> { .. }` (always collapsed, even named fields)
- Enums: variants as name-only list `enum E { A, B(T), C { .. } }`
  on a single line if ≤120 chars, otherwise one variant per line
- Traits: show method signatures but no doc comments, no default bodies
- Impl blocks: `impl Trait for Type { .. }` (always one-liner)
- Functions: signature only (already the case), no docs
- `--compact` implies `--no-docs`

**Changes:**
- `src/cli.rs` — add `--compact` flag
- `src/render.rs` — compact rendering paths for struct, enum, trait, impl
- `tests/integration.rs` — test compact output shape

---

## Phase 3: Search impl summary for type matches

When `--search` matches a struct/enum/union, append an inline comment
summarizing its impl blocks.

**Before:**
```
struct outer::PubStruct { .. };
```

**After:**
```
struct outer::PubStruct { .. };  // impl (3 methods), impl MyTrait, impl Converter
```

**Changes:**
- `src/search.rs` — after pushing a struct/enum/union leaf, collect impl
  summary from the model and attach as a `LeafContext` field or inline
  during rendering
- `tests/integration.rs` — test impl summary appears

**Design notes:**
- Inherent impl: show method count → `impl (N methods)`
- Trait impl: show trait name → `impl TraitName`
- Keep it to one line; if >5 impls, truncate with `+ N more`

---

## Phase 4: `--methods-of <TYPE>` shorthand

Equivalent to `--search "TypeName" --no-structs --no-enums --no-traits
--no-unions --no-constants --no-macros --no-aliases` — shows only methods
and associated items for a given type.

**Changes:**
- `src/cli.rs` — add `--methods-of <TYPE>` (Search group)
- `src/lib.rs` / `src/search.rs` — translate to search + filter flags
- `tests/integration.rs` — test methods-of output

**Open question:** Should this also show fields? Probably yes for struct
fields since they're the type's data contract. Could add `--no-fields` or
just include them.

---

## Phase 5: Feature flags summary in header

When inspecting a crate (especially via `--crates`), show active and
available features in the crate header comment.

**Output:**
```
// crate tokio [features: rt, macros | available: rt, net, io-util, sync, fs, signal, ...]
```

**Changes:**
- `src/resolve.rs` or `src/lib.rs` — extract `[features]` from
  `Cargo.toml` of the target crate (via cargo metadata or direct parse)
- `src/render.rs` / `src/search.rs` — prepend feature info to header line
- `tests/integration.rs` — test feature header on test_fixture

**Design notes:**
- For local crates: show `[features]` from Cargo.toml
- For `--crates`: show which features were requested + full available list
- If no features defined, omit the section entirely

---

## Files to modify (cumulative)

- `src/cli.rs` — `--no-docs`, `--compact`, `--methods-of`
- `src/render.rs` — no-docs suppression, compact rendering
- `src/search.rs` — no-docs suppression, impl summary, methods-of logic
- `src/lib.rs` — methods-of translation, feature header
- `src/resolve.rs` — feature extraction (Phase 5)
- `tests/integration.rs` — tests for each phase
- 4 other test files — new `BriefArgs` fields

## Verification (per phase)

1. `cargo test` — all tests pass
2. `cargo clippy` — no new warnings
3. Manual: `cargo brief --crates serde --compact` — output under 100 lines
4. Manual: `cargo brief self --search "CrateModel"` — impl summary visible

---

### Result (804cfc7) — Phases 1–4

**Implemented:**
- `--no-docs`: suppresses all `///` doc comments in both normal and search output
- `--compact`: implies no_docs, collapses struct fields `{ .. }`, enum variants
  name-only (one-line if ≤120 chars), traits `{ .. }`, inherent impls `{ .. }`
- Search impl summary: struct/enum/union search hits show inline `// impl (N methods), impl Trait1` comment
- `--methods-of <TYPE>`: translates to `--search TYPE` + exclusion flags, showing methods/fields/variants

**Deviations from plan:**
- `args` threading required `#[allow(clippy::too_many_arguments)]` on `render_item` and `render_struct`
- Walker modified to always walk struct fields, enum variants, and trait items even when the parent
  container is excluded by `--no-structs`/`--no-enums`/`--no-traits`, since `--methods-of` needs them
- `format_path_pub` wrapper added following existing `_pub` pattern

**Phase 5 deferred:** requires cargo metadata changes for feature extraction.
