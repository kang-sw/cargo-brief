---
title: "--summary / TOC mode for large crates"
status: idea
---

# Idea: `--summary` / TOC mode for large crates

## Priority: P2

## Motivation

Large crates produce thousands of lines with no progressive disclosure:
- axum: 2,220 lines (default), 438 lines (compact)
- tokio: 1,123 lines
- http: 2,324 lines

An LLM exploring an unfamiliar crate needs a 20-30 line overview first,
then targeted drill-down. Currently the only option is reading the full
output or using `--search` (which requires knowing what to search for).

## Ideas

### A. `--summary` flag

Ultra-compact TOC: module names + item counts + top re-exports.

```
// crate tokio (6 modules, 5 macros)
mod io       // 4 traits, 15 structs, 8 fns
mod net      // 5 structs (TcpStream, TcpListener, UdpSocket, ...)
mod runtime  // 3 structs (Runtime, Builder, Handle)
mod sync     // 8 structs (Mutex, RwLock, Semaphore, Notify, ...)
mod task     // 3 fns (spawn, spawn_blocking, spawn_local), 2 structs
mod time     // 3 structs (Instant, Interval, Sleep), 3 fns
// top-level: spawn, pin!, select!, join!, try_join!, task_local!
```

### B. Token budget mode `--max-tokens N`

Estimate output tokens and auto-truncate with a `// ... N more items` message.
Useful when the LLM agent has a fixed context budget.

### C. Smarter default depth

When output exceeds N lines, auto-suggest `--compact` or `--search`:
```
// crate axum (2,220 lines) — use --compact (438 lines) or --search <pattern>
```

## Complexity

- A: Medium — requires counting items per module without rendering them
- B: Medium — token estimation + truncation logic
- C: Low — just a comment in the output header
