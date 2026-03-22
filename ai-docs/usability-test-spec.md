# Usability Test Specification

Test scenarios for LLM-based evaluation of cargo-brief CLI output quality.
This file is read by the `/usability-test` skill and passed to a haiku evaluator agent.

## Setup

Binary: `./target/debug/cargo-brief` (built via `cargo build` before tests).
All commands below use `BRIEF` as shorthand for the binary path.

Timeout guidance:
- Local commands: default timeout (120s)
- Remote commands (`-C` flag): 180s — first run downloads and builds crates

---

## Smoke Tests

Commands that **must exit 0 and produce non-empty stdout**.
Any non-zero exit or empty stdout is an automatic **FAIL**.

| ID  | Command                              | Verify                                              |
|-----|--------------------------------------|------------------------------------------------------|
| S01 | `BRIEF --help`                       | Lists subcommands: api, search, examples, summary, clean |
| S02 | `BRIEF api --help`                   | Shows --depth, --recursive, --expand-glob            |
| S03 | `BRIEF search --help`                | Shows --limit, --methods-of, --members               |
| S04 | `BRIEF api self`                     | Produces pseudo-Rust output                          |
| S05 | `BRIEF search self fn`               | Finds at least one function                          |
| S06 | `BRIEF summary self`                 | Shows module-level overview                          |
| S07 | `BRIEF -C api serde`                 | Produces serde API output                            |
| S08 | `BRIEF -C search serde Serialize`    | Finds Serialize trait                                |

---

## Quality Tests

Run each command and evaluate the output against listed criteria.
Use PASS / WARN / FAIL per test.

### Q01: Self API
**Command:** `BRIEF api self`
**Criteria:**
- Output resembles valid Rust (mod blocks, fn signatures, struct/enum definitions)
- Module hierarchy visible (nested `mod` blocks or qualified paths)
- Key public functions present (e.g., `run_api_pipeline`, `run_search_pipeline`)
- Doc comments from source preserved in output

### Q02: Serde API
**Command:** `BRIEF -C api serde`
**Criteria:**
- `Serialize` and `Deserialize` traits present with method signatures
- Module structure visible (ser, de modules or re-exports)
- Doc comments present on major types
- Re-exports resolved to user-facing paths (not internal crate paths)

### Q03: Tokio Search
**Command:** `BRIEF -C search tokio@1 spawn`
**Criteria:**
- `tokio::spawn` (or `tokio::task::spawn`) function found
- `spawn_blocking` found
- Results show full qualified paths
- Function signatures visible in results

### Q04: Clap API
**Command:** `BRIEF -C api clap`
**Criteria:**
- Major types visible: `Command`, `Arg`, `ArgMatches`
- Builder methods present on key types
- Output is navigable — not just an unsorted wall of items
- Re-exports from sub-crates resolved to `clap::` paths

### Q05: Error — Nonexistent Crate
**Command:** `BRIEF -C api nonexistent-crate-xyz-12345`
**Expected:** Non-zero exit
**Criteria:**
- Error message mentions the crate name or "not found"
- No stack trace or panic output
- Helpful, actionable message

### Q06: Error — Nonexistent Module
**Command:** `BRIEF api self nonexistent::module::path`
**Expected:** Non-zero exit
**Criteria:**
- Error mentions "module not found" or lists available modules
- No panic

### Q07: Compact Output
**Command:** `BRIEF -C api serde --compact --no-docs`
**Criteria:**
- Output is noticeably shorter than Q02 (no doc comments, compressed layout)
- Still contains the same key items (Serialize, Deserialize)
- Readable despite compactness

### Q08: Search with Members
**Command:** `BRIEF -C search serde --members Serialize`
**Criteria:**
- Serialize trait found with associated methods/types expanded
- Member items displayed (not just the trait name)
- Continuation lines (`-::member`) visible if applicable

---

## Exploratory Test

Autonomous discovery — the agent picks commands and evaluates results.

### Instructions
1. Run `BRIEF --help` and read all available subcommands and flags
2. Pick **2 crates** from this allowlist: `serde, tokio, clap, anyhow, regex, axum`
3. For each crate, try **at least 3** different command combinations:
   - Different subcommands (api, search, summary, examples)
   - Different flags (--depth 2, --compact, --no-docs, --members, --recursive)
   - Module targeting (e.g., `tokio@1 net`, `clap derive`)
4. Report findings in these categories:
   - **Broken**: commands that should work but error unexpectedly
   - **Degraded**: output that seems incomplete, garbled, or misleading
   - **Confusing**: unhelpful error messages or surprising behavior
   - **Notable**: surprisingly good output worth calling out

### Constraints
- Max 10 command invocations for exploratory testing
- Skip crates that failed in smoke/quality tests (already reported)
- Do not test `clean` subcommand (destructive)

---

## Verdict Format

Report each test as:

```
### <ID>: <name>
Command: `<actual command run>`
Exit: <code>
Verdict: PASS | WARN | FAIL
<explanation — required for WARN/FAIL, optional for PASS>
```

For exploratory tests, use IDs like `E01`, `E02`, etc.

End the report with:

```
## Summary
PASS: <n> | WARN: <n> | FAIL: <n>

### Issues
- [FAIL] <ID>: <one-line description>
- [WARN] <ID>: <one-line description>
```

If all tests pass, the Issues section can say "None."
