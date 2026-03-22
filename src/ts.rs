//! Tree-sitter structural query execution against crate source files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::cli::TsArgs;
use crate::examples;

/// Collect all `.rs` files from src/, examples/, tests/, benches/.
fn collect_source_files(source_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir_name in &["src", "examples", "tests", "benches"] {
        let dir = source_root.join(dir_name);
        if dir.is_dir() {
            files.extend(examples::collect_rs_files(&dir, 999));
        }
    }
    files.sort();
    files
}

/// Find positions after each top-level pattern's closing `)` in an S-expression query.
/// Handles string literals and escaped characters inside predicates.
fn find_pattern_boundaries(query_src: &str) -> Vec<usize> {
    let mut boundaries = Vec::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let bytes = query_src.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' if !in_string => in_string = true,
            b'"' if in_string => in_string = false,
            b'\\' if in_string => {
                i += 1; // skip escaped char
            }
            b'(' if !in_string => depth += 1,
            b')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    boundaries.push(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }

    boundaries
}

/// Auto-add `@_match` capture to each top-level pattern that doesn't already
/// have a root capture. This ensures verbatim mode can show the full matched
/// node (pattern root) instead of just the first named capture.
fn augment_with_root_captures(query_src: &str) -> String {
    let boundaries = find_pattern_boundaries(query_src);
    if boundaries.is_empty() {
        return query_src.to_string();
    }

    let mut result = String::with_capacity(query_src.len() + boundaries.len() * 8);
    let mut last = 0;

    for &pos in &boundaries {
        result.push_str(&query_src[last..pos]);
        // Check if already followed by @capture (skip whitespace)
        let rest = query_src[pos..].trim_start();
        if !rest.starts_with('@') {
            result.push_str(" @_match");
        }
        last = pos;
    }
    result.push_str(&query_src[last..]);
    result
}

/// Run a tree-sitter query against all source files and format output.
pub fn run_query(source_root: &Path, query_src: &str, args: &TsArgs) -> Result<String> {
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

    // Validate the query first with the original source.
    Query::new(&language, query_src)
        .map_err(|e| anyhow::anyhow!("Invalid tree-sitter query: {e}"))?;

    // Auto-add @_match capture on each pattern root for verbatim mode.
    let augmented = augment_with_root_captures(query_src);
    let query = Query::new(&language, &augmented)
        .map_err(|e| anyhow::anyhow!("Invalid tree-sitter query: {e}"))?;

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .context("Failed to set tree-sitter Rust language")?;

    let files = collect_source_files(source_root);
    let capture_names = query.capture_names().to_vec();

    // Find the index of the auto-added @_match capture (if present).
    let match_capture_idx = capture_names
        .iter()
        .position(|n| *n == "_match")
        .map(|i| i as u32);

    let (ctx_before, ctx_after) = examples::parse_context(&args.context);

    if args.captures && (ctx_before > 0 || ctx_after > 0) {
        eprintln!("warning: --context is ignored in --captures mode");
    }

    let mut output = String::new();
    let mut match_count = 0;

    for file_path in &files {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };

        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        if args.captures {
            let mut captures = cursor.captures(&query, root, source.as_bytes());
            while let Some((query_match, _capture_idx)) = captures.next() {
                let rel = file_path.strip_prefix(source_root).unwrap_or(file_path);
                for capture in query_match.captures {
                    // Skip internal @_match captures in --captures mode
                    if Some(capture.index) == match_capture_idx {
                        continue;
                    }
                    let node = capture.node;
                    let name = &capture_names[capture.index as usize];
                    let text = &source[node.start_byte()..node.end_byte()];
                    let line = node.start_position().row + 1;
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&format!("@{}:{}\n", rel.display(), line));
                    output.push_str(&format!("  @{name}: {text}\n"));
                    match_count += 1;
                }
            }
        } else {
            let mut matches = cursor.matches(&query, root, source.as_bytes());
            while let Some(query_match) = matches.next() {
                if query_match.captures.is_empty() {
                    continue;
                }

                // Prefer @_match (pattern root) over first capture
                let node = match match_capture_idx {
                    Some(idx) => query_match
                        .captures
                        .iter()
                        .find(|c| c.index == idx)
                        .map(|c| c.node)
                        .unwrap_or(query_match.captures[0].node),
                    None => query_match.captures[0].node,
                };

                let start_line = node.start_position().row + 1;
                let text = &source[node.start_byte()..node.end_byte()];
                let rel = file_path.strip_prefix(source_root).unwrap_or(file_path);

                if !output.is_empty() {
                    output.push('\n');
                }

                if ctx_before > 0 || ctx_after > 0 {
                    render_with_context(
                        &source,
                        node.start_position().row,
                        node.end_position().row,
                        ctx_before,
                        ctx_after,
                        rel,
                        &mut output,
                    );
                } else {
                    output.push_str(&format!("@{}:{}\n", rel.display(), start_line));
                    output.push_str(text);
                    if !text.ends_with('\n') {
                        output.push('\n');
                    }
                }
                match_count += 1;
            }
        }
    }

    if match_count == 0 {
        output.push_str("// no matches\n");
    }

    Ok(output)
}

/// Render a matched region with context lines.
fn render_with_context(
    source: &str,
    match_start_row: usize,
    match_end_row: usize,
    ctx_before: usize,
    ctx_after: usize,
    rel_path: &Path,
    output: &mut String,
) {
    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len();
    let start = match_start_row.saturating_sub(ctx_before);
    let end = (match_end_row + ctx_after + 1).min(total);

    output.push_str(&format!("@{}:{}\n", rel_path.display(), start + 1));
    for (i, line) in lines[start..end].iter().enumerate() {
        let row = start + i;
        let marker = if row >= match_start_row && row <= match_end_row {
            '*'
        } else {
            ' '
        };
        output.push_str(&format!("{marker} {line}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_pattern_boundaries_single() {
        let q = "(function_item)";
        assert_eq!(find_pattern_boundaries(q), vec![15]);
    }

    #[test]
    fn test_find_pattern_boundaries_multi() {
        let q = "(function_item) (struct_item)";
        assert_eq!(find_pattern_boundaries(q), vec![15, 29]);
    }

    #[test]
    fn test_find_pattern_boundaries_nested() {
        let q = "(impl_item trait: (type_identifier) @t (#eq? @t \"MyTrait\"))";
        assert_eq!(find_pattern_boundaries(q), vec![q.len()]);
    }

    #[test]
    fn test_find_pattern_boundaries_string_with_paren() {
        let q = "(impl_item (#eq? @t \")\"))";
        assert_eq!(find_pattern_boundaries(q), vec![q.len()]);
    }

    #[test]
    fn test_augment_adds_match() {
        let q = "(function_item name: (identifier) @name)";
        let aug = augment_with_root_captures(q);
        assert!(aug.ends_with("@_match"), "Should add @_match: {aug}");
    }

    #[test]
    fn test_augment_skips_existing_root_capture() {
        let q = "(function_item) @fn";
        let aug = augment_with_root_captures(q);
        assert!(!aug.contains("@_match"), "Should not add @_match: {aug}");
    }

    #[test]
    fn test_augment_multi_pattern() {
        let q = "(function_item) @fn (struct_item)";
        let aug = augment_with_root_captures(q);
        assert!(
            aug.contains("(struct_item) @_match"),
            "Should add @_match to second pattern: {aug}"
        );
        assert!(
            !aug.contains("(function_item) @_match"),
            "Should not add @_match to first pattern: {aug}"
        );
    }
}
