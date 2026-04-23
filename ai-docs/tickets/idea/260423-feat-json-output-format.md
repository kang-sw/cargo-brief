---
title: "--format json: machine-readable JSON output mode"
---

# --format json: machine-readable JSON output mode

## Background

cargo-brief's current output philosophy is "pseudo-Rust for LLMs" — human-readable text
designed for language model consumption. A `--format json` flag would add a machine-readable
output path for programmatic consumers: CI pipelines, editor integrations, diff tools, or
wrapper tools that want to re-render the data in their own format.

## Constraints

- **Philosophical tension.** The project charter explicitly frames output as "pseudo-Rust for
  LLMs." JSON output is a second rendering pipeline with different goals (machine parsing
  over LLM comprehension). A decision to implement this must resolve whether JSON is an
  alternative renderer of the same data, or a different data model entirely.
- **Schema stability.** A published JSON schema becomes a semver-relevant API surface.
  Breaking the schema is a minor/major version bump. This is a long-term maintenance
  commitment.
- **Scope.** Which subcommands get JSON support? `api`, `search`, and `summary` each have
  different result shapes. `features` output is already pseudo-TOML and might be a natural
  JSON candidate.

## Decisions

No design decisions made. Deferred to when the idea is promoted to `todo/`.

At promotion time, the key question to resolve first:
- Is JSON an alternative view of the same rustdoc-JSON-derived data, or a subset (e.g.,
  only item paths and kinds, no full signatures)?
- Is the target audience programmatic tooling, or LLMs that handle JSON prompts?
