//! Pre-crafted tree-sitter code lookup by item kind and name.
//!
//! Bridges the gap between `search` (API shape, no source locations) and `ts`
//! (raw S-expressions). Provides `cargo brief code <target> [kind] <name>`.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::cli::CodeArgs;
use crate::examples;

// ── Item kinds ───────────────────────────────────────────────────────

/// Supported item kinds for code lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Fn,
    Struct,
    Enum,
    Trait,
    Field,
    Type,
    Impl,
    Macro,
    Const,
    Use,
}

impl ItemKind {
    /// Parse a kind keyword. Returns `None` for unrecognized strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fn" => Some(Self::Fn),
            "struct" => Some(Self::Struct),
            "enum" => Some(Self::Enum),
            "trait" => Some(Self::Trait),
            "field" => Some(Self::Field),
            "type" => Some(Self::Type),
            "impl" => Some(Self::Impl),
            "macro" => Some(Self::Macro),
            "const" => Some(Self::Const),
            "use" => Some(Self::Use),
            _ => None,
        }
    }

    /// Display keyword (lowercase, not Debug format).
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Fn => "fn",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Field => "field",
            Self::Type => "type",
            Self::Impl => "impl",
            Self::Macro => "macro",
            Self::Const => "const",
            Self::Use => "use",
        }
    }
}

// ── Argument resolution ──────────────────────────────────────────────

/// Resolved positional arguments for the `code` subcommand.
pub struct ResolvedCodeArgs {
    /// `"self"` or a crate name / spec.
    pub target: String,
    pub kind: Option<ItemKind>,
    pub name: String,
}

/// Parse the variadic positional args (`1..=3`) into target, kind, and name.
///
/// - **1 arg:** `code NAME` → target=`"self"`, catch-all. Errors if the single
///   arg is a kind keyword (ambiguous).
/// - **2 args:** `code KIND NAME` if arg[0] is a kind keyword, else
///   `code TARGET NAME` (catch-all).
/// - **3 args:** `code TARGET KIND NAME` — arg[1] must be a valid kind.
/// - **0 or 4+ args:** error with usage.
pub fn resolve_code_args(args: &CodeArgs) -> Result<ResolvedCodeArgs> {
    match args.args.len() {
        1 => {
            let a = &args.args[0];
            if ItemKind::parse(a).is_some() {
                bail!(
                    "'{}' is an item kind. Usage: cargo brief code [TARGET] {} <name>",
                    a,
                    a
                );
            }
            Ok(ResolvedCodeArgs {
                target: "self".to_string(),
                kind: None,
                name: a.clone(),
            })
        }
        2 => {
            let a0 = &args.args[0];
            let a1 = &args.args[1];
            if let Some(kind) = ItemKind::parse(a0) {
                Ok(ResolvedCodeArgs {
                    target: "self".to_string(),
                    kind: Some(kind),
                    name: a1.clone(),
                })
            } else {
                Ok(ResolvedCodeArgs {
                    target: a0.clone(),
                    kind: None,
                    name: a1.clone(),
                })
            }
        }
        3 => {
            let target = &args.args[0];
            let kind_str = &args.args[1];
            let name = &args.args[2];
            match ItemKind::parse(kind_str) {
                Some(kind) => Ok(ResolvedCodeArgs {
                    target: target.clone(),
                    kind: Some(kind),
                    name: name.clone(),
                }),
                None => bail!(
                    "Unknown item kind '{}'. Valid kinds: fn, struct, enum, trait, field, type, impl, macro, const, use",
                    kind_str
                ),
            }
        }
        _ => bail!(
            "Expected 1–3 positional arguments: [TARGET] [KIND] NAME\n\
             Usage: cargo brief code [TARGET] [KIND] NAME"
        ),
    }
}

// ── Tree-sitter queries ──────────────────────────────────────────────

/// Build a tree-sitter query string for the given item kind (or all kinds).
/// Each pattern has `@name` (the identifier to match) and `@item` (the full node).
fn build_query(kind: Option<ItemKind>) -> String {
    let mut parts = Vec::new();

    let add = |parts: &mut Vec<&str>, k: ItemKind| match k {
        ItemKind::Fn => {
            parts.push("(function_item name: (identifier) @name) @item");
            parts.push("(function_signature_item name: (identifier) @name) @item");
        }
        ItemKind::Struct => {
            parts.push("(struct_item name: (type_identifier) @name) @item");
        }
        ItemKind::Enum => {
            parts.push("(enum_item name: (type_identifier) @name) @item");
        }
        ItemKind::Trait => {
            parts.push("(trait_item name: (type_identifier) @name) @item");
        }
        ItemKind::Field => {
            parts.push("(field_declaration name: (field_identifier) @name) @item");
        }
        ItemKind::Type => {
            parts.push("(type_item name: (type_identifier) @name) @item");
        }
        ItemKind::Impl => {
            parts.push("(impl_item type: (type_identifier) @name) @item");
            parts.push("(impl_item type: (generic_type type: (type_identifier) @name)) @item");
            parts.push(
                "(impl_item type: (scoped_type_identifier name: (type_identifier) @name)) @item",
            );
        }
        ItemKind::Macro => {
            parts.push("(macro_definition name: (identifier) @name) @item");
        }
        ItemKind::Const => {
            parts.push("(const_item name: (identifier) @name) @item");
            parts.push("(static_item name: (identifier) @name) @item");
        }
        ItemKind::Use => {
            parts.push(
                "(use_declaration argument: (use_as_clause alias: (identifier) @name)) @item",
            );
            parts.push(
                "(use_declaration argument: (scoped_identifier name: (identifier) @name)) @item",
            );
            parts.push("(use_declaration argument: (identifier) @name) @item");
        }
    };

    if let Some(k) = kind {
        add(&mut parts, k);
    } else {
        // Catch-all: all kinds except Use (reduces noise)
        for k in [
            ItemKind::Fn,
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::Trait,
            ItemKind::Field,
            ItemKind::Type,
            ItemKind::Impl,
            ItemKind::Macro,
            ItemKind::Const,
        ] {
            add(&mut parts, k);
        }
    }

    parts.join("\n")
}

// ── Name matching ────────────────────────────────────────────────────

/// Smart-case: all-lowercase pattern → case-insensitive.
fn is_case_sensitive(pattern: &str) -> bool {
    pattern.chars().any(|c| c.is_uppercase())
}

fn name_matches(captured: &str, pattern: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        captured == pattern
    } else {
        captured.eq_ignore_ascii_case(pattern)
    }
}

// ── File collection ──────────────────────────────────────────────────

fn collect_source_files(source_root: &Path, src_only: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let dirs: &[&str] = if src_only {
        &["src"]
    } else {
        &["src", "examples", "tests", "benches"]
    };
    for dir_name in dirs {
        let dir = source_root.join(dir_name);
        if dir.is_dir() {
            files.extend(examples::collect_rs_files(&dir, 999));
        }
    }
    files.sort();
    files
}

// ── Module context derivation ────────────────────────────────────────

/// Derive module path from file path relative to source root.
/// `lib.rs`/`main.rs` → empty. `src/foo/bar.rs` → `foo::bar`.
/// `src/foo/mod.rs` → `foo`.
fn derive_module_path(file_path: &Path, source_root: &Path) -> String {
    let rel = file_path.strip_prefix(source_root).unwrap_or(file_path);

    // Strip leading src/ if present
    let rel = rel.strip_prefix("src").unwrap_or(rel);

    let s = rel.to_string_lossy();
    let s = s.strip_suffix(".rs").unwrap_or(&s);

    // mod.rs → use parent dir
    let s = s
        .strip_suffix("/mod")
        .or_else(|| s.strip_suffix("\\mod"))
        .unwrap_or(s);

    // lib / main at root → empty
    if s == "lib" || s == "main" || s == "/lib" || s == "/main" || s == "\\lib" || s == "\\main" {
        return String::new();
    }

    // Strip leading separator
    let s = s
        .strip_prefix('/')
        .or_else(|| s.strip_prefix('\\'))
        .unwrap_or(s);

    s.replace(['/', '\\'], "::")
}

/// Walk up from a node collecting inline `mod_item` ancestor names (reversed for top-down order).
fn collect_inline_module_names(node: tree_sitter::Node, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item"
            && let Some(name_node) = parent.child_by_field_name("name")
        {
            names.push(source[name_node.start_byte()..name_node.end_byte()].to_string());
        }
        current = parent.parent();
    }
    names.reverse();
    names
}

/// Build full module context: `crate_name::file_module::inline_modules`.
fn build_module_context(
    crate_name: &str,
    file_path: &Path,
    source_root: &Path,
    node: tree_sitter::Node,
    source: &str,
) -> String {
    let file_mod = derive_module_path(file_path, source_root);
    let inline_mods = collect_inline_module_names(node, source);

    let mut path = String::from(crate_name);
    if !file_mod.is_empty() {
        path.push_str("::");
        path.push_str(&file_mod);
    }
    if !inline_mods.is_empty() {
        path.push_str("::");
        path.push_str(&inline_mods.join("::"));
    }
    path
}

// ── Parent context ───────────────────────────────────────────────────

/// Walk up from node to find nearest impl/trait/struct/enum parent.
/// Returns a display string like `impl Commands<'w, 's'>` or `trait Plugin`.
fn find_parent_context(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "impl_item" => {
                // Extract: `impl [Trait for] Type`
                let trait_part = parent
                    .child_by_field_name("trait")
                    .map(|t| &source[t.start_byte()..t.end_byte()]);
                let type_part = parent
                    .child_by_field_name("type")
                    .map(|t| &source[t.start_byte()..t.end_byte()]);
                return match (trait_part, type_part) {
                    (Some(tr), Some(ty)) => Some(format!("impl {tr} for {ty}")),
                    (None, Some(ty)) => Some(format!("impl {ty}")),
                    _ => None,
                };
            }
            "trait_item" => {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = &source[name_node.start_byte()..name_node.end_byte()];
                    return Some(format!("trait {name}"));
                }
            }
            "struct_item" => {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = &source[name_node.start_byte()..name_node.end_byte()];
                    return Some(format!("struct {name}"));
                }
            }
            "enum_item" => {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = &source[name_node.start_byte()..name_node.end_byte()];
                    return Some(format!("enum {name}"));
                }
            }
            // Skip intermediate nodes (declaration_list, etc.) and keep walking up
            "mod_item" => return None, // stop at module boundary
            _ => {}
        }
        current = parent.parent();
    }
    None
}

// ── Parent type extraction ────────────────────────────────────────────

/// Extract the type name from the nearest parent impl/trait/struct/enum.
/// Returns the bare type identifier (e.g., "Commands" from "impl Commands<'w>").
fn parent_type_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "impl_item" => {
                let type_node = parent.child_by_field_name("type")?;
                return extract_type_identifier(type_node, source);
            }
            "trait_item" | "struct_item" | "enum_item" => {
                let name_node = parent.child_by_field_name("name")?;
                return Some(source[name_node.start_byte()..name_node.end_byte()].to_string());
            }
            "mod_item" => return None,
            _ => {}
        }
        current = parent.parent();
    }
    None
}

/// Extract the type_identifier from a type node, handling generic_type
/// and scoped_type_identifier wrappers.
fn extract_type_identifier(node: tree_sitter::Node, source: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" => Some(source[node.start_byte()..node.end_byte()].to_string()),
        "generic_type" => {
            let type_node = node.child_by_field_name("type")?;
            extract_type_identifier(type_node, source)
        }
        "scoped_type_identifier" => {
            let name_node = node.child_by_field_name("name")?;
            Some(source[name_node.start_byte()..name_node.end_byte()].to_string())
        }
        _ => None,
    }
}

// ── Limit parsing ────────────────────────────────────────────────────

fn parse_limit(raw: Option<&str>) -> (usize, Option<usize>) {
    let Some(raw) = raw else {
        return (0, None);
    };
    if let Some((offset_str, limit_str)) = raw.split_once(':') {
        (
            offset_str.parse().unwrap_or(0),
            Some(limit_str.parse().unwrap_or(0)),
        )
    } else {
        (0, Some(raw.parse().unwrap_or(0)))
    }
}

// ── Main search function ─────────────────────────────────────────────

/// Search source files for code definitions matching kind and name.
///
/// `sources`: list of `(crate_name, source_root)` pairs to scan.
pub fn search_code(
    sources: &[(String, PathBuf)],
    name: &str,
    kind: Option<ItemKind>,
    args: &CodeArgs,
    in_type: Option<&str>,
) -> Result<String> {
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let query_src = build_query(kind);
    let query = Query::new(&language, &query_src)
        .map_err(|e| anyhow::anyhow!("Failed to compile tree-sitter query: {e}"))?;

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| anyhow::anyhow!("Failed to set tree-sitter language: {e}"))?;

    let capture_names = query.capture_names().to_vec();
    let name_idx = capture_names
        .iter()
        .position(|n| *n == "name")
        .expect("query must have @name capture") as u32;
    let item_idx = capture_names
        .iter()
        .position(|n| *n == "item")
        .expect("query must have @item capture") as u32;

    let case_sensitive = is_case_sensitive(name);
    let (offset, limit) = parse_limit(args.limit.as_deref());

    let mut output = String::new();
    let mut match_count = 0usize;
    let mut emitted = 0usize;

    // Pre-compute lowercase name for insensitive grep pre-filter
    let name_lower = name.to_ascii_lowercase();

    'outer: for (crate_name, source_root) in sources {
        let files = collect_source_files(source_root, args.src_only);

        for file_path in &files {
            let source = match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Grep pre-filter: skip files that don't contain the name
            let contains = if case_sensitive {
                source.contains(name)
            } else {
                source.to_ascii_lowercase().contains(&name_lower)
            };
            if !contains {
                continue;
            }

            let Some(tree) = parser.parse(&source, None) else {
                continue;
            };

            let root = tree.root_node();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, root, source.as_bytes());

            while let Some(query_match) = matches.next() {
                // Find @name and @item captures
                let name_node = query_match.captures.iter().find(|c| c.index == name_idx);
                let item_node = query_match.captures.iter().find(|c| c.index == item_idx);

                let (Some(name_cap), Some(item_cap)) = (name_node, item_node) else {
                    continue;
                };

                let captured_name = &source[name_cap.node.start_byte()..name_cap.node.end_byte()];
                if !name_matches(captured_name, name, case_sensitive) {
                    continue;
                }

                // --in filter: only items inside matching parent type
                if let Some(filter_type) = in_type {
                    let filter_case_sensitive = is_case_sensitive(filter_type);
                    match parent_type_name(item_cap.node, &source) {
                        Some(ref parent_name) => {
                            if !name_matches(parent_name, filter_type, filter_case_sensitive) {
                                continue;
                            }
                        }
                        None => continue,
                    }
                }

                match_count += 1;
                if match_count <= offset {
                    continue;
                }
                if let Some(n) = limit
                    && emitted >= n
                {
                    break 'outer;
                }
                emitted += 1;

                let item_node = item_cap.node;
                let start_line = item_node.start_position().row + 1;
                let rel = file_path.strip_prefix(source_root).unwrap_or(file_path);

                // Module context
                let mod_ctx =
                    build_module_context(crate_name, file_path, source_root, item_node, &source);

                // Parent context
                let parent_ctx = find_parent_context(item_node, &source);

                // Format output
                if !output.is_empty() {
                    output.push('\n');
                }

                output.push_str(&format!("@{}:{}\n", rel.display(), start_line));

                // Context line: `  in module[, parent]`
                output.push_str("  in ");
                output.push_str(&mod_ctx);
                if let Some(ref ctx) = parent_ctx {
                    output.push_str(", ");
                    output.push_str(ctx);
                }
                output.push('\n');

                if !args.quiet {
                    output.push('\n');
                    let text = &source[item_node.start_byte()..item_node.end_byte()];
                    output.push_str(text);
                    if !text.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
        }
    }

    if match_count == 0 {
        let kind_str = kind.map_or("", |k| k.keyword());
        if kind_str.is_empty() {
            output.push_str(&format!("// no definitions found for '{name}'\n"));
        } else {
            output.push_str(&format!(
                "// no {kind_str} definitions found for '{name}'\n"
            ));
        }
    }

    Ok(output)
}

// ── Reference (grep) search ──────────────────────────────────────────

fn digit_count(mut n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

/// Grep source files for literal occurrences of `name`.
pub fn search_references(
    sources: &[(String, PathBuf)],
    name: &str,
    src_only: bool,
    quiet: bool,
    limit: Option<&str>,
) -> String {
    let case_sensitive = is_case_sensitive(name);
    let name_lower = name.to_ascii_lowercase();
    let (offset, limit_n) = parse_limit(limit);

    let ctx_lines: usize = 2;
    let mut output = String::new();
    let mut total_matches = 0usize;
    let mut emitted = 0usize;

    'outer: for (_crate_name, source_root) in sources {
        let files = collect_source_files(source_root, src_only);

        for file_path in &files {
            let content = match std::fs::read_to_string(file_path) {
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
                        line.contains(name)
                    } else {
                        line.to_ascii_lowercase().contains(&name_lower)
                    }
                })
                .map(|(i, _)| i)
                .collect();

            if matches.is_empty() {
                continue;
            }

            let rel = file_path
                .strip_prefix(source_root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .replace('\\', "/");

            if quiet {
                for &m in &matches {
                    total_matches += 1;
                    if total_matches <= offset {
                        continue;
                    }
                    if let Some(n) = limit_n
                        && emitted >= n
                    {
                        break 'outer;
                    }
                    emitted += 1;
                    output.push_str(&format!("@{}:{}\n", rel, m + 1));
                }
            } else {
                // Determine which matches survive offset/limit
                let mut file_match_indices: Vec<usize> = Vec::new();
                for &m in &matches {
                    total_matches += 1;
                    if total_matches <= offset {
                        continue;
                    }
                    if let Some(n) = limit_n
                        && emitted >= n
                    {
                        break;
                    }
                    emitted += 1;
                    file_match_indices.push(m);
                }

                if file_match_indices.is_empty() {
                    if let Some(n) = limit_n
                        && emitted >= n
                    {
                        break 'outer;
                    }
                    continue;
                }

                // Compute context ranges and merge overlapping
                let mut ranges: Vec<(usize, usize)> = Vec::new();
                for &m in &file_match_indices {
                    let start = m.saturating_sub(ctx_lines);
                    let end = (m + ctx_lines).min(total.saturating_sub(1));
                    if let Some(last) = ranges.last_mut()
                        && start <= last.1 + 1
                    {
                        last.1 = last.1.max(end);
                        continue;
                    }
                    ranges.push((start, end));
                }

                // Line number column width
                let max_line_no = ranges.last().map_or(1, |r| r.1 + 1);
                let width = digit_count(max_line_no).max(4);

                output.push_str(&format!("@{rel}\n"));

                let match_set: std::collections::HashSet<usize> =
                    file_match_indices.iter().copied().collect();

                for (range_idx, &(start, end)) in ranges.iter().enumerate() {
                    if range_idx > 0 {
                        output.push_str("  ...\n");
                    }
                    for (i, line) in lines.iter().enumerate().take(end + 1).skip(start) {
                        let line_no = i + 1;
                        let marker = if match_set.contains(&i) { '*' } else { ' ' };
                        output.push_str(&format!(
                            "{marker}{line_no:>width$}:  {line}\n",
                            width = width,
                        ));
                    }
                }

                output.push('\n');

                if let Some(n) = limit_n
                    && emitted >= n
                {
                    break 'outer;
                }
            }
        }
    }

    if total_matches == 0 {
        output.push_str(&format!("// no references found for '{name}'\n"));
    }

    output
}
