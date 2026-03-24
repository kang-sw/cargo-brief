<!-- AI-maintained project state — read before work, update after -->
<!-- - `ai-docs/_index.md` — architecture overview, conventions -->
<!-- - `ai-docs/_memory.md` — recent work, workspace reference -->

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
ai-docs/        — AI-maintained project docs, tickets, dependency API notes
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

Project state and cross-session context live in `ai-docs/`.
Read `_index.md` and `_memory.md` at session start.
Before creating or editing tickets, load `/write-ticket` for conventions.
Reference tickets by **stem only** (e.g., `260115-feat-foo-bar`), never by
full path — stems stay stable across status moves.

**Language:** All AI-authored artifacts — documents, plans, commit messages, ticket entries,
`### Result` entries, and inline code comments — must be in English regardless of
conversation language. Human-facing UI strings are exempt.

```
ai-docs/
  _index.md          — project architecture and stable conventions
  _memory.md         — cross-session continuity, updated each session
  mental-model/      — architecture docs, regenerable from source
  deps/              — external library API delta docs
  ref/               — static reference material (external specs, protocol docs, design notes)
  tickets/<status>/  — idea/ todo/ wip/ done/ dropped/
```

**When to read:** Load `_index.md` and `_memory.md` at session start. Load relevant module
docs before tasks.
**When to update:** After implementing changes that affect operational state or a module's
public API. Update the specific section/doc, not everything.

**Tickets** (`ai-docs/tickets/<status>/YYMMDD-<category>-<name>.md`) track substantial features.
`YYMMDD` is the **creation date**; it never changes when the ticket moves between statuses.
Categories: `bug`, `feat`, `refactor`, `chore`, `research`.

- Frontmatter requires `title` and `status`. Add `started: YYYY-MM-DD` on move to
  `wip/`; add `completed: YYYY-MM-DD` on move to `done/`.
- Status is directory-based: `idea/` → `todo/` → `wip/` → `done/` (or `dropped/`).
- Phases requiring non-trivial design before coding are marked **(plan mode)** — use
  `EnterPlanMode`, explore + design, get user approval, then `ExitPlanMode` to implement.
- After completing a ticket phase, append a `### Result (<short-hash>) - YY-MM-DD` subsection
  recording what was implemented, deviations from the plan, and key findings for future phases.

## Code Standards

1. **Simplicity.** Write the simplest code that works. Implement fully when the spec is
   clear — judge scope by AI effort, not human-hours.
2. **Surgical changes.** Change only what the task requires. Follow existing style. Every
   changed line must trace to the request.
3. **Module structure.** Split files at ~300 lines. Extract an entry file
   (e.g. `mod.rs`, `index.ts`, `__init__.py`) containing doc comments and public
   re-exports only — reading it alone conveys the module's interface.
4. **Hot-path performance.** In performance-critical paths, prefer minimal allocation
   and data locality over convenience abstractions. Apply only when benefit clearly
   outweighs complexity cost.

## Workflow

### Approval Protocol

- **Auto-proceed**: Bug fixes, pattern-following additions, test code, boilerplate,
  refactoring within a single module.
- **Ask first**: New component/protocol additions, architectural changes,
  cross-module interface changes, anything that changes observable behavior.
- **Always ask**: Deleting existing functionality, changing protocol/API semantics,
  modifying persistence schema.

### Commit Rules

Auto-create git commits, each covering one logical unit of change.
Include an **AI context** section in every commit message recording design decisions,
alternatives considered, and trade-offs — focus on _why_ this approach was chosen.
**Version bumps must always include a `CHANGELOG.md` update and `Cargo.lock` changes.**

```
<type>(<scope>): <summary>

<what changed — brief>

## AI Context
- <decision rationale, rejected alternatives, user directives, etc.>
```

### Session Start

- Read `ai-docs/_index.md` and `ai-docs/_memory.md` for project context.
- Run `git log --oneline -10` for recent changes.

### Dependency API Notes

- **`ai-docs/deps/<package>[v<ver>].md`** stores verified API facts for libraries
  whose actual API differs from training knowledge or is too recent to be known.
- **When to read:** Before writing code that uses a package listed in
  `_memory.md → Documented Dependencies`. On compile/type errors resembling wrong
  signatures, missing types, or changed fields, consult `ai-docs/deps/` **before**
  exploring package source from scratch.
- **When to write/update:** After discovering API drift (wrong arg count, renamed types,
  removed methods) or learning a previously unknown package's API. Document the verified
  correct API so future sessions skip re-exploration.

### Response Discipline

- **Evidence before claims.** Run verification commands and read output before
  stating success. Never use "should pass", "probably works", or "looks correct."
- **No performative agreement.** Never respond with "Great point!", "You're
  absolutely right!", or similar. Restate the technical requirement, verify
  against the codebase, then act (or push back with reasoning).
- **Actions over words.** "Fixed. [what changed]" or just show the diff.
  Skip gratitude expressions and filler.

### Context Window Discipline

- Keep context small. Load only the module docs relevant to the current task.
- Source code is the ground truth; docs supplement it.
- When a module doc drifts from source, update the doc (or flag it).
