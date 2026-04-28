# Brief: pr3-proc-macro

## Intent
Surface proc-macro items (bang `name!(...)`, attribute `#[name]`, derive `#[derive(Name)]`) across
all five cargo-brief commands: `summary`, `api`, `search`, `cross-crate`, and `code`.
Before this PR, `ItemEnum::ProcMacro` had wildcard fallthrough in every match arm, making
proc-macro crates completely invisible. The fix also corrects a latent `seen_names` dedup
bug where a same-named trait would be blocked once a proc-macro claimed the name.

## Approach
- Add three `ItemKind` / `LeafKind` / `AccessibleItemKind` variants (`ProcMacro`, `ProcAttrMacro`,
  `ProcDeriveMacro`) uniformly across `summary.rs`, `search.rs`, `cross_crate.rs`, `code.rs`.
- Add `render_proc_macro` in `render.rs` producing pseudo-Rust with kind-appropriate leading
  attributes (`#[proc_macro]`, `#[proc_macro_attribute]`, `#[proc_macro_derive(...)]`).
- Wire `--no-macros` to suppress all four macro kinds (macro_rules! + three proc-macro kinds).
- Map proc-macro kinds to `function_item` tree-sitter query in `code.rs` (proc-macros are
  `pub fn` in source; tree-sitter has no dedicated node).
- Fix `seen_names`: do not insert a proc-macro name so same-named traits from other source
  crates (e.g. serde's `Serialize` derive + `Serialize` trait) can still expand.
- New fixture `test_fixture/proc-macro-fixture` with one of each kind.
- 5 integration tests: api, summary counts, no-macros flag, search one-liners, code kinds.

## Constraints
- All four macro kinds must be suppressed by `--no-macros` (no new flags).
- Visibility filtering (`is_visible_from`) must apply to proc-macros identically to other items.
- Output is pseudo-Rust for LLMs; `pub macro` keyword is acceptable even though it is not
  valid Rust — it reads correctly to syntax highlighters.
- `render_proc_macro` must not insert proc-macro names into `seen_names`.
- tree-sitter catch-all must not duplicate `function_item` patterns (proc-macro kinds
  intentionally excluded from the catch-all loop).

## Out of scope
- A separate `--no-proc-macros` flag.
- Cross-crate re-export of proc-macros via `pub use` (tracked as future work).
- Windows LSP support or any LSP changes.

## Details
rustdoc-types 0.57 proc-macro variant:
```rust
ItemEnum::ProcMacro(ProcMacro { kind: MacroKind, helpers: Vec<String> })
// MacroKind: Bang | Attr | Derive
```

Rendered output per kind (api):
```
#[proc_macro]
pub macro name! { /* ... */ }

#[proc_macro_attribute]
pub macro name { /* ... */ }

#[proc_macro_derive(Name, attributes(helper))]
pub macro Name { /* ... */ }
```

Search one-liners:
```
proc_macro    path::name!;
attr_macro    #[path::name];
derive_macro  #[derive(path::Name)];
```

Summary keys: `proc_macros`, `attr_macros`, `derive_macros`.

## References
- `spec/cli-surface.md` — [Must] `--no-macros` flag scope and per-subcommand filtering
- `spec/output-format.md` — [Must] macro rendering contract; proc-macro variants extend it
- `spec/visibility.md` — [Must] visibility semantics apply to proc-macros identically
- `ai-docs/mental-model/search.md` — [Must] search module item inclusion/exclusion patterns
- `ai-docs/mental-model/testing.md` — [Must] fixture conventions and integration test helpers
- `ai-docs/mental-model/remote-pipeline.md` — [Maybe] cross-crate index build and AccessibleItemKind
- `ai-docs/mental-model/glob-expansion.md` — [Maybe] seen_names dedup machinery context
