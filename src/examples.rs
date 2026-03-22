//! Example/test/bench source file scanning and grep rendering.
//!
//! Two modes:
//! - **List mode** (no pattern): enumerate files with `//!` doc comments
//! - **Grep mode** (pattern given): show matching lines with context

use std::path::{Path, PathBuf};

use crate::cli::ExamplesArgs;

/// Parse `--context` value. `"N"` → `(N, N)`. `"B:A"` → `(B, A)`.
pub fn parse_context(s: &str) -> (usize, usize) {
    if let Some((b, a)) = s.split_once(':') {
        let before = b.parse().unwrap_or(2);
        let after = a.parse().unwrap_or(2);
        (before, after)
    } else {
        let n = s.parse().unwrap_or(2);
        (n, n)
    }
}

/// Recursively collect `.rs` files under `dir`, up to `max_depth` levels.
/// Depth 0 means only files directly in `dir` (no subdirectories).
pub(crate) fn collect_rs_files(dir: &Path, max_depth: u32) -> Vec<PathBuf> {
    let mut result = Vec::new();
    collect_rs_files_inner(dir, max_depth, &mut result);
    result.sort();
    result
}

fn collect_rs_files_inner(dir: &Path, depth_remaining: u32, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth_remaining > 0 {
                collect_rs_files_inner(&path, depth_remaining - 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Collect all source files based on scope flags.
/// Always includes `examples/`; optionally includes `tests/` and `benches/`.
fn collect_all_files(source_root: &Path, args: &ExamplesArgs) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let examples_dir = source_root.join("examples");
    if examples_dir.is_dir() {
        files.extend(collect_rs_files(&examples_dir, 999));
    }

    if let Some(depth) = args.tests {
        let tests_dir = source_root.join("tests");
        if tests_dir.is_dir() {
            // depth=1 means top-level only; collect_rs_files uses 0-based depth
            files.extend(collect_rs_files(&tests_dir, depth.saturating_sub(1)));
        }
    }

    if let Some(depth) = args.benches {
        let benches_dir = source_root.join("benches");
        if benches_dir.is_dir() {
            files.extend(collect_rs_files(&benches_dir, depth.saturating_sub(1)));
        }
    }

    files.sort();
    files
}

/// Render list mode: files with their `//!` doc comments.
fn render_list(source_root: &Path, files: &[PathBuf]) -> String {
    let mut out = String::new();

    for file in files {
        let relative = file
            .strip_prefix(source_root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        out.push_str(&format!("@{relative}\n"));

        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => {
                out.push_str("    (read error)\n\n");
                continue;
            }
        };

        let mut has_doc = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(stripped) = trimmed.strip_prefix("//!") {
                has_doc = true;
                // Strip leading space after "//!" if present
                let doc = stripped.strip_prefix(' ').unwrap_or(stripped);
                out.push_str(&format!("    {doc}\n"));
            } else if trimmed.is_empty() {
                // Skip blank lines before doc comments
                continue;
            } else {
                // First non-doc, non-empty line → stop
                break;
            }
        }

        if !has_doc {
            out.push_str("    (no module doc)\n");
        }
        out.push('\n');
    }

    out
}

/// Number of decimal digits in a number.
fn digit_count(mut n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    while n > 0 {
        n /= 10;
        count += 1;
    }
    count
}

/// Render grep mode: matching lines with context, `*` markers, dynamic line numbers.
fn render_grep(source_root: &Path, files: &[PathBuf], pattern: &str, context: &str) -> String {
    let (ctx_before, ctx_after) = parse_context(context);
    let case_sensitive = pattern.chars().any(|c| c.is_uppercase());
    let pattern_lower = pattern.to_lowercase();

    let mut out = String::new();

    for file in files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        // Find matching line indices (0-based)
        let matches: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                if case_sensitive {
                    line.contains(pattern)
                } else {
                    line.to_lowercase().contains(&pattern_lower)
                }
            })
            .map(|(i, _)| i)
            .collect();

        if matches.is_empty() {
            continue;
        }

        // Compute context ranges and merge overlapping
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for &m in &matches {
            let start = m.saturating_sub(ctx_before);
            let end = (m + ctx_after).min(total - 1);
            if let Some(last) = ranges.last_mut()
                && start <= last.1 + 1
            {
                // Merge: adjacent or overlapping
                last.1 = last.1.max(end);
                continue;
            }
            ranges.push((start, end));
        }

        // Compute max line number for column width (1-indexed)
        let max_line_no = ranges.last().map_or(1, |r| r.1 + 1);
        let width = digit_count(max_line_no).max(4);

        let relative = file
            .strip_prefix(source_root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        out.push_str(&format!("@{relative}\n"));

        let match_set: std::collections::HashSet<usize> = matches.iter().copied().collect();

        for (range_idx, &(start, end)) in ranges.iter().enumerate() {
            if range_idx > 0 {
                out.push_str("  ...\n");
            }
            for (i, line) in lines.iter().enumerate().take(end + 1).skip(start) {
                let line_no = i + 1; // 1-indexed
                let marker = if match_set.contains(&i) { '*' } else { ' ' };
                out.push_str(&format!(
                    "{marker}{line_no:>width$}:  {line}\n",
                    width = width,
                ));
            }
        }

        out.push('\n');
    }

    out
}

/// Top-level orchestrator: collect files and dispatch to list or grep mode.
pub fn render_examples(source_root: &Path, crate_display: &str, args: &ExamplesArgs) -> String {
    let files = collect_all_files(source_root, args);

    if files.is_empty() {
        return format!("// no examples found for {crate_display}\n");
    }

    let mut out = format!("// examples for {crate_display}\n");
    out.push_str(&format!("// root: {}/\n\n", source_root.display()));

    if let Some(ref pattern) = args.pattern() {
        let grep_output = render_grep(source_root, &files, pattern, &args.context);
        if grep_output.is_empty() {
            out.push_str(&format!("// no matches for \"{pattern}\"\n"));
        } else {
            out.push_str(&grep_output);
        }
    } else {
        out.push_str(&render_list(source_root, &files));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_context_single() {
        assert_eq!(parse_context("3"), (3, 3));
        assert_eq!(parse_context("0"), (0, 0));
    }

    #[test]
    fn test_parse_context_pair() {
        assert_eq!(parse_context("1:3"), (1, 3));
        assert_eq!(parse_context("0:5"), (0, 5));
    }

    #[test]
    fn test_parse_context_invalid() {
        assert_eq!(parse_context("abc"), (2, 2)); // falls back to default
    }

    #[test]
    fn test_digit_count() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(1), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
        assert_eq!(digit_count(9999), 4);
    }
}
