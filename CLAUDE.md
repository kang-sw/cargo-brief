# CLAUDE.md — cargo-brief

## Project Summary

**cargo-brief** — A visibility-aware Rust API extractor that outputs pseudo-Rust documentation
for AI agent consumption. Cargo subcommand (`cargo brief`). Solo dev.
Target: **Stable external-crate support and same_crate auto-detection (v0.2).**

## Tech Stack

Rust (edition 2024) + clap 4. Key libs: rustdoc-types 0.57, serde_json, anyhow.

## Workspace

```
src/            — source code (lib.rs entry, main.rs CLI, modules for resolve/model/render)
tests/          — integration tests
test_fixture/   — sample crate exercising all supported item types
notes/ai-docs/  — AI-maintained project docs, tickets, dependency API notes
```

## Architecture Rules

1. **Visibility is the core feature.** All filtering must respect `--at-mod` / `--at-package`
   semantics. Never show items that wouldn't compile if `use`d from the observer's position.
2. **Output is pseudo-Rust for LLMs.** Not machine-parseable JSON. Must be valid enough for
   syntax highlighters but function bodies are replaced with `;` and hidden fields with `..`.
3. **Single cargo metadata call.** `resolve::load_cargo_metadata()` is the single source of
   truth for workspace info, target dir, and package resolution. No redundant subprocess calls.
4. **Nightly rustdoc JSON backend.** Always invoke `cargo +nightly rustdoc` with
   `--output-format json -Z unstable-options --document-private-items`.

---

## Project Knowledge

Project state, architecture, and source layout live in **`notes/ai-docs/_index.md`**.
This is the primary cross-session context document.

**When to read:** Load `_index.md` at session start. Load relevant module docs before tasks.
**When to update:** After implementing changes that affect operational state or a module's
  public API. Update the specific section/doc, not everything.
**Language:** All files under `notes/ai-docs/` are AI-maintained — write them in English only.

**Tickets** (`notes/ai-docs/tickets/YYMMDD-<name>.md`) track substantial features.
In-progress tickets use a `[wip]` suffix: `YYMMDD-<name>[wip].md`.
Remove the `[wip]` marker when the ticket is complete.
Phases that require non-trivial design before coding are marked **(plan mode)** — use the
`EnterPlanMode` tool, explore + design, get user approval, then `ExitPlanMode` to implement.
After completing a ticket phase, append a `### Result (<short-hash>)` subsection recording:
what was implemented, deviations from the plan, and key findings for future phases.

**MEMORY.md** (`~/.claude/projects/.../memory/MEMORY.md`) persists across sessions.
Stores user-specific preferences only (communication style, workflow habits).
Project-specific memory (build memos, recent context, workspace ref) lives in the
`# MEMORY` section at the bottom of this file so it's git-tracked with the project.

## Coding Guidelines

0. **English only for AI-authored content.** All documents, plans, commit messages, ticket
   entries, and inline code comments written by the AI must be in English — regardless of the
   conversation language. This includes `notes/ai-docs/`, `MEMORY` sections, `### Result`
   entries, and any other machine-maintained artifacts. Human-facing UI strings are exempt.
1. **Think first.** State assumptions. Verify before guessing. Define clear success
   criteria before starting — what must hold true when done.
2. **Simplicity.** Write the simplest code that works. Implement fully when the spec is
   clear — judge scope by AI effort, not human-hours.
3. **Surgical changes.** Change only what the task requires. Follow existing style. Every
   changed line must trace to the request.
4. **Test proactively.** Write unit tests for non-trivial pure logic (math, protocol, ECS,
   state machines) as you code. Run the test suite before moving on.
   When tests fail, first diagnose whether the **test assumptions** or the **implementation
   logic** is wrong — don't blindly fix the implementation to match a bad test.
   For user-interactive features (UI, visual output), request manual testing instead.
5. **Module structure.** Split files at ~300 lines into `<module>/mod.rs` + submodules
   (or language-equivalent: `index.ts`, `__init__.py`, etc.). The entry file should contain
   doc comments + public re-exports only — reading it alone conveys the module's interface.
6. **Hot-path performance.** In performance-critical paths, prefer stack allocation,
   pre-allocated buffers, and borrowed references over heap allocation. Apply only when
   benefit clearly outweighs complexity cost.

## Workflow — AI-Driven Implementation

### Approval Protocol
- **Auto-proceed**: Bug fixes, pattern-following additions, test code, boilerplate,
  refactoring within a single module.
- **Ask first**: New component/protocol additions, architectural changes,
  cross-module interface changes, anything that changes observable behavior.
- **Always ask**: Deleting existing functionality, changing protocol/API semantics,
  modifying persistence schema.

### Implementation Process
1. **Task list first.** For non-trivial changes, break the work into brief steps
   via `TaskCreate` — implementation, docs, and commit. Check them off as you progress.
2. **Verify.** Run `cargo test` (unit + integration). Must pass before committing.
3. **Build.** Run `cargo build` so all artifacts are up to date.
4. **Update docs.** After non-trivial tasks:
   - Update `_index.md` Operational State if project capabilities changed.
   - Update `# MEMORY` section in this file (what was done, what's next).
   - If completing a ticket phase, append `### Result` to that phase in the ticket doc.
   - Prune aggressively: keep both documents focused on current state.
5. **Commit.** Auto-create git commits broken down by logical units.
   Commit messages must include an **AI context** section after the change summary.
   Record design decisions, alternatives considered, trade-offs — focus on *why*
   this approach was chosen. Format:
   ```
   <type>(<scope>): <summary>

   <what changed — brief>

   ## AI Context
   - <decision rationale, rejected alternatives, user directives, etc.>
   ```

### Session Start
- Read `notes/ai-docs/_index.md` to understand project state and architecture.
- Run `git log --oneline -10` to catch up on recent work.

### Dependency API Notes
- **`notes/ai-docs/deps/<package>[v<ver>].md`** stores verified API facts for libraries
  whose actual API differs from training knowledge or is too new to be known.
- **When to read:** Before writing code that uses a package listed in
  `# MEMORY → Documented Dependencies`. Also check on compile/type errors that look like
  wrong signatures, missing types, or changed fields — consult `notes/ai-docs/deps/` **before**
  exploring package source from scratch.
- **When to write/update:** After discovering API drift (wrong arg count, renamed types,
  removed methods, etc.) or after learning a previously-unknown package's API, document
  the verified correct API so future sessions skip re-exploration.

### Context Window Discipline
- Keep context small. Load only the module docs relevant to the current task.
- Source code is the ground truth; docs supplement it.
- When a module doc drifts from source, update the doc (or flag it).

---

# MEMORY

<!-- AI-maintained. Update after each non-trivial session. Prune aggressively. -->

## Build & Workflow

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy`
- Requires nightly toolchain for rustdoc JSON generation (`cargo +nightly rustdoc`)

## Recent Work

- v0.1.2: `{ .. }` for trait impl rendering, visibility-filtered field indicators
- Flexible resolution: `self`, `crate::module`, file path→module conversion
- Active ticket: visibility auto-detection & rendering fixes (260308)

## Workspace Reference

- Crate name: `cargo-brief` (binary: `cargo-brief`, lib: `cargo_brief`)
- Entry: `src/lib.rs` → `run_pipeline()`, `src/main.rs` → CLI
- Modules: `cli`, `resolve`, `rustdoc_json`, `model`, `render`
- Test fixture: `test_fixture/` (sample crate with all item types)
- Integration tests: `tests/integration.rs`

## Documented Dependencies

- (none yet — add entries here as API drift is discovered)
