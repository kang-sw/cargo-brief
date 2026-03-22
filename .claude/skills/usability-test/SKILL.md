---
description: "Run LLM-based usability and regression tests for cargo-brief CLI"
---

# Usability Test

Run qualitative usability tests on cargo-brief using an LLM evaluator agent.

## Arguments

- `/usability-test` — run all tests (smoke + quality + exploratory)
- `/usability-test smoke` — smoke tests only
- `/usability-test quality` — quality evaluation only
- `/usability-test explore` — exploratory test only
- `/usability-test explore <crate>` — exploratory test focused on a specific crate

## Procedure

### Step 1: Build

Run `cargo build`. If the build fails, report the error and stop.

### Step 2: Read Spec

Read `ai-docs/usability-test-spec.md` for test definitions and evaluation criteria.

### Step 3: Determine Scope

Parse the skill arguments to determine which test categories to run:
- No args or `all` → smoke + quality + exploratory
- `smoke` → smoke tests only
- `quality` → quality tests only
- `explore` → exploratory tests only
- `explore <crate>` → exploratory focused on given crate

### Step 4: Spawn Evaluator

Spawn a **single** agent with `model: "haiku"` and compose the prompt as follows:

```
You are a QA evaluator for the cargo-brief CLI tool. cargo-brief extracts
Rust crate APIs as pseudo-Rust documentation for AI consumption.

Your task: run the specified test commands, evaluate their output, and
produce a structured verdict report.

## Setup

Binary path: ./target/debug/cargo-brief
Working directory: {project_root}

When running commands, substitute BRIEF with the binary path above.

## Rules

- Run each command via the Bash tool
- For commands with -C flag (remote crates), use timeout: 180000 and dangerouslyDisableSandbox: true
- Capture both stdout and stderr
- For smoke tests: check exit code and stdout non-empty
- For quality tests: evaluate output against each criterion specifically —
  cite what IS present and what is MISSING
- For error-handling tests (Q05, Q06): a non-zero exit IS the expected behavior
- Be specific in verdicts: "Serialize trait present with 3 methods" not "looks good"
- Do NOT run `clean` subcommand

## Test Definitions

{paste full contents of ai-docs/usability-test-spec.md here}

## Scope

Run these categories: {smoke|quality|explore|all}
{if explore with crate: "Focus exploratory testing on: <crate>"}

Produce the report in the verdict format defined in the spec.
End with the Summary section.
```

### Step 5: Present Results

Relay the agent's report to the user verbatim. If there are FAIL or WARN
verdicts, highlight them at the top before the full report.
