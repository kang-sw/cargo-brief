use anyhow::Result;

/// The feature graph for a single crate — populated from `cargo metadata` or crates.io payload.
pub struct FeatureGraph {
    pub crate_name: String,
    /// Features in the `default` group (expanded names).
    pub defaults: Vec<String>,
    /// All named features in alphabetical order: (feature_name, enables_list).
    pub features: Vec<(String, Vec<String>)>,
    /// Names of features that are purely optional-dep aliases (value is a single `dep:*` entry).
    pub optional_dep_features: Vec<String>,
}

impl FeatureGraph {
    pub fn is_valid_feature(&self, name: &str) -> bool {
        self.features.iter().any(|(f, _)| f == name)
    }

    pub fn feature_names(&self) -> impl Iterator<Item = &str> {
        self.features.iter().map(|(f, _)| f.as_str())
    }
}

/// Render a FeatureGraph as pseudo-TOML mirroring a `[features]` section.
pub fn render_features(graph: &FeatureGraph) -> String {
    let mut out = String::new();

    out.push_str("[features]\n");

    // default = [...] first
    out.push_str("default = [");
    if graph.defaults.is_empty() {
        out.push_str("]\n");
    } else {
        let items: Vec<String> = graph.defaults.iter().map(|d| format!("\"{d}\"")).collect();
        out.push_str(&items.join(", "));
        out.push_str("]\n");
    }

    // Named features alphabetically
    let optional_set: std::collections::HashSet<&str> =
        graph.optional_dep_features.iter().map(|s| s.as_str()).collect();

    for (name, enables) in &graph.features {
        if name == "default" {
            continue;
        }
        let is_opt_dep = optional_set.contains(name.as_str());
        let items: Vec<String> = enables.iter().map(|e| format!("\"{e}\"")).collect();
        let rhs = if items.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", items.join(", "))
        };
        if is_opt_dep {
            out.push_str(&format!("{name} = {rhs} # optional dep\n"));
        } else {
            out.push_str(&format!("{name} = {rhs}\n"));
        }
    }

    out
}

/// Build a `FeatureGraph` from the raw features map extracted from `cargo metadata`
/// for a single package.
///
/// `raw_features`: the `packages[].features` JSON object (feature_name → [enables]).
pub fn build_feature_graph(
    crate_name: String,
    raw_features: &serde_json::Value,
) -> FeatureGraph {
    let mut features: Vec<(String, Vec<String>)> = Vec::new();
    let mut defaults: Vec<String> = Vec::new();
    let mut optional_dep_features: Vec<String> = Vec::new();

    if let Some(map) = raw_features.as_object() {
        for (name, enables_val) in map {
            let enables: Vec<String> = enables_val
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if name == "default" {
                defaults = enables.clone();
            } else {
                // A feature is an optional-dep alias if all enables are `dep:*` entries.
                let all_dep = !enables.is_empty()
                    && enables.iter().all(|e| e.starts_with("dep:"));
                if all_dep {
                    optional_dep_features.push(name.clone());
                }
            }
            features.push((name.clone(), enables));
        }
    }

    // Sort alphabetically (serde_json Map without preserve_order gives BTreeMap order,
    // but normalise here regardless of source ordering).
    features.sort_by(|a, b| a.0.cmp(&b.0));
    optional_dep_features.sort();

    FeatureGraph {
        crate_name,
        defaults,
        features,
        optional_dep_features,
    }
}

/// Validate each feature name in a comma-separated list against a FeatureGraph.
///
/// On any unknown feature, returns an error with Jaro-Winkler-ranked did-you-mean
/// suggestions (top 3 above 0.6 threshold) and the full valid-feature list.
pub fn validate_requested_features(graph: &FeatureGraph, requested: &str) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    for raw in requested.split(',') {
        let feat = raw.trim();
        if feat.is_empty() {
            continue;
        }
        if graph.is_valid_feature(feat) {
            continue;
        }

        let mut ranked: Vec<(&str, f64)> = graph
            .feature_names()
            .map(|f| (f, strsim::jaro_winkler(feat, f)))
            .filter(|(_, score)| *score >= 0.6)
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(3);

        let mut msg = format!("unknown feature '{feat}' for crate '{}'", graph.crate_name);
        if !ranked.is_empty() {
            let suggestions: Vec<&str> = ranked.iter().map(|(f, _)| *f).collect();
            msg.push_str(&format!("\n  did you mean: {}", suggestions.join(", ")));
        }
        let all_names: Vec<&str> = graph.feature_names().collect();
        msg.push_str(&format!("\n  valid features: {}", all_names.join(", ")));

        errors.push(msg);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{}", errors.join("\n\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph(name: &str, pairs: &[(&str, &[&str])]) -> FeatureGraph {
        let mut features = Vec::new();
        let mut defaults = Vec::new();
        let mut optional_dep_features = Vec::new();
        for (feat, enables) in pairs {
            let enables: Vec<String> = enables.iter().map(|s| s.to_string()).collect();
            if *feat == "default" {
                defaults = enables.clone();
            } else {
                let all_dep = !enables.is_empty() && enables.iter().all(|e| e.starts_with("dep:"));
                if all_dep {
                    optional_dep_features.push(feat.to_string());
                }
            }
            features.push((feat.to_string(), enables));
        }
        features.sort_by(|a, b| a.0.cmp(&b.0));
        optional_dep_features.sort();
        FeatureGraph {
            crate_name: name.to_string(),
            defaults,
            features,
            optional_dep_features,
        }
    }

    #[test]
    fn is_valid_feature_true() {
        let g = make_graph("foo", &[("full", &[]), ("async", &[])]);
        assert!(g.is_valid_feature("full"));
        assert!(g.is_valid_feature("async"));
    }

    #[test]
    fn is_valid_feature_false() {
        let g = make_graph("foo", &[("full", &[])]);
        assert!(!g.is_valid_feature("typo"));
    }

    #[test]
    fn feature_names_iter() {
        let g = make_graph("foo", &[("b", &[]), ("a", &[])]);
        let names: Vec<&str> = g.feature_names().collect();
        // sorted alphabetically
        assert_eq!(names, &["a", "b"]);
    }

    #[test]
    fn render_empty_features() {
        let g = make_graph("foo", &[]);
        let out = render_features(&g);
        assert!(out.contains("[features]"));
        assert!(out.contains("default = []"));
    }

    #[test]
    fn render_with_defaults_and_features() {
        let g = make_graph(
            "foo",
            &[("default", &["full"]), ("full", &["a", "b"]), ("a", &[])],
        );
        let out = render_features(&g);
        assert!(out.contains("default = [\"full\"]"));
        assert!(out.contains("full = [\"a\", \"b\"]"));
        assert!(out.contains("a = []"));
    }

    #[test]
    fn render_optional_dep_comment() {
        let g = make_graph("foo", &[("serde", &["dep:serde"]), ("net", &[])]);
        let out = render_features(&g);
        assert!(out.contains("serde = [\"dep:serde\"] # optional dep"));
        assert!(!out.contains("net = [\"dep:net\"] # optional dep"));
    }

    #[test]
    fn validate_valid_feature_succeeds() {
        let g = make_graph("foo", &[("full", &[]), ("async", &[])]);
        assert!(validate_requested_features(&g, "full").is_ok());
        assert!(validate_requested_features(&g, "full,async").is_ok());
    }

    #[test]
    fn validate_unknown_feature_errors() {
        let g = make_graph("foo", &[("full", &[]), ("async", &[])]);
        let err = validate_requested_features(&g, "typo").unwrap_err().to_string();
        assert!(err.contains("unknown feature 'typo'"), "{err}");
        assert!(err.contains("valid features:"), "{err}");
    }

    #[test]
    fn validate_typo_with_suggestion() {
        let g = make_graph("foo", &[("derive", &[]), ("std", &[])]);
        let err = validate_requested_features(&g, "deriev").unwrap_err().to_string();
        assert!(err.contains("did you mean"), "{err}");
        assert!(err.contains("derive"), "{err}");
    }

    #[test]
    fn validate_no_suggestion_below_threshold() {
        let g = make_graph("foo", &[("alpha", &[]), ("beta", &[])]);
        let err = validate_requested_features(&g, "zzznomatch").unwrap_err().to_string();
        assert!(!err.contains("did you mean"), "should have no suggestions: {err}");
        assert!(err.contains("valid features:"), "{err}");
    }

    #[test]
    fn validate_empty_string_ok() {
        let g = make_graph("foo", &[("full", &[])]);
        assert!(validate_requested_features(&g, "").is_ok());
    }

    #[test]
    fn build_feature_graph_from_json() {
        let json = serde_json::json!({
            "default": ["full"],
            "full": ["a", "b"],
            "a": [],
            "serde": ["dep:serde"]
        });
        let g = build_feature_graph("mycrate".to_string(), &json);
        assert_eq!(g.crate_name, "mycrate");
        assert_eq!(g.defaults, &["full"]);
        assert!(g.is_valid_feature("full"));
        assert!(g.is_valid_feature("a"));
        assert!(g.optional_dep_features.contains(&"serde".to_string()));
    }
}
