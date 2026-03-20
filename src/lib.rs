pub mod cli;
pub mod cross_crate;
pub mod examples;
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
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustdoc_types::{Id, ItemEnum, Visibility};

use cli::{ApiArgs, ExamplesArgs, FilterArgs, SearchArgs};
use model::{CrateModel, compute_reachable_set};

/// Result of glob re-export expansion. Contains both the item names (for Phase 1
/// individual `pub use` lines) and the full source models (for Phase 2 inlining).
struct GlobExpansionResult {
    /// Phase 1 data: source crate → sorted list of public item names
    item_names: HashMap<String, Vec<String>>,
    /// Phase 2 data: source crate → full CrateModel
    source_models: HashMap<String, CrateModel>,
}

/// Shared context produced after target resolution, consumed by api/search pipelines.
struct PipelineContext {
    manifest_path: Option<String>,
    target_dir: PathBuf,
    package_name: String,
    module_path: Option<String>,
    /// Observer package for same-crate detection. None → always external view.
    observer_package: Option<String>,
    toolchain: String,
    verbose: bool,
    /// Skip cargo rustdoc if JSON exists. True for non-workspace-member crates.
    use_cache: bool,
    /// Pre-computed crate header with version + features (remote api only).
    crate_header: Option<String>,
    /// Holds the remote workspace alive (TempDir drops on scope exit).
    _workspace: Option<remote::WorkspaceDir>,
}

/// Generate rustdoc JSON, parse it (bincode-cached), build CrateModel, compute visibility.
fn generate_and_parse_model(
    ctx: &PipelineContext,
) -> Result<(CrateModel, bool, Option<HashSet<Id>>)> {
    if ctx.verbose {
        eprintln!(
            "[cargo-brief] Running cargo rustdoc for '{}'...",
            ctx.package_name
        );
    }
    let json_path = rustdoc_json::generate_rustdoc_json(
        &ctx.package_name,
        &ctx.toolchain,
        ctx.manifest_path.as_deref(),
        true, // always document private items
        &ctx.target_dir,
        ctx.verbose,
        ctx.use_cache,
    )
    .with_context(|| format!("Failed to generate rustdoc JSON for '{}'", ctx.package_name))?;

    if ctx.verbose {
        eprintln!("[cargo-brief] Parsing rustdoc JSON...");
    }
    let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path)
        .with_context(|| format!("Failed to parse rustdoc JSON at '{}'", json_path.display()))?;
    let model = CrateModel::from_crate(krate);

    let same_crate = match &ctx.observer_package {
        Some(obs) => obs == &ctx.package_name || obs.replace('-', "_") == model.crate_name(),
        None => false,
    };
    let reachable = if !same_crate {
        Some(compute_reachable_set(&model))
    } else {
        None
    };

    Ok((model, same_crate, reachable))
}

/// Run the API extraction pipeline and return the rendered output string.
pub fn run_api_pipeline(args: &ApiArgs) -> Result<String> {
    let ctx = if let Some(spec) = &args.remote.crates {
        build_remote_context_api(args, spec)?
    } else {
        build_local_context_api(args)?
    };
    run_shared_api_pipeline(&ctx, args)
}

fn build_local_context_api(args: &ApiArgs) -> Result<PipelineContext> {
    if args.global.verbose {
        eprintln!(
            "[cargo-brief] Resolving target '{}'...",
            args.target.crate_name
        );
    }
    let metadata = resolve::load_cargo_metadata(args.target.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let resolved = resolve::resolve_target(
        &args.target.crate_name,
        args.target.module_path.as_deref(),
        &metadata,
    )
    .context("Failed to resolve target")?;

    let observer_package = args
        .target
        .at_package
        .clone()
        .or(metadata.current_package.clone());

    Ok(PipelineContext {
        manifest_path: args.target.manifest_path.clone(),
        target_dir: metadata.target_dir,
        package_name: resolved.package_name,
        module_path: resolved.module_path,
        observer_package,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: false, // workspace member — always regenerate
        crate_header: None,
        _workspace: None,
    })
}

fn build_remote_context_api(args: &ApiArgs, spec: &str) -> Result<PipelineContext> {
    // When --crates is used, the first positional arg (crate_name) may actually be
    // a module path (e.g., `cargo brief api --crates bevy ecs` → crate_name="ecs").
    // Merge it into module_path if it's not "self".
    let module_path = if args.target.crate_name != "self" && args.target.module_path.is_none() {
        Some(args.target.crate_name.clone())
    } else {
        args.target.module_path.clone()
    };

    let (name, _) = remote::parse_crate_spec(spec);
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
    }
    let (workspace, resolved_version) =
        remote::resolve_workspace(spec, args.remote.features.as_deref(), args.remote.no_cache)
            .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    let manifest_path = workspace
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();

    let metadata = resolve::load_cargo_metadata(Some(&manifest_path))
        .context("Failed to load cargo metadata for remote crate")?;

    let crate_header = build_remote_crate_header(
        &name,
        resolved_version.as_deref(),
        workspace.path(),
        args.remote.features.as_deref(),
    );

    Ok(PipelineContext {
        manifest_path: Some(manifest_path),
        target_dir: metadata.target_dir,
        package_name: name,
        module_path,
        observer_package: None, // remote → always external view
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: true, // remote — versions are locked
        crate_header,
        _workspace: Some(workspace),
    })
}

fn run_shared_api_pipeline(ctx: &PipelineContext, args: &ApiArgs) -> Result<String> {
    let (model, same_crate, reachable) = generate_and_parse_model(ctx)?;
    let has_cross_crate = cross_crate::root_has_cross_crate_reexports(&model);

    let mut output = if let Some(ref module_path) = ctx.module_path {
        // Module targeting — try local first, then cross-crate resolution
        if model.find_module(module_path).is_some() {
            render_and_expand_globs(
                &model,
                Some(module_path),
                args,
                ctx,
                same_crate,
                reachable.as_ref(),
            )?
        } else {
            // Cross-crate module resolution
            if ctx.verbose {
                eprintln!(
                    "[cargo-brief] Module '{module_path}' not found locally, trying cross-crate resolution..."
                );
            }
            if let Some(resolution) = cross_crate::resolve_cross_crate_module(
                &model,
                module_path,
                &ctx.toolchain,
                ctx.manifest_path.as_deref(),
                &ctx.target_dir,
                ctx.verbose,
            ) {
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
                    &ctx.toolchain,
                    ctx.manifest_path.as_deref(),
                    &ctx.target_dir,
                    ctx.verbose,
                    ctx.use_cache,
                );
                apply_glob_expansions(&mut output, &result, args.expand_glob, &args.filter);
                output
            } else {
                // Fall through to normal render (produces "module not found" error)
                render_and_expand_globs(
                    &model,
                    Some(module_path),
                    args,
                    ctx,
                    same_crate,
                    reachable.as_ref(),
                )?
            }
        }
    } else if args.recursive && has_cross_crate {
        // Recursive mode with cross-crate expansion
        let mut output =
            render_and_expand_globs(&model, None, args, ctx, same_crate, reachable.as_ref())?;
        if ctx.verbose {
            eprintln!("[cargo-brief] Discovering cross-crate re-exports...");
        }
        let sub_crates = cross_crate::discover_all_reexported_crates(
            &model,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
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
        // Normal mode
        render_and_expand_globs(
            &model,
            ctx.module_path.as_deref(),
            args,
            ctx,
            same_crate,
            reachable.as_ref(),
        )?
    };

    // Enrich header with version + features if available
    if let Some(header) = &ctx.crate_header
        && let Some(first_newline) = output.find('\n')
    {
        let first_line = &output[..first_newline];
        if first_line.starts_with("// crate ") {
            output.replace_range(..first_newline, header);
        }
    }

    Ok(output)
}

/// Render module API + expand globs.
fn render_and_expand_globs(
    model: &CrateModel,
    module_path: Option<&str>,
    args: &ApiArgs,
    ctx: &PipelineContext,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
) -> Result<String> {
    let mut output = render::render_module_api(
        model,
        module_path,
        args,
        if same_crate {
            args.target.at_mod.as_deref()
        } else {
            None
        },
        same_crate,
        reachable,
    );
    let result = expand_glob_reexports(
        model,
        module_path,
        &ctx.toolchain,
        ctx.manifest_path.as_deref(),
        &ctx.target_dir,
        ctx.verbose,
        ctx.use_cache,
    );
    apply_glob_expansions(&mut output, &result, args.expand_glob, &args.filter);
    Ok(output)
}

/// Run the search pipeline and return the rendered output string.
pub fn run_search_pipeline(args: &SearchArgs) -> Result<String> {
    // When --crates is used, the first positional (crate_name) is redundant since
    // the crate is specified by --crates. If pattern is empty and crate_name is not
    // "self", treat crate_name as the pattern (mirrors api subcommand behavior).
    let args =
        if args.remote.crates.is_some() && args.pattern.is_empty() && args.crate_name != "self" {
            let mut args = args.clone();
            args.pattern = std::mem::take(&mut args.crate_name);
            args.crate_name = "self".to_string();
            std::borrow::Cow::Owned(args)
        } else {
            std::borrow::Cow::Borrowed(args)
        };
    let args = args.as_ref();

    // Validate: need either a pattern or --methods-of
    if args.pattern.is_empty() && args.methods_of.is_none() {
        anyhow::bail!("search requires a pattern or --methods-of <TYPE>");
    }

    // --methods-of: translate into pattern + exclusion flags
    if let Some(type_name) = &args.methods_of {
        let mut args = args.clone();
        args.pattern = type_name.clone();
        args.filter.no_structs = true;
        args.filter.no_enums = true;
        args.filter.no_traits = true;
        args.filter.no_unions = true;
        args.filter.no_constants = true;
        args.filter.no_macros = true;
        args.filter.no_aliases = true;
        args.methods_of = None;
        // Leave no_functions = false (methods are functions)
        return run_search_pipeline(&args);
    }

    let ctx = if let Some(spec) = &args.remote.crates {
        build_remote_context_search(args, spec)?
    } else {
        build_local_context_search(args)?
    };
    run_shared_search_pipeline(&ctx, args)
}

fn build_local_context_search(args: &SearchArgs) -> Result<PipelineContext> {
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving target '{}'...", args.crate_name);
    }
    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let resolved = resolve::resolve_target(&args.crate_name, None, &metadata)
        .context("Failed to resolve target")?;

    let observer_package = args.at_package.clone().or(metadata.current_package.clone());

    Ok(PipelineContext {
        manifest_path: args.manifest_path.clone(),
        target_dir: metadata.target_dir,
        package_name: resolved.package_name,
        module_path: None, // search doesn't target modules
        observer_package,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: false, // workspace member — always regenerate
        crate_header: None,
        _workspace: None,
    })
}

fn build_remote_context_search(args: &SearchArgs, spec: &str) -> Result<PipelineContext> {
    let (name, _) = remote::parse_crate_spec(spec);
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
    }
    let (workspace, _resolved_version) =
        remote::resolve_workspace(spec, args.remote.features.as_deref(), args.remote.no_cache)
            .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    let manifest_path = workspace
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();

    let metadata = resolve::load_cargo_metadata(Some(&manifest_path))
        .context("Failed to load cargo metadata for remote crate")?;

    Ok(PipelineContext {
        manifest_path: Some(manifest_path),
        target_dir: metadata.target_dir,
        package_name: name,
        module_path: None,      // search doesn't target modules
        observer_package: None, // remote → always external view
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: true, // remote — versions are locked
        crate_header: None,
        _workspace: Some(workspace),
    })
}

fn run_shared_search_pipeline(ctx: &PipelineContext, args: &SearchArgs) -> Result<String> {
    let (model, same_crate, reachable) = generate_and_parse_model(ctx)?;

    let mut output = search::render_search(
        &model,
        &args.pattern,
        &args.filter,
        args.limit.as_deref(),
        if same_crate {
            args.at_mod.as_deref()
        } else {
            None
        },
        same_crate,
        reachable.as_ref(),
    );

    // Cross-crate search: discover sub-crates, search each
    if cross_crate::root_has_cross_crate_reexports(&model) {
        if ctx.verbose {
            eprintln!("[cargo-brief] Discovering cross-crate re-exports...");
        }
        let sub_crates = cross_crate::discover_all_reexported_crates(
            &model,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
        );
        for sub in &sub_crates {
            let sub_reachable = Some(compute_reachable_set(&sub.model));
            let sub_output = search::render_search(
                &sub.model,
                &args.pattern,
                &args.filter,
                args.limit.as_deref(),
                None,
                false,
                sub_reachable.as_ref(),
            );
            output.push_str(&sub_output);
        }
    }

    Ok(output)
}

/// Run the examples pipeline and return the rendered output string.
pub fn run_examples_pipeline(args: &ExamplesArgs) -> Result<String> {
    // When --crates is used and crate_name is not "self" and no pattern,
    // treat crate_name as pattern (mirrors search subcommand behavior).
    let args =
        if args.remote.crates.is_some() && args.pattern.is_none() && args.crate_name != "self" {
            let mut args = args.clone();
            args.pattern = Some(std::mem::take(&mut args.crate_name));
            args.crate_name = "self".to_string();
            std::borrow::Cow::Owned(args)
        } else {
            std::borrow::Cow::Borrowed(args)
        };
    let args = args.as_ref();

    if let Some(spec) = &args.remote.crates {
        // Remote path
        let (name, _) = remote::parse_crate_spec(spec);
        if args.global.verbose {
            eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
        }
        let (workspace, resolved_version) =
            remote::resolve_workspace(spec, None, args.remote.no_cache)
                .with_context(|| format!("Failed to create workspace for '{name}'"))?;

        let manifest_path = workspace
            .path()
            .join("Cargo.toml")
            .to_string_lossy()
            .into_owned();

        if args.global.verbose {
            eprintln!("[cargo-brief] Finding source root for '{name}'...");
        }
        let source_root = resolve::find_dep_source_root(&manifest_path, &name)
            .with_context(|| format!("Failed to find source root for '{name}'"))?;

        let version =
            resolved_version.or_else(|| remote::resolve_crate_version(workspace.path(), &name));
        let crate_display = match version {
            Some(v) => format!("{name}[{v}]"),
            None => name.clone(),
        };

        Ok(examples::render_examples(
            &source_root,
            &crate_display,
            args,
        ))
    } else {
        // Local path
        let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
            .context("Failed to load cargo metadata")?;

        let (pkg_name, source_root) = if args.crate_name == "self" {
            let pkg = metadata.current_package.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot resolve 'self': no package found for the current directory."
                )
            })?;
            let dir = metadata
                .package_manifest_dirs
                .get(pkg)
                .cloned()
                .or(metadata.current_package_manifest_dir.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!("Cannot find manifest directory for package '{pkg}'")
                })?;
            (pkg.clone(), dir)
        } else {
            // Look up named package in workspace
            let normalized = args.crate_name.replace('-', "_");
            let found = metadata
                .package_manifest_dirs
                .iter()
                .find(|(k, _)| k.replace('-', "_") == normalized);
            match found {
                Some((name, dir)) => (name.clone(), dir.clone()),
                None => {
                    anyhow::bail!(
                        "Package '{}' not found in workspace. Available: {}",
                        args.crate_name,
                        metadata.workspace_packages.join(", ")
                    );
                }
            }
        };

        if args.global.verbose {
            eprintln!("[cargo-brief] Scanning examples for '{pkg_name}'...");
        }

        Ok(examples::render_examples(&source_root, &pkg_name, args))
    }
}

/// Build an enriched `// crate name[version] features = [...]` header for remote crates.
/// Returns None if version cannot be determined.
fn build_remote_crate_header(
    crate_name: &str,
    resolved_version: Option<&str>,
    workspace_dir: &Path,
    features: Option<&str>,
) -> Option<String> {
    let version = resolved_version
        .map(|v| v.to_string())
        .or_else(|| remote::resolve_crate_version(workspace_dir, crate_name))?;
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

/// Apply glob expansion results to the rendered output.
fn apply_glob_expansions(
    output: &mut String,
    result: &GlobExpansionResult,
    expand_glob: bool,
    filter: &FilterArgs,
) {
    if expand_glob && !result.source_models.is_empty() {
        // Phase 2: inline full definitions from source crates
        let mut seen_names = HashSet::new();
        for (source, source_model) in &result.source_models {
            let rendered = render::render_inlined_items(source_model, filter, &mut seen_names);
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
    verbose: bool,
    use_cache: bool,
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
            verbose,
            use_cache,
        ) else {
            continue;
        };
        let Ok(source_krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
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
