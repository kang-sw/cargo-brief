/// A parsed cfg predicate extracted from an `Attribute::Other` string.
///
/// `Feature` carries a concrete feature name. `NonFeature` carries the raw
/// name/value so mixed predicates (e.g. `all(feature = "x", unix)`) round-trip.
#[derive(Debug, PartialEq)]
pub enum CfgPredicate {
    Feature(String),
    NonFeature { name: String, value: Option<String> },
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

/// Parse a raw `Attribute::Other` string into a `CfgPredicate` tree.
///
/// Returns `None` on any parse failure — callers should fall back to emitting the
/// raw string rather than crashing or silently dropping the attribute.
///
/// Expected wrapper format (from nightly HIR debug serialization):
/// `"#[attr = CfgTrace([<pred>])]"`
pub fn parse_cfg_attribute(raw: &str) -> Option<CfgPredicate> {
    // Strip outer wrapper: `#[attr = CfgTrace([` ... `])]`
    let inner = raw
        .strip_prefix("#[attr = CfgTrace([")?
        .strip_suffix("])]")?;

    let (pred, rest) = parse_pred(inner.trim())?;
    if rest.trim().is_empty() {
        Some(pred)
    } else {
        None
    }
}

/// Reconstruct a `#[cfg(...)]` attribute string from a predicate tree.
///
/// Total — every variant round-trips, so callers never need a fallback for
/// shape reasons (only for upstream parse failure).
pub fn reconstruct_cfg_attr(pred: &CfgPredicate) -> String {
    let mut inner = String::new();
    write_predicate(pred, &mut inner);
    format!("#[cfg({inner})]")
}

fn write_predicate(pred: &CfgPredicate, out: &mut String) {
    match pred {
        CfgPredicate::Feature(f) => out.push_str(&format!("feature = \"{f}\"")),
        CfgPredicate::NonFeature { name, value: None } => out.push_str(name),
        CfgPredicate::NonFeature { name, value: Some(v) } => {
            out.push_str(&format!("{name} = \"{v}\""))
        }
        CfgPredicate::Not(inner) => {
            out.push_str("not(");
            write_predicate(inner, out);
            out.push(')');
        }
        CfgPredicate::All(ps) => write_list("all", ps, out),
        CfgPredicate::Any(ps) => write_list("any", ps, out),
    }
}

fn write_list(combinator: &str, preds: &[CfgPredicate], out: &mut String) {
    out.push_str(combinator);
    out.push('(');
    for (i, p) in preds.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_predicate(p, out);
    }
    out.push(')');
}

// ── Parser internals ──────────────────────────────────────────────────────────

/// Parse one predicate from `s`, returning `(pred, remaining_input)`.
fn parse_pred(s: &str) -> Option<(CfgPredicate, &str)> {
    if let Some(rest) = s.strip_prefix("All([") {
        let (preds, rest) = parse_pred_list(rest)?;
        let rest = rest.trim_start();
        // consume ", span)"
        let rest = consume_span_suffix(rest)?;
        return Some((CfgPredicate::All(preds), rest));
    }
    if let Some(rest) = s.strip_prefix("Any([") {
        let (preds, rest) = parse_pred_list(rest)?;
        let rest = rest.trim_start();
        let rest = consume_span_suffix(rest)?;
        return Some((CfgPredicate::Any(preds), rest));
    }
    if let Some(rest) = s.strip_prefix("Not(") {
        let (inner, rest) = parse_pred(rest.trim_start())?;
        let rest = rest.trim_start();
        let rest = consume_span_suffix(rest)?;
        return Some((CfgPredicate::Not(Box::new(inner)), rest));
    }
    if s.starts_with("NameValue") {
        return parse_namevalue(s);
    }
    None
}

/// Consume `, <span_text>)` after a predicate list or Not-inner.
///
/// The span text may itself contain balanced parentheses (e.g., `(#0)`), so we use
/// a depth counter: the first `)` encountered at depth 0 is the enclosing predicate's
/// closing paren.
fn consume_span_suffix(s: &str) -> Option<&str> {
    let s = s.strip_prefix(',')?.trim_start();
    let mut depth: i32 = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(&s[i + 1..]);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Parse a comma-separated list of predicates until the closing `]`.
fn parse_pred_list(mut s: &str) -> Option<(Vec<CfgPredicate>, &str)> {
    let mut preds = Vec::new();

    loop {
        s = s.trim_start();
        if s.starts_with(']') {
            return Some((preds, &s[1..]));
        }
        if !preds.is_empty() {
            // consume comma separator
            s = s.strip_prefix(',')?.trim_start();
        }
        let (pred, rest) = parse_pred(s)?;
        preds.push(pred);
        s = rest.trim_start();
    }
}

/// Parse `NameValue { name: "<name>", value: <val>, span: ... }` and return
/// `(Feature(x) | NonFeature { .. }, remaining)`.
fn parse_namevalue(s: &str) -> Option<(CfgPredicate, &str)> {
    let s = s.strip_prefix("NameValue {")?;
    let s = s.trim_start();

    // name: "<ident>"
    let s = s.strip_prefix("name: \"")?;
    let (name, s) = split_at_unescaped_quote(s)?;
    let s = s.strip_prefix('"')?;

    // skip to value field: ", value: "
    let s = skip_to_field(s, "value:")?;
    let s = s.trim_start();

    let (value, s) = parse_option_value(s)?;

    // skip the rest of the struct up to the closing }
    let close = s.find('}')?;
    let rest = &s[close + 1..];

    let pred = if name == "feature" {
        match value {
            Some(v) => CfgPredicate::Feature(v),
            None => CfgPredicate::NonFeature { name: "feature".to_string(), value: None },
        }
    } else {
        CfgPredicate::NonFeature { name: name.to_string(), value }
    };

    Some((pred, rest))
}

/// Parse `Some("<value>")` or `None`.
fn parse_option_value(s: &str) -> Option<(Option<String>, &str)> {
    if let Some(s) = s.strip_prefix("None") {
        return Some((None, s));
    }
    let s = s.strip_prefix("Some(\"")?;
    let (val, s) = split_at_unescaped_quote(s)?;
    let s = s.strip_prefix('"')?;
    let s = s.strip_prefix(')')?;
    Some((Some(val.to_string()), s))
}

/// Advance past the next occurrence of `field_name` (including trailing whitespace).
fn skip_to_field<'a>(s: &'a str, field_name: &str) -> Option<&'a str> {
    let pos = s.find(field_name)?;
    Some(&s[pos + field_name.len()..])
}

/// Split at the first unescaped `"`, returning `(before, after_quote_char)`.
fn split_at_unescaped_quote(s: &str) -> Option<(&str, &str)> {
    let pos = s.find('"')?;
    Some((&s[..pos], &s[pos..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helpers to build the HIR debug strings used in tests
    fn nv(name: &str, value: Option<&str>) -> String {
        let val = match value {
            Some(v) => format!("Some(\"{v}\")"),
            None => "None".to_string(),
        };
        format!("NameValue {{ name: \"{name}\", value: {val}, span: <dummy> }}")
    }

    fn wrap(inner: &str) -> String {
        format!("#[attr = CfgTrace([{inner}])]")
    }

    #[test]
    fn simple_feature() {
        let raw = wrap(&nv("feature", Some("net")));
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(pred, CfgPredicate::Feature("net".to_string()));
        assert_eq!(reconstruct_cfg_attr(&pred), "#[cfg(feature = \"net\")]");
    }

    #[test]
    fn non_feature_namevalue() {
        let raw = wrap(&nv("unix", None));
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(
            pred,
            CfgPredicate::NonFeature { name: "unix".to_string(), value: None }
        );
        assert_eq!(reconstruct_cfg_attr(&pred), "#[cfg(unix)]");
    }

    #[test]
    fn non_feature_namevalue_with_value() {
        let raw = wrap(&nv("target_os", Some("linux")));
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(
            pred,
            CfgPredicate::NonFeature { name: "target_os".to_string(), value: Some("linux".to_string()) }
        );
        assert_eq!(reconstruct_cfg_attr(&pred), "#[cfg(target_os = \"linux\")]");
    }

    #[test]
    fn all_two_features() {
        let inner = format!("All([{}, {}], span)", nv("feature", Some("net")), nv("feature", Some("tls")));
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(all(feature = \"net\", feature = \"tls\"))]"
        );
    }

    #[test]
    fn all_three_features() {
        let inner = format!(
            "All([{}, {}, {}], span)",
            nv("feature", Some("a")),
            nv("feature", Some("b")),
            nv("feature", Some("c"))
        );
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(all(feature = \"a\", feature = \"b\", feature = \"c\"))]"
        );
    }

    #[test]
    fn any_two_features() {
        let inner = format!("Any([{}, {}], span)", nv("feature", Some("a")), nv("feature", Some("b")));
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(any(feature = \"a\", feature = \"b\"))]"
        );
    }

    #[test]
    fn any_three_features() {
        let inner = format!(
            "Any([{}, {}, {}], span)",
            nv("feature", Some("a")),
            nv("feature", Some("b")),
            nv("feature", Some("c"))
        );
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(any(feature = \"a\", feature = \"b\", feature = \"c\"))]"
        );
    }

    #[test]
    fn not_feature() {
        let inner = format!("Not({}, span)", nv("feature", Some("beta")));
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(reconstruct_cfg_attr(&pred), "#[cfg(not(feature = \"beta\"))]");
    }

    #[test]
    fn mixed_all_reconstructs() {
        // all(feature = "net", unix) — NonFeature inside All now reconstructs cleanly
        let inner = format!("All([{}, {}], span)", nv("feature", Some("net")), nv("unix", None));
        let pred = parse_cfg_attribute(&wrap(&inner)).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(all(feature = \"net\", unix))]"
        );
    }

    #[test]
    fn malformed_no_wrapper_returns_none() {
        assert!(parse_cfg_attribute("not a cfg attr").is_none());
    }

    #[test]
    fn truncated_returns_none() {
        // Missing closing "])]"
        assert!(parse_cfg_attribute("#[attr = CfgTrace([NameValue {").is_none());
    }

    // Tests with the real nightly span format (contains "(#0)" inside the span field).
    fn real_nv(name: &str, value: Option<&str>, file: &str, lo: &str, hi: &str) -> String {
        let val = match value {
            Some(v) => format!("Some(\"{v}\")"),
            None => "None".to_string(),
        };
        format!("NameValue {{ name: \"{name}\", value: {val}, span: {file}:{lo}: {hi} (#0) }}")
    }

    #[test]
    fn real_span_simple_feature() {
        let inner = real_nv("feature", Some("net"), "src/lib.rs", "1:1", "1:10");
        let raw = format!("#[attr = CfgTrace([{inner}])]");
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(pred, CfgPredicate::Feature("net".to_string()));
        assert_eq!(reconstruct_cfg_attr(&pred), "#[cfg(feature = \"net\")]");
    }

    #[test]
    fn real_span_any_two_features() {
        let a = real_nv("feature", Some("extra"), "src/lib.rs", "1:15", "1:32");
        let b = real_nv("feature", Some("experimental"), "src/lib.rs", "1:34", "1:58");
        let inner = format!("Any([{a}, {b}], src/lib.rs:1:14: 1:59 (#0))");
        let raw = format!("#[attr = CfgTrace([{inner}])]");
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(any(feature = \"extra\", feature = \"experimental\"))]"
        );
    }

    #[test]
    fn real_span_not_feature() {
        let nv = real_nv("feature", Some("experimental"), "src/lib.rs", "1:15", "1:39");
        let inner = format!("Not({nv}, src/lib.rs:1:14: 1:40 (#0))");
        let raw = format!("#[attr = CfgTrace([{inner}])]");
        let pred = parse_cfg_attribute(&raw).unwrap();
        assert_eq!(
            reconstruct_cfg_attr(&pred),
            "#[cfg(not(feature = \"experimental\"))]"
        );
    }
}
