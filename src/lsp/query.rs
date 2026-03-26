//! Symbol resolution and reference queries via rust-analyzer.

use std::collections::{BTreeMap, HashSet, VecDeque};
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

/// Orchestrator: resolve symbol → prepare call hierarchy → incoming/outgoing → format.
pub fn handle_call_hierarchy(
    transport: &mut RaTransport,
    workspace_root: &Path,
    symbol: &str,
    outgoing: bool,
    quiet: bool,
) -> Result<String> {
    let m = match resolve_symbol(transport, symbol)? {
        ResolveResult::NotFound => bail!("Symbol not found: {symbol}"),
        ResolveResult::Ambiguous(matches) => {
            return Ok(format_disambiguation(&matches, symbol, workspace_root));
        }
        ResolveResult::Ok(m) => m,
    };

    let items = prepare_call_hierarchy(transport, &m.uri, m.line, m.col)?;
    if items.is_empty() {
        return Ok(format!("No call hierarchy found for {symbol}\n"));
    }

    let item = &items[0];
    let calls = if outgoing {
        outgoing_calls(transport, item)?
    } else {
        incoming_calls(transport, item)?
    };

    Ok(format_call_hierarchy(
        &calls,
        workspace_root,
        symbol,
        outgoing,
        quiet,
    ))
}

/// Format call hierarchy results for display.
pub fn format_call_hierarchy(
    calls: &[serde_json::Value],
    workspace_root: &Path,
    symbol: &str,
    outgoing: bool,
    quiet: bool,
) -> String {
    if quiet {
        return format_call_hierarchy_quiet(calls, workspace_root, outgoing);
    }

    let direction = if outgoing {
        "Outgoing calls from"
    } else {
        "Incoming calls to"
    };
    let arrow = if outgoing { "→" } else { "←" };
    let mut out = format!("// {direction} {symbol}\n//\n");

    if calls.is_empty() {
        out.push_str("// (none)\n");
        return out;
    }

    // Collect entries for column alignment
    let entries: Vec<(String, String)> = calls
        .iter()
        .filter_map(|call| {
            let item = if outgoing { &call["to"] } else { &call["from"] };
            let name = item["name"].as_str()?;
            let uri = item["uri"].as_str()?;
            let line = item["selectionRange"]["start"]["line"].as_u64()?;
            let rel = uri_to_relative(workspace_root, uri);
            Some((format!("{name}()"), format!("{rel}:{}", line + 1)))
        })
        .collect();

    let max_name = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, loc) in &entries {
        out.push_str(&format!("// {arrow} {name:<max_name$}  {loc}\n"));
    }

    out
}

fn format_call_hierarchy_quiet(
    calls: &[serde_json::Value],
    workspace_root: &Path,
    outgoing: bool,
) -> String {
    let mut out = String::new();
    for call in calls {
        let item = if outgoing { &call["to"] } else { &call["from"] };
        if let (Some(name), Some(uri), Some(line)) = (
            item["name"].as_str(),
            item["uri"].as_str(),
            item["selectionRange"]["start"]["line"].as_u64(),
        ) {
            let rel = uri_to_relative(workspace_root, uri);
            out.push_str(&format!("@{rel}:{}  {name}\n", line + 1));
        }
    }
    out
}

/// Orchestrator: resolve symbol → prepare → BFS incoming calls up to depth.
pub fn handle_blast_radius(
    transport: &mut RaTransport,
    workspace_root: &Path,
    symbol: &str,
    depth: u32,
    quiet: bool,
) -> Result<String> {
    let depth = depth.clamp(1, 10);

    let m = match resolve_symbol(transport, symbol)? {
        ResolveResult::NotFound => bail!("Symbol not found: {symbol}"),
        ResolveResult::Ambiguous(matches) => {
            return Ok(format_disambiguation(&matches, symbol, workspace_root));
        }
        ResolveResult::Ok(m) => m,
    };

    let items = prepare_call_hierarchy(transport, &m.uri, m.line, m.col)?;
    if items.is_empty() {
        return Ok(format!("No call hierarchy found for {symbol}\n"));
    }

    // BFS: collect callers at each depth level
    // Key: (uri, selectionRange.start.line) for dedup
    let mut seen: HashSet<(String, u64)> = HashSet::new();
    // Each level: Vec<CallerEntry>
    let mut levels: Vec<Vec<CallerEntry>> = Vec::new();
    // BFS queue: (item_json, depth_level, parent_name)
    let mut queue: VecDeque<(serde_json::Value, u32, Option<String>)> = VecDeque::new();

    // Seed with the target item(s)
    let root_item = &items[0];
    // Mark the root itself as seen so it doesn't appear as its own caller
    if let (Some(uri), Some(line)) = (
        root_item["uri"].as_str(),
        root_item["selectionRange"]["start"]["line"].as_u64(),
    ) {
        seen.insert((uri.to_string(), line));
    }
    queue.push_back((root_item.clone(), 0, None));

    while let Some((item, current_depth, parent_name)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }

        let callers = incoming_calls(transport, &item)?;
        for call in &callers {
            let from = &call["from"];
            let uri = match from["uri"].as_str() {
                Some(u) => u.to_string(),
                None => continue,
            };
            let line = match from["selectionRange"]["start"]["line"].as_u64() {
                Some(l) => l,
                None => continue,
            };

            let key = (uri.clone(), line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let name = from["name"].as_str().unwrap_or("?").to_string();
            let rel = uri_to_relative(workspace_root, &uri);
            let level = current_depth as usize;

            // Extend levels vec if needed
            while levels.len() <= level {
                levels.push(Vec::new());
            }

            levels[level].push(CallerEntry {
                name: name.clone(),
                location: format!("{rel}:{}", line + 1),
                via: parent_name.clone(),
            });

            // Enqueue for deeper traversal
            queue.push_back((from.clone(), current_depth + 1, Some(name)));
        }
    }

    Ok(format_blast_radius(&levels, symbol, quiet))
}

pub(crate) struct CallerEntry {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) via: Option<String>,
}

/// Format blast radius results.
pub fn format_blast_radius(levels: &[Vec<CallerEntry>], symbol: &str, quiet: bool) -> String {
    if quiet {
        return format_blast_radius_quiet(levels);
    }

    let direct = levels.first().map(|l| l.len()).unwrap_or(0);
    let transitive: usize = levels.iter().skip(1).map(|l| l.len()).sum();
    let total = direct + transitive;

    let mut out = if transitive > 0 {
        format!("// Blast radius for {symbol} ({direct} direct, {transitive} transitive)\n")
    } else {
        format!("// Blast radius for {symbol} ({total} direct)\n")
    };

    if levels.is_empty() || total == 0 {
        out.push_str("//\n// (no callers found)\n");
        return out;
    }

    for (i, level) in levels.iter().enumerate() {
        if level.is_empty() {
            continue;
        }
        let label = if i == 0 {
            "Direct".to_string()
        } else {
            format!("Depth {}", i + 1)
        };
        out.push_str(&format!("//\n// {label}:\n"));

        // Column alignment
        let max_name = level.iter().map(|e| e.name.len() + 2).max().unwrap_or(0); // +2 for "()"
        for entry in level {
            let name_display = format!("{}()", entry.name);
            if let Some(via) = &entry.via {
                out.push_str(&format!(
                    "//   {name_display:<max_name$}  {}  → {via}()\n",
                    entry.location
                ));
            } else {
                out.push_str(&format!(
                    "//   {name_display:<max_name$}  {}\n",
                    entry.location
                ));
            }
        }
    }

    out
}

fn format_blast_radius_quiet(levels: &[Vec<CallerEntry>]) -> String {
    let mut out = String::new();
    for (i, level) in levels.iter().enumerate() {
        for entry in level {
            out.push_str(&format!(
                "@{}  {}  [depth={}]\n",
                entry.location,
                entry.name,
                i + 1
            ));
        }
    }
    out
}

/// Send `callHierarchy/prepare` and return raw CallHierarchyItem array.
pub fn prepare_call_hierarchy(
    transport: &mut RaTransport,
    uri: &str,
    line: u32,
    col: u32,
) -> Result<Vec<serde_json::Value>> {
    let params = serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": col }
    });
    let response = transport.send_request_and_wait("textDocument/prepareCallHierarchy", params)?;
    Ok(response["result"].as_array().cloned().unwrap_or_default())
}

/// Send `callHierarchy/incomingCalls` for a CallHierarchyItem.
pub fn incoming_calls(
    transport: &mut RaTransport,
    item: &serde_json::Value,
) -> Result<Vec<serde_json::Value>> {
    let params = serde_json::json!({ "item": item });
    let response = transport.send_request_and_wait("callHierarchy/incomingCalls", params)?;
    Ok(response["result"].as_array().cloned().unwrap_or_default())
}

/// Send `callHierarchy/outgoingCalls` for a CallHierarchyItem.
pub fn outgoing_calls(
    transport: &mut RaTransport,
    item: &serde_json::Value,
) -> Result<Vec<serde_json::Value>> {
    let params = serde_json::json!({ "item": item });
    let response = transport.send_request_and_wait("callHierarchy/outgoingCalls", params)?;
    Ok(response["result"].as_array().cloned().unwrap_or_default())
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

    fn mock_call_hierarchy_item(name: &str, uri: &str, line: u64) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": 12,
            "uri": uri,
            "range": { "start": { "line": line, "character": 0 }, "end": { "line": line + 10, "character": 0 } },
            "selectionRange": { "start": { "line": line, "character": 4 }, "end": { "line": line, "character": 4 + name.len() } }
        })
    }

    fn mock_incoming_call(from_name: &str, uri: &str, line: u64) -> serde_json::Value {
        serde_json::json!({
            "from": mock_call_hierarchy_item(from_name, uri, line),
            "fromRanges": [{ "start": { "line": line + 5, "character": 8 }, "end": { "line": line + 5, "character": 20 } }]
        })
    }

    fn mock_outgoing_call(to_name: &str, uri: &str, line: u64) -> serde_json::Value {
        serde_json::json!({
            "to": mock_call_hierarchy_item(to_name, uri, line),
            "fromRanges": [{ "start": { "line": 10, "character": 8 }, "end": { "line": 10, "character": 20 } }]
        })
    }

    #[test]
    fn format_call_hierarchy_incoming() {
        let calls = vec![
            mock_incoming_call("run_pipeline", "file:///project/src/pipeline.rs", 41),
            mock_incoming_call("render_item", "file:///project/src/render.rs", 114),
        ];
        let result = format_call_hierarchy(&calls, Path::new("/project"), "Foo::bar", false, false);
        assert!(result.starts_with("// Incoming calls to Foo::bar\n"));
        assert!(result.contains("← run_pipeline()"));
        assert!(result.contains("src/pipeline.rs:42"));
        assert!(result.contains("← render_item()"));
        assert!(result.contains("src/render.rs:115"));
    }

    #[test]
    fn format_call_hierarchy_outgoing() {
        let calls = vec![
            mock_outgoing_call("resolve_path", "file:///project/src/resolve.rs", 22),
            mock_outgoing_call("lookup", "file:///project/src/model.rs", 155),
        ];
        let result = format_call_hierarchy(&calls, Path::new("/project"), "Foo::bar", true, false);
        assert!(result.starts_with("// Outgoing calls from Foo::bar\n"));
        assert!(result.contains("→ resolve_path()"));
        assert!(result.contains("→ lookup()"));
    }

    #[test]
    fn format_call_hierarchy_empty() {
        let result = format_call_hierarchy(&[], Path::new("/project"), "Foo::bar", false, false);
        assert!(result.contains("Incoming calls to Foo::bar"));
        assert!(result.contains("(none)"));
    }

    #[test]
    fn format_call_hierarchy_quiet() {
        let calls = vec![
            mock_incoming_call("run_pipeline", "file:///project/src/pipeline.rs", 41),
            mock_incoming_call("render_item", "file:///project/src/render.rs", 114),
        ];
        let result = format_call_hierarchy(&calls, Path::new("/project"), "X", false, true);
        assert_eq!(
            result,
            "@src/pipeline.rs:42  run_pipeline\n@src/render.rs:115  render_item\n"
        );
    }

    #[test]
    fn format_blast_radius_depth_one() {
        let levels = vec![vec![
            CallerEntry {
                name: "run_pipeline".to_string(),
                location: "src/pipeline.rs:42".to_string(),
                via: None,
            },
            CallerEntry {
                name: "search_index".to_string(),
                location: "src/search.rs:67".to_string(),
                via: None,
            },
        ]];
        let result = format_blast_radius(&levels, "Foo::bar", false);
        assert!(result.contains("Blast radius for Foo::bar (2 direct)"));
        assert!(result.contains("Direct:"));
        assert!(result.contains("run_pipeline()"));
        assert!(result.contains("search_index()"));
    }

    #[test]
    fn format_blast_radius_depth_two() {
        let levels = vec![
            vec![CallerEntry {
                name: "run_pipeline".to_string(),
                location: "src/pipeline.rs:42".to_string(),
                via: None,
            }],
            vec![CallerEntry {
                name: "run_api_pipeline".to_string(),
                location: "src/lib.rs:89".to_string(),
                via: Some("run_pipeline".to_string()),
            }],
        ];
        let result = format_blast_radius(&levels, "Foo::bar", false);
        assert!(result.contains("1 direct, 1 transitive"));
        assert!(result.contains("Direct:"));
        assert!(result.contains("Depth 2:"));
        assert!(result.contains("→ run_pipeline()"));
    }

    #[test]
    fn format_blast_radius_empty() {
        let levels: Vec<Vec<CallerEntry>> = vec![];
        let result = format_blast_radius(&levels, "Foo::bar", false);
        assert!(result.contains("(no callers found)"));
    }

    #[test]
    fn format_blast_radius_quiet() {
        let levels = vec![
            vec![CallerEntry {
                name: "run_pipeline".to_string(),
                location: "src/pipeline.rs:42".to_string(),
                via: None,
            }],
            vec![CallerEntry {
                name: "run_api".to_string(),
                location: "src/lib.rs:89".to_string(),
                via: Some("run_pipeline".to_string()),
            }],
        ];
        let result = format_blast_radius(&levels, "X", true);
        assert_eq!(
            result,
            "@src/pipeline.rs:42  run_pipeline  [depth=1]\n@src/lib.rs:89  run_api  [depth=2]\n"
        );
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
