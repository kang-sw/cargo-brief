pub mod cli;
pub mod cross_crate;
pub mod model;
pub mod remote;
pub mod render;
pub mod resolve;
pub mod rustdoc_json;
pub mod search;

/// Clean cached remote crate workspaces. Empty spec = all.
pub fn clean_cache(spec: &str) -> anyhow::Result<()> {
    remote::clean_cache(spec)
}

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use rustdoc_types::{Id, ItemEnum, Visibility};

use cli::BriefArgs;
use model::{CrateModel, compute_reachable_set};

/// Result of glob re-export expansion. Contains both the item names (for Phase 1
/// individual `pub use` lines) and the full source models (for Phase 2 inlining).
struct GlobExpansionResult {
    /// Phase 1 data: source crate → sorted list of public item names
    item_names: HashMap<String, Vec<String>>,
    /// Phase 2 data: source crate → full CrateModel
    source_models: HashMap<String, CrateModel>,
}

/// Run the cargo-brief pipeline and return the rendered output string.
pub fn run_pipeline(args: &BriefArgs) -> Result<String> {
    // --methods-of: translate into --search + exclusion flags
    if let Some(type_name) = &args.methods_of {
        let mut args = args.clone();
        args.search = Some(type_name.clone());
        args.no_structs = true;
        args.no_enums = true;
        args.no_traits = true;
        args.no_unions = true;
        args.no_constants = true;
        args.no_macros = true;
        args.no_aliases = true;
        args.methods_of = None;
        // Leave no_functions = false (methods are functions)
        return run_pipeline(&args);
    }

    if let Some(spec) = &args.crates {
        return run_remote_pipeline(args, spec);
    }

    // Step 0: Load cargo metadata and resolve target
    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let resolved =
        resolve::resolve_target(&args.crate_name, args.module_path.as_deref(), &metadata)
            .context("Failed to resolve target")?;

    // Step 1: Generate rustdoc JSON
    let json_path = rustdoc_json::generate_rustdoc_json(
        &resolved.package_name,
        &args.toolchain,
        args.manifest_path.as_deref(),
        true, // always document private items for visibility filtering
        &metadata.target_dir,
    )
    .with_context(|| {
        format!(
            "Failed to generate rustdoc JSON for crate '{}'",
            resolved.package_name
        )
    })?;

    // Step 2: Parse JSON
    let krate = rustdoc_json::parse_rustdoc_json(&json_path)
        .with_context(|| format!("Failed to parse rustdoc JSON at '{}'", json_path.display()))?;

    // Step 3: Build model
    let model = CrateModel::from_crate(krate);

    // Step 4: Determine if observer is in the same crate
    let observer_crate = args
        .at_package
        .as_deref()
        .or(metadata.current_package.as_deref());
    let same_crate = match observer_crate {
        Some(obs) => obs == resolved.package_name || obs.replace('-', "_") == model.crate_name(),
        None => false,
    };
    let reachable = if !same_crate {
        Some(compute_reachable_set(&model))
    } else {
        None
    };

    // Step 5: Render (search mode or normal mode)
    if let Some(pattern) = &args.search {
        let output = search::render_search(
            &model,
            pattern,
            args,
            args.at_mod.as_deref(),
            same_crate,
            reachable.as_ref(),
        );
        return Ok(output);
    }

    let mut output = render::render_module_api(
        &model,
        resolved.module_path.as_deref(),
        args,
        args.at_mod.as_deref(),
        same_crate,
        reachable.as_ref(),
    );

    // Step 6: Expand glob re-exports
    // The renderer outputs `pub use source::*;` for glob re-exports.
    // Replace each with either individual `pub use` lines (default) or
    // full inlined definitions (--expand-glob).
    let result = expand_glob_reexports(
        &model,
        resolved.module_path.as_deref(),
        &args.toolchain,
        args.manifest_path.as_deref(),
        &metadata.target_dir,
    );

    apply_glob_expansions(&mut output, &result, args);

    Ok(output)
}

/// Run the pipeline for a remote crate fetched via a cached or temp workspace.
fn run_remote_pipeline(args: &BriefArgs, spec: &str) -> Result<String> {
    // When --crates is used, the first positional arg (crate_name) may actually be
    // a module path (e.g., `cargo brief --crates bevy ecs` → crate_name="ecs").
    // Merge it into module_path if it's not "self".
    let module_path = if args.crate_name != "self" && args.module_path.is_none() {
        Some(args.crate_name.clone())
    } else {
        args.module_path.clone()
    };

    let (name, _) = remote::parse_crate_spec(spec);
    let workspace = remote::resolve_workspace(spec, args.features.as_deref(), args.no_cache)
        .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    let manifest_path = workspace
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();

    let metadata = resolve::load_cargo_metadata(Some(&manifest_path))
        .context("Failed to load cargo metadata for remote crate")?;

    let json_path = rustdoc_json::generate_rustdoc_json_cached(
        &name,
        &args.toolchain,
        Some(&manifest_path),
        true, // include private modules so facade crate internals are visible
        &metadata.target_dir,
    )
    .with_context(|| format!("Failed to generate rustdoc JSON for remote crate '{name}'"))?;

    let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path)?;
    let model = CrateModel::from_crate(krate);
    let reachable = Some(compute_reachable_set(&model));
    let has_cross_crate = cross_crate::root_has_cross_crate_reexports(&model);

    // Build enriched crate header with version + features
    let crate_header = build_remote_crate_header(&name, workspace.path(), args.features.as_deref());

    // --- Search mode ---
    let mut output = if let Some(pattern) = &args.search {
        let mut output =
            search::render_search(&model, pattern, args, None, false, reachable.as_ref());

        // Cross-crate search: discover sub-crates, search each
        if has_cross_crate {
            let sub_crates = cross_crate::discover_all_reexported_crates(
                &model,
                &args.toolchain,
                Some(&manifest_path),
                &metadata.target_dir,
            );
            for sub in &sub_crates {
                let sub_reachable = Some(compute_reachable_set(&sub.model));
                let sub_output = search::render_search(
                    &sub.model,
                    pattern,
                    args,
                    None,
                    false,
                    sub_reachable.as_ref(),
                );
                output.push_str(&sub_output);
            }
        }

        output
    } else if let Some(ref module_path) = module_path {
        // --- Module targeting with cross-crate resolution ---
        if model.find_module(module_path).is_some() {
            // Found locally — render normally
            render_remote_normal(
                &model,
                Some(module_path),
                args,
                &manifest_path,
                &metadata.target_dir,
                reachable.as_ref(),
            )?
        } else if let Some(resolution) = cross_crate::resolve_cross_crate_module(
            &model,
            module_path,
            &args.toolchain,
            Some(&manifest_path),
            &metadata.target_dir,
        ) {
            // Cross-crate resolved: use sub-crate model
            let sub_reachable = Some(compute_reachable_set(&resolution.model));
            let mut output = render::render_module_api(
                &resolution.model,
                resolution.inner_module_path.as_deref(),
                args,
                None,
                false,
                sub_reachable.as_ref(),
            );

            let result = expand_glob_reexports(
                &resolution.model,
                resolution.inner_module_path.as_deref(),
                &args.toolchain,
                Some(&manifest_path),
                &metadata.target_dir,
            );
            apply_glob_expansions(&mut output, &result, args);
            output
        } else {
            // Fall through to normal render (will produce "module not found" error)
            render_remote_normal(
                &model,
                Some(module_path),
                args,
                &manifest_path,
                &metadata.target_dir,
                reachable.as_ref(),
            )?
        }
    } else if args.recursive && has_cross_crate {
        // --- Recursive mode with cross-crate expansion ---
        let mut output = render::render_module_api(
            &model,
            module_path.as_deref(),
            args,
            None,
            false,
            reachable.as_ref(),
        );

        let result = expand_glob_reexports(
            &model,
            module_path.as_deref(),
            &args.toolchain,
            Some(&manifest_path),
            &metadata.target_dir,
        );
        apply_glob_expansions(&mut output, &result, args);

        let sub_crates = cross_crate::discover_all_reexported_crates(
            &model,
            &args.toolchain,
            Some(&manifest_path),
            &metadata.target_dir,
        );
        for sub in &sub_crates {
            let sub_reachable = Some(compute_reachable_set(&sub.model));
            let sub_output = render::render_module_api(
                &sub.model,
                None,
                args,
                None,
                false,
                sub_reachable.as_ref(),
            );
            output.push_str(&format!(
                "\n// --- module {} (from sub-crate {}) ---\n",
                sub.display_name,
                sub.model.crate_name()
            ));
            output.push_str(&sub_output);
        }

        output
    } else {
        // --- Normal mode ---
        render_remote_normal(
            &model,
            module_path.as_deref(),
            args,
            &manifest_path,
            &metadata.target_dir,
            reachable.as_ref(),
        )?
    };

    // Enrich the first `// crate <name>` header with version + features
    if let Some(header) = &crate_header {
        if let Some(first_newline) = output.find('\n') {
            let first_line = &output[..first_newline];
            if first_line.starts_with("// crate ") {
                output.replace_range(..first_newline, header);
            }
        }
    }

    Ok(output)
}

/// Build an enriched `// crate name[version] features = [...]` header for remote crates.
/// Returns None if version cannot be determined.
fn build_remote_crate_header(
    crate_name: &str,
    workspace_dir: &Path,
    features: Option<&str>,
) -> Option<String> {
    let version = remote::resolve_crate_version(workspace_dir, crate_name)?;
    let mut header = format!("// crate {crate_name}[{version}]");
    if let Some(feats) = features {
        let feat_list: Vec<&str> = feats.split(',').map(|s| s.trim()).collect();
        let formatted = feat_list
            .iter()
            .map(|f| format!("\"{f}\""))
            .collect::<Vec<_>>()
            .join(", ");
        header.push_str(&format!(" features = [{formatted}]"));
    }
    Some(header)
}

/// Render a remote crate in normal (non-search, non-cross-crate) mode.
fn render_remote_normal(
    model: &CrateModel,
    module_path: Option<&str>,
    args: &BriefArgs,
    manifest_path: &str,
    target_dir: &Path,
    reachable: Option<&HashSet<Id>>,
) -> Result<String> {
    let mut output = render::render_module_api(model, module_path, args, None, false, reachable);

    let result = expand_glob_reexports(
        model,
        module_path,
        &args.toolchain,
        Some(manifest_path),
        target_dir,
    );
    apply_glob_expansions(&mut output, &result, args);

    Ok(output)
}

/// Apply glob expansion results to the rendered output.
fn apply_glob_expansions(output: &mut String, result: &GlobExpansionResult, args: &BriefArgs) {
    if args.expand_glob && !result.source_models.is_empty() {
        // Phase 2: inline full definitions from source crates
        let mut seen_names = HashSet::new();
        for (source, source_model) in &result.source_models {
            let rendered = render::render_inlined_items(source_model, args, &mut seen_names);
            let pattern = format!("pub use {source}::*;");
            replace_glob_lines(output, &pattern, &rendered);
        }
    } else if !result.item_names.is_empty() {
        // Phase 1: individual pub use lines
        for (source, items) in &result.item_names {
            let pattern = format!("pub use {source}::*;");
            let mut replacement = String::new();
            for name in items {
                replacement.push_str(&format!("pub use {source}::{name};\n"));
            }
            replace_glob_lines(output, &pattern, &replacement);
        }
    }
}

/// Find and replace all lines whose normalized content matches `pattern`.
///
/// Normalization: trim whitespace, collapse multiple spaces.
/// Replacement lines inherit the original line's indentation.
fn replace_glob_lines(output: &mut String, pattern: &str, replacement: &str) {
    loop {
        let Some((start, end, indent)) = find_normalized_line(output, pattern) else {
            break;
        };
        let indented: String = replacement
            .lines()
            .map(|l| {
                if l.is_empty() {
                    "\n".to_string()
                } else {
                    format!("{indent}{l}\n")
                }
            })
            .collect();
        output.replace_range(start..end, &indented);
    }
}

/// Find the first line in `text` whose trimmed, space-collapsed content equals `pattern`.
/// Returns `(start_byte, end_byte, indent_str)`.
fn find_normalized_line(text: &str, pattern: &str) -> Option<(usize, usize, String)> {
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.len() + 1; // +1 for '\n'
        let normalized: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized == pattern {
            let indent = &line[..line.len() - line.trim_start().len()];
            return Some((start, end.min(text.len()), indent.to_string()));
        }
        start = end;
    }
    None
}

/// Detect glob re-exports in the target module and expand each by generating
/// rustdoc JSON for the source crate and enumerating its public items.
///
/// Returns both item names (for Phase 1 `pub use` lines) and source models
/// (for Phase 2 full definition inlining).
fn expand_glob_reexports(
    model: &CrateModel,
    target_module_path: Option<&str>,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
) -> GlobExpansionResult {
    let target_item = if let Some(path) = target_module_path {
        model.find_module(path)
    } else {
        model.root_module()
    };

    let Some(target_item) = target_item else {
        return GlobExpansionResult {
            item_names: HashMap::new(),
            source_models: HashMap::new(),
        };
    };

    let mut item_names = HashMap::new();
    let mut source_models = HashMap::new();

    for (_id, child) in model.module_children(target_item) {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if !use_item.is_glob {
            continue;
        }

        let source = &use_item.source;

        // Generate JSON for the source crate (pub items only, no private items)
        let Ok(json_path) = rustdoc_json::generate_rustdoc_json(
            source,
            toolchain,
            manifest_path,
            false,
            target_dir,
        ) else {
            continue;
        };
        let Ok(source_krate) = rustdoc_json::parse_rustdoc_json(&json_path) else {
            continue;
        };

        let source_model = CrateModel::from_crate(source_krate);
        let Some(root) = source_model.root_module() else {
            continue;
        };

        let mut items: Vec<String> = source_model
            .module_children(root)
            .iter()
            .filter(|(_, item)| matches!(item.visibility, Visibility::Public))
            .filter(|(_, item)| !matches!(item.inner, ItemEnum::Module(_)))
            .filter_map(|(_, item)| {
                // Use items store their name in inner.use.name, not item.name
                item.name.clone().or_else(|| {
                    if let ItemEnum::Use(u) = &item.inner {
                        Some(u.name.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();

        items.sort();
        items.dedup();
        item_names.insert(source.clone(), items);
        source_models.insert(source.clone(), source_model);
    }

    GlobExpansionResult {
        item_names,
        source_models,
    }
}
