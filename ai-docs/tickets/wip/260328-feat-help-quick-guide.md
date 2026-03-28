---
title: Add AI agent quick guide to --help output
status: wip
started: 2026-03-28
---

# Add AI agent quick guide to --help output

## Problem

AI agents running `cargo brief --help` get only subcommand names with one-line
descriptions. They cannot determine which subcommand fits their situation without
an external agent definition file (e.g., `rust-api-lookup.md`).

## Solution

- `after_long_help` (`--help`): condensed quick guide with situation-to-subcommand
  mapping, common workflow, remote crate flags, and tips.
- `after_help` (`-h`): short pointer to `--help` for the full guide.

This makes `cargo brief --help` self-sufficient for AI agent onboarding.
