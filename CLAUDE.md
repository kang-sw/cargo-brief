<!-- AI-maintained project state — read before work, update after -->
<!-- - `ai-docs/_index.md` — architecture, conventions, build/test, session notes -->

# CLAUDE.md — cargo-brief

## Project Memory

Read in this order at every session start, before any other action:

1. **Preamble** — read `ai-docs/_index.md`. Project-level truth that no
   session should re-derive. Prune aggressively: if derivable from code
   or commit history, delete.
2. **Local** — read `ai-docs/_index.local.md` if it exists. .gitignored.
   Machine-bound context (paths, env vars, build config) and personal
   session notes.
3. **Project arc** — run `git log --oneline --graph -50`. Trajectory and
   topic clusters at a glance.
4. **Recent history** — run `git log -10`. Decision rationale via AI Context
   sections. Fades as history grows.

## Response Discipline

- **Evidence before claims.** Run verification commands and read output before
  stating success. Never use "should pass", "probably works", or "looks correct."
- **No performative agreement.** Never respond with "Great point!", "You're
  absolutely right!", or similar. Restate the technical requirement, verify
  against the codebase, then act (or push back with reasoning).
- **Actions over words.** "Fixed. [what changed]" or just show the diff.
  Skip gratitude expressions and filler.

## Code Standards

1. **Simplicity.** Write the simplest code that works. Implement fully when the spec is
   clear — judge scope by AI effort, not human-hours.
2. **Surgical changes.** Change only what the task requires. Follow existing style. Every
   changed line must trace to the request.
3. **Responsibility check.** As you implement, ask whether each change
   keeps the module's role clean. Split when responsibility drifts.
4. **Module structure.** Split files at ~300 lines. Extract an entry file
   (e.g. `mod.rs`, `index.ts`, `__init__.py`) containing doc comments and public
   re-exports only — reading it alone conveys the module's interface.
5. **Hot-path performance.** In performance-critical paths, prefer minimal allocation
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

## Ticket Updates                          # optional — only when ticket-driven
- <ticket-stem>[: <optional-label>]
  > Forward: <what future phases must know>

## Spec                                    # optional — one per affected spec feature; omit if none
- <spec-stem>
```

When a spec heading's `{#slug}` changes, include `renamed-spec: <old-stem> → <new-stem>` in the commit message body.

### Context Window Discipline

- Source code is ground truth; load only docs relevant to the current task. Update drifted docs on contact.

### Dependency API Notes

- **`ai-docs/deps/<package>[v<ver>].md`** stores verified API facts for libraries
  whose actual API differs from training knowledge or is too recent to be known.
- **When to read:** Before writing code that uses a package listed in
  `_index.md → Documented Dependencies`. On compile/type errors resembling wrong
  signatures, missing types, or changed fields, consult `ai-docs/deps/` **before**
  exploring package source from scratch.
- **When to write/update:** After discovering API drift (wrong arg count, renamed types,
  removed methods) or learning a previously unknown package's API. Document the verified
  correct API so future sessions skip re-exploration.

## Architecture Rules

1. **Visibility is the core feature.** All filtering must respect `--at-mod` / `--at-package`
   semantics. Never show items that wouldn't compile if `use`d from the observer's position.
2. **Output is pseudo-Rust for LLMs.** Not machine-parseable JSON. Must be valid enough for
   syntax highlighters but function bodies are replaced with `;` and hidden fields with `..`.
3. **Single cargo metadata call.** `resolve::load_cargo_metadata()` is the single source of
   truth for workspace info, target dir, and package resolution. No redundant subprocess calls.
4. **Nightly rustdoc JSON backend.** Always invoke `cargo +nightly rustdoc` with
   `--output-format json -Z unstable-options --document-private-items`.

## Project Knowledge

- Project state and cross-session context live in `ai-docs/`.
- Before creating or editing tickets, load `/write-ticket` for conventions.
- Reference tickets by **stem only** (e.g., `260115-feat-foo-bar`), never by
  full path — stems stay stable across status moves.
- When starting work on a ticket, move it to `wip/` immediately.
- To check ticket completion or prior phase results, use `git log --grep=<ticket-stem>`
  and look for `## Ticket Updates` sections in matching commits.
- **Language:** All AI-authored artifacts — documents, plans, commit messages, ticket entries,
  `### Result` entries, and inline code comments — must be in English regardless of
  conversation language. Human-facing UI strings are exempt.
- **Tickets** (`ai-docs/tickets/<status>/YYMMDD-<category>-<name>.md`) track substantial features.
  `YYMMDD` is the **creation date**; it never changes when the ticket moves between statuses.
  Categories: `bug`, `feat`, `refactor`, `chore`, `research`.
  - Frontmatter requires `title` and `status`. Add `plans:` for phases with existing plan
    documents. Add `started: YYYY-MM-DD` on move to `wip/`; add `completed: YYYY-MM-DD`
    on move to `done/`. Add `parent:` for epic sub-tickets where applicable.
  - Status is directory-based: `idea/` → `todo/` → `wip/` → `done/` (or `dropped/`).
  - Phases requiring non-trivial design before coding are marked **(plan mode)** — use
    `EnterPlanMode`, explore + design, get user approval, then `ExitPlanMode` to implement.
  - After completing a ticket phase, append a `### Result (<short-hash>) - YY-MM-DD` subsection
    recording what was implemented, deviations from the plan, and key findings for future phases.

<!-- Inclusion test: if breaking this rule makes a skill produce
     wrong results, it belongs here. Everything else goes in
     _index.md (context) or skills (process). -->

<!-- Template Version: v0025 -->
