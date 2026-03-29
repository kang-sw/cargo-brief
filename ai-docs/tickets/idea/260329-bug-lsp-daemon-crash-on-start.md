---
title: "lsp: daemon dies silently within seconds of spawn"
status: idea
reported: 2026-03-29
---

# lsp: daemon dies silently within seconds of spawn

## Problem

The LSP daemon starts but dies within seconds. `lsp status` reports
"not running" immediately after `lsp touch` succeeds.

**Reproduction:**

```sh
cargo brief lsp --verbose touch
# [lsp] spawning daemon for /Users/.../gunpowder-odyssey
# [lsp] daemon running (PID 48116, ra: initializing, uptime: 0s)

sleep 10
cargo brief lsp status
# LSP daemon: not running
```

Repeated attempts show the same behavior — daemon spawns, reports
"initializing", then disappears. No error message is surfaced to the user.

## Environment

- cargo-brief v0.9.2
- macOS Darwin 25.3.0
- Workspace: virtual workspace with ~6 crates (gunpowder-odyssey)
- rust-analyzer available on PATH

## Notes

- The previous diagnostics ticket (260326-bug-lsp-daemon-spawn-diagnostics)
  addressed timeout detection, but this case is different: `touch` returns
  success, yet the daemon dies shortly after.
- Possible causes: rust-analyzer crash during workspace loading, socket
  cleanup race, or sandbox-related file access failures.
- No stderr output is visible — daemon stderr may still be discarded or
  logged to an inaccessible location.

## Severity

High — `lsp` subcommand is entirely unusable until this is resolved.
`code --refs` serves as a grep-based fallback for reference tracking.
