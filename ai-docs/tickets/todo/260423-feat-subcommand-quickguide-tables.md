---
title: "per-subcommand help: add question→flag quick-guide tables"
spec:
  - 260423-ai-agent-quick-guide
---

# per-subcommand help: add question→flag quick-guide tables

## Background

The top-level `cargo brief --help` has a QUICK GUIDE section mapping natural-language
questions to subcommands. Individual subcommand help pages (`api --help`, `search --help`,
etc.) have EXAMPLES sections but no equivalent question→flag guidance. A user who has
already chosen the right subcommand still has to read option descriptions to figure out
which flag combination answers their specific need.

The gap is most visible in `search --help`: `--methods-of`, `--members`, `--limit`, and
the pattern DSL are documented as individual flags but not mapped to common intent. An LLM
agent consuming `--help` to decide how to invoke the tool benefits from explicit
question→answer pairs.

## Phases

### Phase 1: Add WHEN TO USE sections to api, search, summary, features

**Goals:**
For each of the four subcommands, add a `WHEN TO USE` (or `QUICK PATTERNS`) block in the
help text that maps common user intents to flag combinations. Examples:

`search`:
```
WHEN TO USE:
  "Show all methods of a type"          → --methods-of <TYPE>
  "Include fields and variants too"     → --members
  "Find by exact name (no substring)"   → =Name pattern
  "Paginate large result sets"          → --limit OFFSET:N
```

`api`:
```
WHEN TO USE:
  "Compact output for large crates"     → --compact [--no-docs]
  "Drill into a specific module"        → api self::module::path
  "See all impl blocks (incl blanket)"  → --all
  "Hide feature-gate noise"             → --no-feature-gates
```

**Constraints:**
- Tables must be narrow enough to render cleanly in an 80-column terminal.
- Content must not duplicate the EXAMPLES section — EXAMPLES show command lines,
  WHEN TO USE maps intent to flags.
- Keep each table to 4–6 entries; more than 6 loses scannability.
- Subcommands with already-adequate discoverability (`clean`, `lsp`, `ts`, `code`,
  `examples`) may be deferred or omitted if the value-add is low.

**Success criteria:**
- `cargo brief search --help` contains a `WHEN TO USE` block with at least 4 entries.
- `cargo brief api --help` contains a `WHEN TO USE` block with at least 4 entries.
- No existing EXAMPLES content is removed.
