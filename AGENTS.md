# AGENTS.md - cargo-brief

## Project Memory

Read at every session start, before other action:

1. **Preamble** - read `ai-docs/_index.md`; keep only context a session must not re-derive.
2. **Local** - read `ai-docs/_index.local.md` if present; it is .gitignored machine context.
3. **Project arc** - run `git log --oneline --graph -50`.
4. **Recent history** - run `git log -10` for `## AI Context` rationale.

## Response Discipline

- **Evidence before claims.** Run verification and read output before stating success.
- **No performative agreement.** Restate the requirement, verify, then act or push back.
- **Actions over words.** Prefer "Fixed. [what changed]" or the diff. Skip filler.

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

Auto-create one commit per logical unit. Include `## AI Context` explaining why the approach was chosen.
**Version bumps must always include a `CHANGELOG.md` update and `Cargo.lock` changes.**

```text
<type>(<scope>): <summary>

<what changed - brief>

## AI Context
- <decision rationale, rejected alternatives, user directives, etc.>

## Ticket Updates                          # optional - ticket-driven only
- <ticket-stem>[: <optional-label>]
  > Forward: <future-phase finding>

## Spec                                    # optional - omit when none
- <spec-stem>
```

When a spec heading `{#slug}` changes, include `renamed-spec: <old-stem> -> <new-stem>`.

### Context Window Discipline

- Source code is ground truth; load only docs relevant to the task.
- Update drifted docs on contact.

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
- Before creating or editing tickets, load the write-ticket workflow skill for conventions.
- Reference tickets by **stem only** (e.g., `260115-feat-foo-bar`), never by
  full path — stems stay stable across status moves.
- Check `## Ticket Queue` in `ai-docs/_index.md` before starting implementation; it lists `ready/` work only.
- To check ticket completion or prior phase results, use `git log --grep=<ticket-stem>`
  and look for `## Ticket Updates` sections in matching commits.
- **Language:** All AI-authored artifacts — documents, plans, commit messages, ticket entries,
  `### Result` entries, and inline code comments - must be in English regardless of
  conversation language. Human-facing UI strings are exempt.
- **Tickets** (`ai-docs/tickets/<status>/YYMMDD-<category>-<name>.md`) track substantial features.
  `YYMMDD` is the **creation date**; it never changes when the ticket moves between statuses.
  Categories: `bug`, `feat`, `refactor`, `chore`, `research`.
  - Frontmatter requires `title` and `status`. Add `plans:` for phases with existing plan
    documents. Add `completed: YYYY-MM-DD` on move to `.done/`. Add `parent:` for epic sub-tickets where applicable.
  - Active status is directory-based: `idea/` -> `todo/` -> `ready/`. Archive status is `.done/` or `.dropped/`.
  - Phases requiring non-trivial design before coding are marked **(plan mode)** — use
    `EnterPlanMode`, explore + design, get user approval, then `ExitPlanMode` to implement.
  - After completing a ticket phase, append a `### Result (<short-hash>) - YY-MM-DD` subsection
    recording what was implemented, deviations from the plan, and key findings for future phases.

<!-- Inclusion test: if breaking this rule makes a skill produce
     wrong results, it belongs here. Everything else goes in
     _index.md (context) or skills (process). -->

<!-- Template Version: v0035 -->
