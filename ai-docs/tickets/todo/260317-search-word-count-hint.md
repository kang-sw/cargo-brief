# UX: Hint about multi-word AND search word count

## Priority: P3

## Problem

`--search` with many words (4+) often returns 0 results because ALL words must
appear in a single item's full path. Users/agents try queries like
`"routing get post put delete patch"` expecting OR semantics or broad matching.

## Possible Fixes

1. **Help text hint**: add "(2-3 words work best)" to the `--search` description
2. **Warning at runtime**: if 4+ words and 0 results, print a hint suggesting
   fewer words
3. **OR mode**: add `--search-or` or change multi-word semantics (breaking change,
   probably not worth it)

## Discovered By

Naive-agent testing (2026-03-17): agent tried 5-word AND query, got 0 results,
had to learn to narrow down. Not a blocker but a minor friction point.
