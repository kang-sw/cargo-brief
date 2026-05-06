---
title: cfg_parse: handle implicit-AND CfgTrace and suppress raw paths in fallback
spec:
  - 260423-feature-gate-annotations
---

# cfg_parse: handle implicit-AND CfgTrace and suppress raw paths in fallback

## Background

`cfg_parse.rs` reconstructs `#[cfg(...)]` attribute strings from the `CfgTrace` data
emitted by nightly rustdoc JSON. When parsing fails, it falls back to a raw comment:

```
// cfg: #[attr = CfgTrace([...])]
```

During usability testing against `tokio` with `-F full`, 3 fallback occurrences were
found in the output. Each case involves a top-level CfgTrace array with multiple
predicates — an implicit AND — combined with a `Not(loom)` non-feature predicate:

```
CfgTrace([
  NameValue { name: "feature", value: Some("fs"), span: ... },
  Not(NameValue { name: "loom", value: None, span: ... }, ...)
])
```

The parser does not handle the top-level implicit AND form (an array with more than one
predicate). As a result, `all(feature = "fs", not(loom))` fails to reconstruct and the
raw CfgTrace spills into the output — multiline and including absolute local paths from
the rustdoc compilation cache (e.g. `/Users/kang-sw/.cargo/registry/...`).

This pollutes the output with internal noise that:
1. Spans multiple lines, breaking the single-comment expectation
2. Leaks machine-specific absolute paths into what should be portable output

## Decisions

**Fix strategy: extend the parser to handle implicit-AND, not just sanitize the fallback.**

A sanitized fallback (truncate to one line, strip paths) would reduce visible noise but
leave the fundamental information loss. Extending the parser is the correct fix:

- A top-level CfgTrace array `[pred, pred, ...]` is an implicit `All` — treat it as
  `All([pred, pred, ...])` and reconstruct as `#[cfg(all(...))]`.
- `Not(NameValue { name: "loom", ... })` is a `NonFeature` predicate. The parser already
  has a `NonFeature` variant — it should be reconstructed as `not(loom)`.

Both changes make the three tokio cases render as proper `#[cfg(all(feature = "fs", not(loom)))]`
lines instead of falling back.

**Fallback hardening (secondary):** Even after the parser improvements, edge cases may still
hit the fallback. The fallback emission should be hardened to:
1. Strip newlines from the raw string (make it single-line)
2. Redact absolute paths (replace `/path/to/.cargo/...` with `...`)

This ensures any remaining fallback cases are at most a cosmetic nuisance, not a
machine-path leak.

## Phases

### Phase 1: Extend cfg_parse implicit-AND and harden fallback

**Goals:**
- Parse top-level CfgTrace arrays as implicit `All` predicates
- Parse `Not(NameValue { name: X, value: None, ... })` as `not(X)` using existing `NonFeature`
- In the fallback emit path: strip newlines, redact absolute paths

**Success criteria:**
- `cargo brief -C -F full api tokio@1` produces zero `// cfg:` fallback lines
- Existing cfg_parse unit tests still pass
- New unit tests cover: implicit-AND, Not(NonFeature), mixed feature+non-feature all()

**Suggested approach:**

In `parse_cfg_attribute`, after extracting the inner predicate list:
- If the outer wrapper is a list (array), map each element and wrap in `CfgPredicate::All`
- If a `Not` contains a `NameValue` with `value: None`, reconstruct as `NonFeature`

In the fallback emit path (render.rs), sanitize the raw string before writing:
- Replace `\n` with ` `
- Replace any substring matching an absolute path pattern (`/.*?\.cargo/.*?`) with `<path>`
