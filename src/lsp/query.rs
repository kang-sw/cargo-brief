//! Symbol resolution and reference queries via rust-analyzer.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};

use super::transport::RaTransport;

pub struct SymbolMatch {
    pub name: String,
    pub container_name: Option<String>,
    pub uri: String,
    pub line: u32,
    pub col: u32,
    pub kind: String,
}

pub struct ReferenceLocation {
    pub uri: String,
    pub line: u32,
    #[allow(dead_code)]
    pub col: u32,
}

pub enum ResolveResult {
    Ok(SymbolMatch),
    Ambiguous(Vec<SymbolMatch>),
    NotFound,
}

fn symbol_kind_label(kind: i64) -> &'static str {
    match kind {
        5 => "struct",
        6 => "fn",
        10 => "enum",
        11 => "trait",
        12 => "fn", // method
        13 => "const",
        _ => "symbol",
    }
}

/// Resolve a symbol query to a single match, multiple matches, or not found.
pub fn resolve_symbol(transport: &mut RaTransport, query: &str) -> Result<ResolveResult> {
    let params = serde_json::json!({ "query": query });
    let response = transport.send_request_and_wait("workspace/symbol", params)?;
    let results = response["result"].as_array();

    let results = match results {
        Some(arr) if !arr.is_empty() => arr,
        _ => return Ok(ResolveResult::NotFound),
    };

    // Extract last :: segment for exact name matching
    let name_filter = query.rsplit("::").next().unwrap_or(query);
    let container_filter = if query.contains("::") {
        Some(&query[..query.rfind("::").unwrap()])
    } else {
        None
    };

    let mut matches: Vec<SymbolMatch> = results
        .iter()
        .filter_map(|item| {
            let name = item["name"].as_str()?;
            if name != name_filter {
                return None;
            }

            if let Some(cf) = container_filter {
                let container = item["containerName"].as_str().unwrap_or("");
                if !container.contains(cf) {
                    return None;
                }
            }

            let kind = item["kind"].as_i64().unwrap_or(0);
            let location = &item["location"];
            let uri = location["uri"].as_str()?;
            let start = &location["range"]["start"];
            let line = start["line"].as_u64()? as u32;
            let col = start["character"].as_u64()? as u32;

            Some(SymbolMatch {
                name: name.to_string(),
                container_name: item["containerName"].as_str().map(|s| s.to_string()),
                uri: uri.to_string(),
                line,
                col,
                kind: symbol_kind_label(kind).to_string(),
            })
        })
        .collect();

    match matches.len() {
        0 => Ok(ResolveResult::NotFound),
        1 => Ok(ResolveResult::Ok(matches.remove(0))),
        _ => Ok(ResolveResult::Ambiguous(matches)),
    }
}

/// Find all references to a symbol at a given position.
pub fn find_references(
    transport: &mut RaTransport,
    uri: &str,
    line: u32,
    col: u32,
) -> Result<Vec<ReferenceLocation>> {
    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": col },
        "context": { "includeDeclaration": false }
    });

    let response = transport.send_request_and_wait("textDocument/references", params)?;
    let results = response["result"].as_array();

    let refs = match results {
        Some(arr) => arr
            .iter()
            .filter_map(|loc| {
                let uri = loc["uri"].as_str()?;
                let start = &loc["range"]["start"];
                let line = start["line"].as_u64()? as u32;
                let col = start["character"].as_u64()? as u32;
                Some(ReferenceLocation {
                    uri: uri.to_string(),
                    line,
                    col,
                })
            })
            .collect(),
        None => Vec::new(),
    };

    Ok(refs)
}

/// Format references for display. `workspace_root` is used to make paths relative.
pub fn format_references(
    refs: &[ReferenceLocation],
    workspace_root: &Path,
    symbol_name: &str,
    quiet: bool,
) -> String {
    if quiet {
        return format_references_quiet(refs, workspace_root);
    }

    let mut out = format!("// {} references to {symbol_name}\n", refs.len());

    if refs.is_empty() {
        return out;
    }

    // Group by file, preserving insertion order via BTreeMap on relative path
    let mut by_file: BTreeMap<String, Vec<&ReferenceLocation>> = BTreeMap::new();
    for r in refs {
        let rel = uri_to_relative(workspace_root, &r.uri);
        by_file.entry(rel).or_default().push(r);
    }

    for (rel_path, mut file_refs) in by_file {
        file_refs.sort_by_key(|r| r.line);

        // Read source file once
        let abs_path = workspace_root.join(&rel_path);
        let lines = read_source_lines(&abs_path);

        // Compute line number display width for this group
        let max_line = file_refs.iter().map(|r| r.line + 1).max().unwrap_or(1);
        let width = max_line.to_string().len();

        out.push_str(&format!("\n// {rel_path}\n"));
        for r in &file_refs {
            let display_line = r.line + 1; // 0-indexed → 1-indexed
            let content = lines
                .as_ref()
                .and_then(|ls| ls.get(r.line as usize))
                .map(|s| s.as_str())
                .unwrap_or("<source unavailable>");
            out.push_str(&format!("{display_line:>width$}:  {content}\n"));
        }
    }

    out
}

fn format_references_quiet(refs: &[ReferenceLocation], workspace_root: &Path) -> String {
    let mut out = String::new();
    for r in refs {
        let rel = uri_to_relative(workspace_root, &r.uri);
        let display_line = r.line + 1;
        out.push_str(&format!("@{rel}:{display_line}\n"));
    }
    out
}

/// Format disambiguation list for ambiguous symbol matches.
pub fn format_disambiguation(
    matches: &[SymbolMatch],
    query: &str,
    workspace_root: &Path,
) -> String {
    let mut out = format!("Multiple symbols match \"{query}\":\n");
    for (i, m) in matches.iter().enumerate() {
        let qualified = match &m.container_name {
            Some(c) => format!("{c}::{}", m.name),
            None => m.name.clone(),
        };
        let rel_path = uri_to_relative(workspace_root, &m.uri);
        let display_line = m.line + 1;
        out.push_str(&format!(
            "  {}. {} {qualified}  {rel_path}:{display_line}\n",
            i + 1,
            m.kind
        ));
    }
    out
}

/// Orchestrator: resolve symbol → find references → format output.
pub fn handle_references(
    transport: &mut RaTransport,
    workspace_root: &Path,
    symbol: &str,
    quiet: bool,
) -> Result<String> {
    match resolve_symbol(transport, symbol)? {
        ResolveResult::NotFound => {
            bail!("Symbol not found: {symbol}")
        }
        ResolveResult::Ambiguous(matches) => {
            Ok(format_disambiguation(&matches, symbol, workspace_root))
        }
        ResolveResult::Ok(m) => {
            let refs = find_references(transport, &m.uri, m.line, m.col)?;
            Ok(format_references(&refs, workspace_root, &m.name, quiet))
        }
    }
}

fn uri_to_relative(workspace_root: &Path, uri: &str) -> String {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    let path = Path::new(path);
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn read_source_lines(path: &Path) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.lines().map(|l| l.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_references_empty() {
        let result = format_references(&[], Path::new("/project"), "Foo", false);
        assert_eq!(result, "// 0 references to Foo\n");
    }

    #[test]
    fn format_references_quiet_mode() {
        let refs = vec![
            ReferenceLocation {
                uri: "file:///project/src/main.rs".to_string(),
                line: 41,
                col: 5,
            },
            ReferenceLocation {
                uri: "file:///project/src/lib.rs".to_string(),
                line: 9,
                col: 0,
            },
        ];
        let result = format_references(&refs, Path::new("/project"), "Foo", true);
        assert_eq!(result, "@src/main.rs:42\n@src/lib.rs:10\n");
    }

    #[test]
    fn format_references_grouped_by_file() {
        let refs = vec![
            ReferenceLocation {
                uri: "file:///project/src/a.rs".to_string(),
                line: 9,
                col: 0,
            },
            ReferenceLocation {
                uri: "file:///project/src/b.rs".to_string(),
                line: 19,
                col: 0,
            },
            ReferenceLocation {
                uri: "file:///project/src/a.rs".to_string(),
                line: 49,
                col: 0,
            },
        ];
        let result = format_references(&refs, Path::new("/project"), "Bar", false);
        // Should have header + two file groups
        assert!(result.starts_with("// 3 references to Bar\n"));
        assert!(result.contains("// src/a.rs\n"));
        assert!(result.contains("// src/b.rs\n"));
        // Lines should be 1-indexed
        assert!(result.contains("10:"));
        assert!(result.contains("50:"));
        assert!(result.contains("20:"));
    }

    #[test]
    fn format_references_line_padding() {
        let refs = vec![
            ReferenceLocation {
                uri: "file:///project/src/a.rs".to_string(),
                line: 0,
                col: 0,
            },
            ReferenceLocation {
                uri: "file:///project/src/a.rs".to_string(),
                line: 99,
                col: 0,
            },
        ];
        let result = format_references(&refs, Path::new("/project"), "X", false);
        // Line 1 should be padded to width of 100 (3 chars)
        assert!(result.contains("  1:  <source unavailable>"));
        assert!(result.contains("100:  <source unavailable>"));
    }

    #[test]
    fn format_disambiguation_two_matches() {
        let matches = vec![
            SymbolMatch {
                name: "bar".to_string(),
                container_name: Some("Foo".to_string()),
                uri: "file:///project/src/foo.rs".to_string(),
                line: 41,
                col: 0,
                kind: "fn".to_string(),
            },
            SymbolMatch {
                name: "bar".to_string(),
                container_name: Some("Baz".to_string()),
                uri: "file:///project/src/baz.rs".to_string(),
                line: 9,
                col: 0,
                kind: "fn".to_string(),
            },
        ];
        let result = format_disambiguation(&matches, "bar", Path::new("/project"));
        assert!(result.starts_with("Multiple symbols match \"bar\":\n"));
        assert!(result.contains("1. fn Foo::bar  src/foo.rs:42"));
        assert!(result.contains("2. fn Baz::bar  src/baz.rs:10"));
    }

    #[test]
    fn format_disambiguation_no_container() {
        let matches = vec![
            SymbolMatch {
                name: "Config".to_string(),
                container_name: None,
                uri: "file:///project/src/config.rs".to_string(),
                line: 0,
                col: 0,
                kind: "struct".to_string(),
            },
            SymbolMatch {
                name: "Config".to_string(),
                container_name: Some("app".to_string()),
                uri: "file:///project/src/app.rs".to_string(),
                line: 5,
                col: 0,
                kind: "struct".to_string(),
            },
        ];
        let result = format_disambiguation(&matches, "Config", Path::new("/project"));
        assert!(result.contains("1. struct Config  src/config.rs:1"));
        assert!(result.contains("2. struct app::Config  src/app.rs:6"));
    }
}
