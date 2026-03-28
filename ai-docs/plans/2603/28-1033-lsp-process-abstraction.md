# LSP Process Management Abstraction (Phase 2)

## Context

Ticket `260328-refactor-lsp-cross-platform-ipc`, Phase 2: extract process
management into `src/lsp/process/` with Unix and Windows backends.

Per `_memory.md`, Phase 2 (process mgmt, simpler) runs first because it has
fewer touch points and no entanglement with the FIFO-based IPC layer.

**Goal:** Move `process_alive()`, daemon process spawning (`process_group(0)`),
and the `which`-based binary discovery out of `client.rs`/`daemon.rs` into a
platform-abstracted `process` module. Unix behavior must be unchanged (pure
refactor). Windows backend compiles but is not runtime-tested.

**Cross-compilation check:** Deferred to Phase 3. Since the `lsp` module is
`#[cfg(unix)]`, `cargo check --target x86_64-pc-windows-msvc` won't compile
`process/windows.rs` at all (parent module gated out). For this phase, verify
with `cargo test` only.

## Relevant Files

- `src/lsp/client.rs` — contains `process_alive()` (L318-324), `spawn_daemon()` (L224-253) with `process_group(0)` (L248)
- `src/lsp/daemon.rs` — contains `discover_ra_binary()` (L57-85) with `Command::new("which")` fallback (L71-78)
- `src/lsp/mod.rs` — module declarations; will add `mod process`
- `Cargo.toml` — may need `[target.'cfg(windows)'.dependencies]` for `windows-sys` (for `OpenProcess`)

## Conventions (verified from code)

- LSP submodules are declared in `src/lsp/mod.rs` with visibility matching their use: `pub(crate)` for `client`, `pub` for `daemon`, private for internal (`protocol`, `query`, `transport`, `watcher`).
- Platform-specific imports use `std::os::unix::*` traits at file top level (not behind `#[cfg]` within functions), except `spawn_daemon` which has `use std::os::unix::process::CommandExt` inside the function body.
- Error handling: `anyhow::Result` with `.context()` at every fallible call. `bail!()` for hard errors.
- Functions extracted from one file to another module use `pub(super)` visibility when only consumed by sibling modules within `lsp/`.

## Implementation Steps

1. **Create `src/lsp/process/mod.rs`** — entry file with doc comment, `#[cfg]`-gated submodule declarations, and re-exports.
   - Delegation: main

   ```rust
   //! Platform-abstracted process management for the LSP daemon.

   #[cfg(unix)]
   mod unix;
   #[cfg(windows)]
   mod windows;

   #[cfg(unix)]
   pub(super) use unix::{configure_daemon_spawn, find_binary_on_path, process_alive};
   #[cfg(windows)]
   pub(super) use windows::{configure_daemon_spawn, find_binary_on_path, process_alive};
   ```

2. **Create `src/lsp/process/unix.rs`** — extract three functions from existing code.
   - Delegation: main

   Required imports: `use std::path::PathBuf;`, `use std::process::Command;`,
   `use std::os::unix::process::CommandExt;`.

   Functions to extract:
   - `process_alive(pid: u32) -> bool` — move verbatim from `client.rs:318-324`
   - `configure_daemon_spawn(cmd: &mut Command)` — wraps the `process_group(0)` call (from `client.rs:248`). Takes `&mut Command`, adds `.process_group(0)`. Returns `()` (modifies in place).
   - `find_binary_on_path(name: &str) -> Option<PathBuf>` — extract the `which` fallback from `daemon.rs:71-78`. Returns `None` if not found (caller handles the error message).

3. **Create `src/lsp/process/windows.rs`** — implement Windows equivalents.
   - Delegation: main

   Required imports: `use std::path::PathBuf;`, `use std::process::Command;`,
   `use std::os::windows::process::CommandExt;`.

   Functions:
   - `process_alive(pid: u32) -> bool` — use `windows-sys` crate: `OpenProcess(SYNCHRONIZE, 0, pid)` (note: `BOOL` is `i32`, not Rust `bool` — use `0` not `false`), check non-null handle, `CloseHandle`. Returns false on failure.
   - `configure_daemon_spawn(cmd: &mut Command)` — use `std::os::windows::process::CommandExt::creation_flags()` with `CREATE_NEW_PROCESS_GROUP` (0x00000200). Returns `()` (modifies in place).
   - `find_binary_on_path(name: &str) -> Option<PathBuf>` — use `Command::new("where.exe").arg(name)` instead of `which`. Parse first line of stdout. Note: Windows paths include `.exe` suffix, which is correct for Windows process spawning.

4. **Update `src/lsp/mod.rs`** — add `mod process;` declaration.
   - Delegation: main
   - Add `mod process;` (private, consumed only by `client` and `daemon` within `lsp/`).

5. **Update `src/lsp/client.rs`** — replace `process_alive` and `process_group(0)`.
   - Delegation: main

   Changes:
   - Remove `process_alive()` function definition (L318-324).
   - In `spawn_daemon()`: remove `use std::os::unix::process::CommandExt;` (L225), replace `.process_group(0)` (L248) with `super::process::configure_daemon_spawn(&mut cmd);` before `.spawn()`. The `Command` builder chain must be split: build `cmd` with args/stdio first, then call `configure_daemon_spawn`, then `.spawn()`.
   - In `ensure_daemon()`: replace `process_alive(pid)` calls (L102, L114) with `super::process::process_alive(pid)`.

6. **Update `src/lsp/daemon.rs`** — replace `discover_ra_binary` `which` fallback.
   - Delegation: main

   Changes:
   - In `discover_ra_binary()`: replace the `which` fallback block (L71-78) with a call to `super::process::find_binary_on_path("rust-analyzer")`. The `rustup which` primary path stays as-is (cross-platform). Structure becomes:
     ```rust
     fn discover_ra_binary() -> Result<PathBuf> {
         // Try rustup first (cross-platform)
         if let Ok(output) = Command::new("rustup").args(["which", "rust-analyzer"]).output()
             && output.status.success() { ... }
         // Fall back to platform-specific PATH lookup
         if let Some(path) = super::process::find_binary_on_path("rust-analyzer") {
             return Ok(path);
         }
         bail!("rust-analyzer not found. ...")
     }
     ```

7. **Add `windows-sys` dependency** for Windows process APIs.
   - Delegation: main

   Add to `Cargo.toml` (verify latest version with `cargo search windows-sys`):
   ```toml
   [target.'cfg(windows)'.dependencies]
   windows-sys = { version = "0.59", features = ["Win32_System_Threading", "Win32_Foundation"] }
   ```
   Note: `Cargo.lock` will also change — stage it in the same commit.

8. **Verify: `cargo test` and `cargo clippy`** — all existing tests must pass unchanged, no new warnings.
   - Delegation: main
   - Cross-compilation check deferred to Phase 3 (parent `lsp` module is `#[cfg(unix)]`).

## Testing Strategy

| Module | Approach | Rationale |
|--------|----------|-----------|
| `process/unix.rs` | post-impl | Pure extraction — `process_alive` had no direct test (system call wrapper). `configure_daemon_spawn` is a one-liner wrapper. `find_binary_on_path` is a two-line `which` call. All verified via `cargo test` (existing integration tests exercise the full daemon lifecycle). |
| `process/windows.rs` | manual | No Windows runtime available. Cross-compilation type-check deferred to Phase 3. |
| `client.rs` changes | post-impl | Run `cargo test`. Existing `client::tests` still exercise hash, log_tail, fifo, flock, nonblock. `process_alive` had no direct test. |
| `daemon.rs` changes | post-impl | Run `cargo test`. Existing `daemon::tests` cover progress tracking (unrelated to extraction). |

**Key scenarios for post-impl:**
- `cargo test` — all 224+ integration tests pass (unchanged behavior)
- `cargo clippy` — no new warnings
- Manual: `cargo brief lsp touch` on a real project still works (daemon spawns, process_group detachment works)

## Success Criteria

- `process/mod.rs` + `process/unix.rs` + `process/windows.rs` exist under `src/lsp/`
- `client.rs` no longer contains `process_alive()` or `process_group(0)` — both delegated to `process` module
- `daemon.rs` `discover_ra_binary()` no longer calls `which` directly — delegates to `process::find_binary_on_path()`
- `cargo test` passes with no regressions
- `cargo clippy` clean
- `windows-sys` added as `cfg(windows)` dependency
