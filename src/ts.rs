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

/// Run a tree-sitter query against all source files and format output.
pub fn run_query(source_root: &Path, query_src: &str, args: &TsArgs) -> Result<String> {
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

    // Auto-add a root capture when the query has none, so capture-less queries
    // like `(function_item)` work in verbatim mode.
    let probe = Query::new(&language, query_src)
        .map_err(|e| anyhow::anyhow!("Invalid tree-sitter query: {e}"))?;
    let augmented = if probe.capture_names().is_empty() {
        Some(format!("{query_src} @_match"))
    } else {
        None
    };
    let effective_src = augmented.as_deref().unwrap_or(query_src);
    let query = if augmented.is_some() {
        Query::new(&language, effective_src)
            .map_err(|e| anyhow::anyhow!("Invalid tree-sitter query: {e}"))?
    } else {
        probe
    };

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .context("Failed to set tree-sitter Rust language")?;

    let files = collect_source_files(source_root);
    let capture_names = query.capture_names().to_vec();
    let (ctx_before, ctx_after) = examples::parse_context(&args.context);
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
                let node = if !query_match.captures.is_empty() {
                    query_match.captures[0].node
                } else {
                    continue;
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
