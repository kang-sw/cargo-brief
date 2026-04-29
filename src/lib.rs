pub mod cfg_parse;
pub mod cli;
pub mod code;
pub mod cross_crate;
pub mod examples;
pub mod features;
pub mod lsp;
pub mod model;
pub mod remote;
pub mod render;
pub mod resolve;
pub mod rustdoc_json;
pub mod search;
pub mod summary;
pub mod ts;

/// Clean cached remote crate workspaces. Empty spec = all.
pub fn clean_cache(spec: &str) -> anyhow::Result<()> {
    remote::clean_cache(spec)
}

/// Run the features pipeline and return the rendered feature graph.
pub fn run_features_pipeline(
    args: &cli::FeaturesArgs,
    remote: &cli::RemoteOpts,
) -> anyhow::Result<String> {
    use anyhow::Context;

    if remote.crates {
        let spec = &args.crate_name;
        match remote::load_remote_feature_graph(spec)? {
            Some(graph) => return Ok(features::render_features(&graph)),
            None => {
                eprintln!(
                    "warning: feature graph unavailable for '{spec}'; \
                     crates.io unreachable and no cached payload found"
                );
                anyhow::bail!("Cannot show features for '{spec}': no data available offline");
            }
        }
    }

    let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
        .context("Failed to load cargo metadata")?;

    let target = if args.crate_name == "self" {
        metadata
            .current_package
            .as_deref()
            .context("Cannot resolve 'self': no package found for current directory")?
            .to_string()
    } else {
        args.crate_name.clone()
    };

    let graph = metadata
        .feature_graphs
        .get(&target)
        .with_context(|| format!("No feature data found for crate '{target}'"))?;

    Ok(features::render_features(graph))
}

/// Run an LSP daemon management command (touch/stop/status).
pub fn run_lsp_command(args: &cli::LspArgs, remote: &cli::RemoteOpts) -> anyhow::Result<()> {
    lsp::run_lsp_command(args, remote)
}

use rustdoc_json::LockfilePackages;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rustdoc_types::{ItemEnum, Visibility};

use cli::{
    ApiArgs, CodeArgs, ExamplesArgs, FilterArgs, RemoteOpts, SearchArgs, SummaryArgs, TsArgs,
};
use model::{CrateModel, ReachableInfo, compute_reachable_set};

/// Result of glob re-export expansion. Contains both the item names (for Phase 1
/// individual `pub use` lines) and the full source models (for Phase 2 inlining).
struct GlobExpansionResult {
    /// Phase 1 data: source crate → sorted list of public item names
    item_names: HashMap<String, Vec<String>>,
    /// Phase 2 data: source crate → full CrateModels (direct + recursively discovered)
    /// Shared by both glob and named expansion — keyed by source crate name.
    source_models: HashMap<String, Vec<CrateModel>>,
    /// Named cross-crate re-exports: source crate → list of (item_name, full_source_path)
    named_reexports: HashMap<String, Vec<(String, String)>>,
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
    /// Workspace member package names. Cross-crate expansion uses `use_cache: true`
    /// for crates NOT in this set (they're external deps, effectively immutable).
    workspace_members: HashSet<String>,
    /// All resolved package names/versions from Cargo.lock (for batch validation + disambiguation).
    available_packages: LockfilePackages,
    /// Pre-computed crate header with version + features (remote api only).
    crate_header: Option<String>,
    /// Holds the remote workspace alive (TempDir drops on scope exit).
    _workspace: Option<remote::WorkspaceDir>,
}

/// Generate rustdoc JSON, parse it (bincode-cached), build CrateModel, compute visibility.
fn generate_and_parse_model(
    ctx: &PipelineContext,
) -> Result<(CrateModel, bool, Option<ReachableInfo>)> {
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
pub fn run_api_pipeline(args: &ApiArgs, remote: &RemoteOpts) -> Result<String> {
    let ctx = if remote.crates {
        let spec = &args.target.crate_name;
        build_remote_context_api(args, spec, remote)?
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

    let available_packages =
        rustdoc_json::load_lockfile_packages(args.target.manifest_path.as_deref());

    let is_workspace_member = metadata.workspace_packages.contains(&resolved.package_name);

    Ok(PipelineContext {
        manifest_path: args.target.manifest_path.clone(),
        target_dir: metadata.target_dir,
        package_name: resolved.package_name,
        module_path: resolved.module_path,
        observer_package,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: !is_workspace_member,
        workspace_members: metadata.workspace_packages.into_iter().collect(),
        available_packages,
        crate_header: None,
        _workspace: None,
    })
}

fn build_remote_context_api(
    args: &ApiArgs,
    spec: &str,
    remote: &RemoteOpts,
) -> Result<PipelineContext> {
    // With -C, crate_name IS the spec. If it contains "::", split into spec + module.
    // e.g., `bevy::ecs` → spec="bevy", module="ecs"
    //        `tokio@1::net` → spec="tokio@1", module="net"
    let (actual_spec, module_path) = if let Some(idx) = spec.find("::") {
        let rest = &spec[idx + 2..];
        let module = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
        (&spec[..idx], module)
    } else {
        (spec, args.target.module_path.clone())
    };

    let (name, _) = remote::parse_crate_spec(actual_spec);
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
    }
    let (workspace, resolved_version) = remote::resolve_workspace(
        actual_spec,
        remote.features.as_deref(),
        remote.no_default_features,
        remote.no_cache,
    )
    .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    let manifest_path = workspace
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();

    // Pre-validate -F features against the crate's feature graph.
    if let Some(requested) = remote.features.as_deref() {
        match remote::load_remote_feature_graph(actual_spec) {
            Ok(Some(graph)) => features::validate_requested_features(&graph, requested)?,
            Ok(None) => eprintln!(
                "warning: feature graph unavailable for '{name}'; \
                 -F values will not be validated and feature gates will not be annotated"
            ),
            Err(_) => {} // network failure already handled inside load_remote_feature_graph
        }
    }

    let metadata = resolve::load_cargo_metadata(Some(&manifest_path))
        .context("Failed to load cargo metadata for remote crate")?;

    let crate_header = build_remote_crate_header(
        &name,
        resolved_version.as_deref(),
        workspace.path(),
        remote.features.as_deref(),
    );

    let available_packages = rustdoc_json::load_lockfile_packages(Some(&manifest_path));

    Ok(PipelineContext {
        manifest_path: Some(manifest_path),
        target_dir: metadata.target_dir,
        package_name: name,
        module_path,
        observer_package: None, // remote → always external view
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: true,                   // remote — versions are locked
        workspace_members: HashSet::new(), // remote has no workspace
        available_packages,
        crate_header,
        _workspace: Some(workspace),
    })
}

fn run_shared_api_pipeline(ctx: &PipelineContext, args: &ApiArgs) -> Result<String> {
    let (model, same_crate, reachable) = generate_and_parse_model(ctx)?;
    let has_cross_crate = cross_crate::root_has_cross_crate_reexports(&model);
    if has_cross_crate {
        pre_warm_cross_crate_json(&model, ctx);
    }

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
                    &ctx.workspace_members,
                );
                apply_glob_expansions(&mut output, &result, !args.no_expand_glob, &args.filter);
                output
            } else {
                // Try leaf item resolution before falling through to error
                let leaf_result = if let Some((parent, leaf_name)) = module_path.rsplit_once("::") {
                    model.find_item_in_module(parent, leaf_name)
                } else {
                    model.find_item_in_module("", module_path)
                };

                if let Some((item_id, item)) = leaf_result {
                    render::render_leaf_item(
                        &model,
                        item,
                        item_id,
                        args,
                        if same_crate {
                            args.target.at_mod.as_deref()
                        } else {
                            None
                        },
                        same_crate,
                        reachable.as_ref(),
                    )
                } else {
                    // Check if parent module exists — if so, show leaf-not-found with available items
                    let (parent_path, leaf_name) =
                        if let Some((p, l)) = module_path.rsplit_once("::") {
                            (p, l)
                        } else {
                            ("", module_path.as_str())
                        };

                    let parent_exists = if parent_path.is_empty() {
                        model.root_module().is_some()
                    } else {
                        model.find_module(parent_path).is_some()
                    };

                    if parent_exists {
                        render::render_leaf_not_found(
                            &model,
                            parent_path,
                            leaf_name,
                            same_crate,
                            reachable.as_ref(),
                        )
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
            }
        }
    } else if args.recursive && has_cross_crate {
        // Recursive mode with cross-crate expansion via accessible-path index
        let mut output =
            render_and_expand_globs(&model, None, args, ctx, same_crate, reachable.as_ref())?;
        if ctx.verbose {
            eprintln!("[cargo-brief] Building cross-crate accessible path index...");
        }
        let index = cross_crate::build_cross_crate_index(
            &model,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
            &ctx.workspace_members,
            &ctx.available_packages,
        );
        let cross_output = render::render_cross_crate_api(&index, model.crate_name(), args);
        if !cross_output.is_empty() {
            output.push_str(&cross_output);
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
    reachable: Option<&ReachableInfo>,
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
        &ctx.workspace_members,
    );
    apply_glob_expansions(&mut output, &result, !args.no_expand_glob, &args.filter);
    Ok(output)
}

/// Run the search pipeline and return the rendered output string.
pub fn run_search_pipeline(args: &SearchArgs, remote: &RemoteOpts) -> Result<String> {
    // Validate: need either a pattern or --methods-of
    if args.patterns.is_empty() && args.methods_of.is_none() {
        anyhow::bail!("search requires a pattern or --methods-of <TYPE>");
    }

    // --methods-of: translate into exclusion flags, keep methods_of for exact parent matching
    if args.methods_of.is_some() {
        let mut args = args.clone();
        if args.patterns.is_empty() {
            args.patterns = vec![args.methods_of.as_ref().unwrap().clone()];
        }
        args.filter.no_structs = true;
        args.filter.no_enums = true;
        args.filter.no_traits = true;
        args.filter.no_unions = true;
        args.filter.no_constants = true;
        args.filter.no_macros = true;
        args.filter.no_aliases = true;
        // methods_of stays set — run_shared_search_pipeline uses it for exact matching
        // Leave no_functions = false (methods are functions)
        let ctx = if remote.crates {
            build_remote_context_search(&args, &args.crate_name, remote)?
        } else {
            build_local_context_search(&args)?
        };
        return run_shared_search_pipeline(&ctx, &args);
    }

    let ctx = if remote.crates {
        build_remote_context_search(args, &args.crate_name, remote)?
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

    let available_packages = rustdoc_json::load_lockfile_packages(args.manifest_path.as_deref());
    let is_workspace_member = metadata.workspace_packages.contains(&resolved.package_name);

    Ok(PipelineContext {
        manifest_path: args.manifest_path.clone(),
        target_dir: metadata.target_dir,
        package_name: resolved.package_name,
        module_path: None, // search doesn't target modules
        observer_package,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: !is_workspace_member,
        workspace_members: metadata.workspace_packages.into_iter().collect(),
        available_packages,
        crate_header: None,
        _workspace: None,
    })
}

fn build_remote_context_search(
    args: &SearchArgs,
    spec: &str,
    remote: &RemoteOpts,
) -> Result<PipelineContext> {
    let (name, _) = remote::parse_crate_spec(spec);
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
    }
    let (workspace, _resolved_version) = remote::resolve_workspace(
        spec,
        remote.features.as_deref(),
        remote.no_default_features,
        remote.no_cache,
    )
    .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    // Pre-validate -F features against the crate's feature graph.
    if let Some(requested) = remote.features.as_deref() {
        match remote::load_remote_feature_graph(spec) {
            Ok(Some(graph)) => features::validate_requested_features(&graph, requested)?,
            Ok(None) => eprintln!(
                "warning: feature graph unavailable for '{name}'; \
                 -F values will not be validated and feature gates will not be annotated"
            ),
            Err(_) => {}
        }
    }

    let manifest_path = workspace
        .path()
        .join("Cargo.toml")
        .to_string_lossy()
        .into_owned();

    let metadata = resolve::load_cargo_metadata(Some(&manifest_path))
        .context("Failed to load cargo metadata for remote crate")?;

    let available_packages = rustdoc_json::load_lockfile_packages(Some(&manifest_path));

    Ok(PipelineContext {
        manifest_path: Some(manifest_path),
        target_dir: metadata.target_dir,
        package_name: name,
        module_path: None,      // search doesn't target modules
        observer_package: None, // remote → always external view
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: true, // remote — versions are locked
        workspace_members: HashSet::new(),
        available_packages,
        crate_header: None,
        _workspace: Some(workspace),
    })
}

fn run_shared_search_pipeline(ctx: &PipelineContext, args: &SearchArgs) -> Result<String> {
    let (model, same_crate, reachable) = generate_and_parse_model(ctx)?;
    let pattern = args.pattern();
    let methods_of = args.methods_of.as_deref();

    let search_kind = args.search_kind.as_deref();
    let members = args.members;
    let search_fn = |model: &CrateModel,
                     observer: Option<&str>,
                     same_crate: bool,
                     reachable: Option<&ReachableInfo>| {
        search::render_search_filtered(
            model,
            &pattern,
            &args.filter,
            args.limit.as_deref(),
            observer,
            same_crate,
            reachable,
            methods_of,
            search_kind,
            members,
        )
    };

    let mut output = search_fn(
        &model,
        if same_crate {
            args.at_mod.as_deref()
        } else {
            None
        },
        same_crate,
        reachable.as_ref(),
    );

    // Cross-crate search: build unified index, search with accessible paths
    if cross_crate::root_has_cross_crate_reexports(&model) {
        pre_warm_cross_crate_json(&model, ctx);
        if ctx.verbose {
            eprintln!("[cargo-brief] Building cross-crate accessible path index...");
        }
        let index = cross_crate::build_cross_crate_index(
            &model,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
            &ctx.workspace_members,
            &ctx.available_packages,
        );
        let cross_output = search::search_cross_crate_index(
            &index,
            model.crate_name(),
            &pattern,
            &args.filter,
            args.limit.as_deref(),
            search_kind,
            methods_of,
            members,
        );
        if !cross_output.is_empty() {
            output.push_str(&cross_output);
        }
    }

    Ok(output)
}

/// Run the examples pipeline and return the rendered output string.
pub fn run_examples_pipeline(args: &ExamplesArgs, remote: &RemoteOpts) -> Result<String> {
    if remote.crates {
        // Remote path — crate_name IS the spec
        let spec = &args.crate_name;
        let (name, _) = remote::parse_crate_spec(spec);
        if args.global.verbose {
            eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
        }
        let (workspace, resolved_version) = remote::resolve_workspace(
            spec,
            remote.features.as_deref(),
            remote.no_default_features,
            remote.no_cache,
        )
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

/// Run the tree-sitter query pipeline and return the rendered output string.
pub fn run_ts_pipeline(args: &TsArgs, remote: &RemoteOpts) -> Result<String> {
    if remote.crates {
        let spec = &args.crate_name;
        let (name, _) = remote::parse_crate_spec(spec);
        if args.global.verbose {
            eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
        }
        let (workspace, _resolved_version) = remote::resolve_workspace(
            spec,
            remote.features.as_deref(),
            remote.no_default_features,
            remote.no_cache,
        )
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

        if args.global.verbose {
            eprintln!("[cargo-brief] Running tree-sitter query on '{name}'...");
        }

        ts::run_query(&source_root, &args.query, args)
    } else {
        let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
            .context("Failed to load cargo metadata")?;

        let (_pkg_name, source_root) = if args.crate_name == "self" {
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
            eprintln!(
                "[cargo-brief] Running tree-sitter query on '{}'...",
                args.crate_name
            );
        }

        ts::run_query(&source_root, &args.query, args)
    }
}

/// Run the code lookup pipeline and return the rendered output string.
pub fn run_code_pipeline(args: &CodeArgs, remote: &RemoteOpts) -> Result<String> {
    let resolved = code::resolve_code_args(args)?;

    // Phase A — Resolve target into primary sources + dep-resolution info
    struct CodeTarget {
        /// Primary source roots to search (pkg_name, dir).
        primary_sources: Vec<(String, PathBuf)>,
        effective_manifest: String,
        target_dir: PathBuf,
        is_workspace_member: bool,
        /// Root package name for dep resolution.
        dep_root_pkg: String,
        _workspace: Option<remote::WorkspaceDir>,
    }

    let target = if remote.crates {
        // Remote mode — "self" is invalid
        if resolved.target == "self" {
            bail!("-C (remote) mode requires an explicit crate spec as TARGET");
        }
        let spec = &resolved.target;
        let (crate_name, _) = remote::parse_crate_spec(spec);
        if args.global.verbose {
            eprintln!("[cargo-brief] Resolving workspace for '{crate_name}'...");
        }
        let (workspace, _resolved_version) = remote::resolve_workspace(
            spec,
            remote.features.as_deref(),
            remote.no_default_features,
            remote.no_cache,
        )
        .with_context(|| format!("Failed to create workspace for '{crate_name}'"))?;

        let manifest_path = workspace
            .path()
            .join("Cargo.toml")
            .to_string_lossy()
            .into_owned();

        if args.global.verbose {
            eprintln!("[cargo-brief] Finding source root for '{crate_name}'...");
        }
        let source_root = resolve::find_dep_source_root(&manifest_path, &crate_name)
            .with_context(|| format!("Failed to find source root for '{crate_name}'"))?;

        // For remote crates, target_dir is needed only when searching deps
        let target_dir = if !args.no_deps {
            let meta = resolve::load_cargo_metadata(Some(&manifest_path))
                .context("Failed to load cargo metadata for remote crate")?;
            meta.target_dir
        } else {
            PathBuf::new()
        };

        CodeTarget {
            primary_sources: vec![(crate_name.to_string(), source_root)],
            effective_manifest: manifest_path,
            target_dir,
            is_workspace_member: false,
            dep_root_pkg: crate_name.to_string(),
            _workspace: Some(workspace),
        }
    } else {
        let metadata = resolve::load_cargo_metadata(args.manifest_path.as_deref())
            .context("Failed to load cargo metadata")?;

        if resolved.target == "self" {
            // "self" → all workspace members
            let mut primary_sources = Vec::new();
            for pkg in &metadata.workspace_packages {
                if let Some(dir) = metadata.package_manifest_dirs.get(pkg) {
                    primary_sources.push((pkg.clone(), dir.clone()));
                }
            }
            if primary_sources.is_empty() {
                bail!("No workspace packages found. Run from inside a Cargo project.");
            }

            let effective_manifest = args
                .manifest_path
                .clone()
                .unwrap_or_else(|| "Cargo.toml".to_string());

            let dep_root_pkg = metadata
                .current_package
                .clone()
                .or_else(|| metadata.workspace_packages.first().cloned())
                .unwrap_or_default();

            CodeTarget {
                primary_sources,
                effective_manifest,
                target_dir: metadata.target_dir,
                is_workspace_member: true,
                dep_root_pkg,
                _workspace: None,
            }
        } else {
            // Named target — single crate
            let normalized = resolved.target.replace('-', "_");
            let found = metadata
                .package_manifest_dirs
                .iter()
                .find(|(k, _)| k.replace('-', "_") == normalized);
            let (pkg_name, source_root) = match found {
                Some((name, dir)) => (name.clone(), dir.clone()),
                None => {
                    anyhow::bail!(
                        "Package '{}' not found in workspace. Available: {}",
                        resolved.target,
                        metadata.workspace_packages.join(", ")
                    );
                }
            };

            let effective_manifest = metadata
                .package_manifest_dirs
                .get(&pkg_name)
                .map(|d| d.join("Cargo.toml").to_string_lossy().into_owned())
                .or_else(|| args.manifest_path.clone())
                .unwrap_or_else(|| "Cargo.toml".to_string());

            CodeTarget {
                primary_sources: vec![(pkg_name.clone(), source_root)],
                effective_manifest,
                target_dir: metadata.target_dir,
                is_workspace_member: true,
                dep_root_pkg: pkg_name,
                _workspace: None,
            }
        }
    };

    if args.global.verbose {
        let names: Vec<&str> = target
            .primary_sources
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        eprintln!(
            "[cargo-brief] Searching {} for code definitions...",
            names.join(", ")
        );
    }

    // Phase B — Collect dep sources
    let mut sources = target.primary_sources.clone();

    if !args.no_deps {
        let dep_sources = if args.all_deps {
            collect_all_deps_sources(&target.effective_manifest, &target.dep_root_pkg)?
        } else {
            collect_accessible_deps_sources(
                &target.dep_root_pkg,
                &target.effective_manifest,
                &target.target_dir,
                &args.global.toolchain,
                args.global.verbose,
                !target.is_workspace_member,
            )?
        };
        // Filter out deps that are already in primary sources
        let primary_names: std::collections::HashSet<&str> = target
            .primary_sources
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        sources.extend(
            dep_sources
                .into_iter()
                .filter(|(n, _)| !primary_names.contains(n.as_str())),
        );
        if args.global.verbose {
            eprintln!("[cargo-brief] Searching {} crate(s)...", sources.len());
        }
    }

    // Phase C — Search
    let mut output = String::new();

    // Definitions (unless --refs-only)
    if !args.refs_only {
        output = code::search_code(
            &sources,
            &resolved.name,
            resolved.kind,
            args,
            args.in_type.as_deref(),
        )?;
    }

    // References (if --refs or --refs-only)
    if args.refs || args.refs_only {
        let ref_limit = if args.refs_only {
            args.limit.as_deref()
        } else {
            None
        };
        let refs = code::search_references(
            &sources,
            &resolved.name,
            args.src_only,
            args.quiet,
            ref_limit,
        );
        if !refs.is_empty() && !refs.starts_with("// no references") {
            if !output.is_empty() {
                output.push_str("\n// --- References ---\n\n");
            }
            output.push_str(&refs);
        } else if args.refs_only {
            output = refs;
        }
    }

    Ok(output)
}

/// Collect source dirs for all direct dependencies via cargo metadata.
fn collect_all_deps_sources(
    manifest_path: &str,
    root_package: &str,
) -> Result<Vec<(String, PathBuf)>> {
    let (all_dirs, direct_deps) =
        resolve::load_dep_package_dirs(Some(manifest_path), root_package)?;

    let mut result = Vec::new();
    for dep_name in &direct_deps {
        if let Some(dir) = all_dirs.get(dep_name) {
            result.push((dep_name.clone(), dir.clone()));
        }
    }
    Ok(result)
}

/// Collect source dirs for accessible dependencies via rustdoc JSON BFS.
fn collect_accessible_deps_sources(
    pkg_name: &str,
    manifest_path: &str,
    target_dir: &Path,
    toolchain: &str,
    verbose: bool,
    use_cache: bool,
) -> Result<Vec<(String, PathBuf)>> {
    // Generate rustdoc JSON for the target crate
    let json_path = rustdoc_json::generate_rustdoc_json(
        pkg_name,
        toolchain,
        Some(manifest_path),
        true, // document_private_items
        target_dir,
        verbose,
        use_cache,
    )
    .with_context(|| format!("Failed to generate rustdoc JSON for '{pkg_name}'"))?;

    let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path)
        .with_context(|| format!("Failed to parse rustdoc JSON for '{pkg_name}'"))?;

    let model = CrateModel::from_crate(krate);

    // BFS discover accessible deps
    let accessible =
        discover_accessible_deps(&model, toolchain, Some(manifest_path), target_dir, verbose);

    if accessible.is_empty() {
        return Ok(Vec::new());
    }

    // Map accessible dep names to source directories
    let (all_dirs, _) = resolve::load_dep_package_dirs(Some(manifest_path), pkg_name)?;

    let mut result = Vec::new();
    for dep_name in &accessible {
        let base = dep_name.split('@').next().unwrap_or(dep_name);
        if let Some(dir) = all_dirs.get(base) {
            result.push((base.to_string(), dir.clone()));
        } else {
            // Try hyphen/underscore normalization
            let alt = base.replace('_', "-");
            if let Some(dir) = all_dirs.get(&alt) {
                result.push((alt, dir.clone()));
            }
        }
    }
    Ok(result)
}

/// Run the summary pipeline and return the rendered output string.
pub fn run_summary_pipeline(args: &SummaryArgs, remote: &RemoteOpts) -> Result<String> {
    let ctx = if remote.crates {
        let spec = &args.target.crate_name;
        build_remote_context_summary(args, spec, remote)?
    } else {
        build_local_context_summary(args)?
    };
    run_shared_summary_pipeline(&ctx)
}

fn build_local_context_summary(args: &SummaryArgs) -> Result<PipelineContext> {
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

    let available_packages =
        rustdoc_json::load_lockfile_packages(args.target.manifest_path.as_deref());
    let is_workspace_member = metadata.workspace_packages.contains(&resolved.package_name);

    Ok(PipelineContext {
        manifest_path: args.target.manifest_path.clone(),
        target_dir: metadata.target_dir,
        package_name: resolved.package_name,
        module_path: resolved.module_path,
        observer_package,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: !is_workspace_member,
        workspace_members: metadata.workspace_packages.into_iter().collect(),
        available_packages,
        crate_header: None,
        _workspace: None,
    })
}

fn build_remote_context_summary(
    args: &SummaryArgs,
    spec: &str,
    remote: &RemoteOpts,
) -> Result<PipelineContext> {
    // With -C, crate_name IS the spec. If it contains "::", split into spec + module.
    let (actual_spec, module_path) = if let Some(idx) = spec.find("::") {
        let rest = &spec[idx + 2..];
        let module = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
        (&spec[..idx], module)
    } else {
        (spec, args.target.module_path.clone())
    };

    let (name, _) = remote::parse_crate_spec(actual_spec);
    if args.global.verbose {
        eprintln!("[cargo-brief] Resolving workspace for '{name}'...");
    }
    let (workspace, resolved_version) = remote::resolve_workspace(
        actual_spec,
        remote.features.as_deref(),
        remote.no_default_features,
        remote.no_cache,
    )
    .with_context(|| format!("Failed to create workspace for '{name}'"))?;

    // Pre-validate -F features against the crate's feature graph.
    if let Some(requested) = remote.features.as_deref() {
        match remote::load_remote_feature_graph(actual_spec) {
            Ok(Some(graph)) => features::validate_requested_features(&graph, requested)?,
            Ok(None) => eprintln!(
                "warning: feature graph unavailable for '{name}'; \
                 -F values will not be validated and feature gates will not be annotated"
            ),
            Err(_) => {}
        }
    }

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
        remote.features.as_deref(),
    );

    let available_packages = rustdoc_json::load_lockfile_packages(Some(&manifest_path));

    Ok(PipelineContext {
        manifest_path: Some(manifest_path),
        target_dir: metadata.target_dir,
        package_name: name,
        module_path,
        observer_package: None,
        toolchain: args.global.toolchain.clone(),
        verbose: args.global.verbose,
        use_cache: true,
        workspace_members: HashSet::new(),
        available_packages,
        crate_header,
        _workspace: Some(workspace),
    })
}

fn run_shared_summary_pipeline(ctx: &PipelineContext) -> Result<String> {
    let (model, same_crate, reachable) = generate_and_parse_model(ctx)?;

    let mut output = summary::render_summary(
        &model,
        ctx.module_path.as_deref(),
        same_crate,
        reachable.as_ref(),
    );

    // Cross-crate: if facade and no module scoping, build accessible-path index
    if ctx.module_path.is_none() && cross_crate::root_has_cross_crate_reexports(&model) {
        pre_warm_cross_crate_json(&model, ctx);
        if ctx.verbose {
            eprintln!("[cargo-brief] Building cross-crate accessible path index...");
        }
        let index = cross_crate::build_cross_crate_index(
            &model,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
            &ctx.workspace_members,
            &ctx.available_packages,
        );
        let cross_summary = summary::summarize_cross_crate_index(&index);
        if !cross_summary.is_empty() {
            output.push_str(&cross_summary);
        }
    }

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

/// Pre-warm rustdoc JSON cache for cross-crate dependencies via batch generation.
///
/// Recursive BFS: each iteration discovers new crate names from the previous batch,
/// generates them, and repeats until no new crates are found or MAX_DEPTH is reached.
fn pre_warm_cross_crate_json(model: &CrateModel, ctx: &PipelineContext) {
    let mut seen = HashSet::new();

    // Seed: collect external crate names from the primary model.
    // Names from rustdoc use underscores; normalize to Cargo.lock form (may be hyphenated).
    let mut batch: Vec<String> = cross_crate::collect_external_crate_names(model)
        .into_iter()
        .filter_map(|n| normalize_to_lockfile_name(&n, &ctx.available_packages))
        .collect();
    batch.sort();
    batch.dedup();
    seen.extend(batch.iter().cloned());

    const MAX_DEPTH: usize = 8;
    for _ in 0..MAX_DEPTH {
        if batch.is_empty() {
            break;
        }

        let refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
        rustdoc_json::batch_generate_rustdoc_json(
            &refs,
            &ctx.toolchain,
            ctx.manifest_path.as_deref(),
            &ctx.target_dir,
            ctx.verbose,
        );

        // Parse this batch's crates to discover the next level
        let mut next_batch = Vec::new();
        for name in &batch {
            let base = name.split('@').next().unwrap_or(name);
            let doc_dir = ctx.target_dir.join("doc");
            let Some(json_path) =
                rustdoc_json::find_lib_json_path(base, ctx.manifest_path.as_deref(), &doc_dir)
            else {
                continue;
            };
            let Ok(krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
                continue;
            };
            let sub_model = CrateModel::from_crate(krate);
            for sub_name in cross_crate::collect_external_crate_names(&sub_model) {
                if let Some(pkg_name) =
                    normalize_to_lockfile_name(&sub_name, &ctx.available_packages)
                    && !seen.contains(&pkg_name)
                {
                    seen.insert(pkg_name.clone());
                    next_batch.push(pkg_name);
                }
            }
        }
        next_batch.sort();
        next_batch.dedup();
        batch = next_batch;
    }
}

/// BFS-discover accessible dependency crate names via rustdoc JSON.
/// Standalone version — takes explicit params instead of PipelineContext.
/// Returns cargo-compatible package names (possibly with @version suffix).
fn discover_accessible_deps(
    model: &CrateModel,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
) -> HashSet<String> {
    let packages = rustdoc_json::load_lockfile_packages(manifest_path);

    // Seed: collect external crate names from the primary model.
    let mut batch: Vec<String> = cross_crate::collect_external_crate_names(model)
        .into_iter()
        .filter_map(|n| normalize_to_lockfile_name(&n, &packages))
        .collect();
    batch.sort();
    batch.dedup();
    let mut seen: HashSet<String> = batch.iter().cloned().collect();

    const MAX_DEPTH: usize = 8;
    for _ in 0..MAX_DEPTH {
        if batch.is_empty() {
            break;
        }

        let refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
        rustdoc_json::batch_generate_rustdoc_json(
            &refs,
            toolchain,
            manifest_path,
            target_dir,
            verbose,
        );

        let mut next_batch = Vec::new();
        for name in &batch {
            let base = name.split('@').next().unwrap_or(name);
            let doc_dir = target_dir.join("doc");
            let Some(json_path) =
                rustdoc_json::find_lib_json_path(base, manifest_path, &doc_dir)
            else {
                continue;
            };
            let Ok(krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
                continue;
            };
            let sub_model = CrateModel::from_crate(krate);
            for sub_name in cross_crate::collect_external_crate_names(&sub_model) {
                if let Some(pkg_name) = normalize_to_lockfile_name(&sub_name, &packages)
                    && !seen.contains(&pkg_name)
                {
                    seen.insert(pkg_name.clone());
                    next_batch.push(pkg_name);
                }
            }
        }
        next_batch.sort();
        next_batch.dedup();
        batch = next_batch;
    }

    seen
}

/// Normalize a rustdoc crate name (underscores) to the Cargo.lock package spec.
///
/// Rustdoc `use_item.source` gives Rust identifiers (e.g. `bevy_ecs`), but
/// `cargo doc -p` expects Cargo package names (e.g. `bevy-ecs`). Returns the
/// spec that can be passed to cargo, with `@version` suffix when multiple
/// versions exist. Returns None if not found.
fn normalize_to_lockfile_name(name: &str, packages: &LockfilePackages) -> Option<String> {
    packages.resolve_spec(name)
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
        // Phase 2: inline full definitions from source crates (only glob sources)
        let mut seen_names = HashSet::new();
        for source in result.item_names.keys() {
            if let Some(models) = result.source_models.get(source) {
                let mut rendered = String::new();
                for model in models {
                    rendered.push_str(&render::render_inlined_items(
                        model,
                        filter,
                        &mut seen_names,
                    ));
                }
                let pattern = format!("pub use {source}::*;");
                replace_glob_lines(output, &pattern, &rendered);
            }
        }

        // Named cross-crate re-exports (same expand_glob gate as Phase 2)
        for (source, items) in &result.named_reexports {
            if let Some(models) = result.source_models.get(source) {
                for (item_name, full_source_path) in items {
                    if let Some(rendered) = render::render_single_inlined_item(
                        models,
                        item_name,
                        filter,
                        &mut seen_names,
                    ) {
                        let pattern = format!("pub use {full_source_path};");
                        replace_glob_lines(output, &pattern, &rendered);
                    }
                }
            }
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

/// Try generating rustdoc JSON for a crate, falling back to hyphenated name.
///
/// Rustdoc `use_item.source` gives Rust identifiers (underscores), but
/// `cargo rustdoc -p` expects package names (hyphens). Try the original name
/// first, then try with `_` → `-` if it fails.
fn try_generate_rustdoc_json(
    source: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
    use_cache: bool,
) -> Option<PathBuf> {
    // Try original name first (works for crates without hyphens)
    if let Ok(path) = rustdoc_json::generate_rustdoc_json(
        source,
        toolchain,
        manifest_path,
        false,
        target_dir,
        verbose,
        use_cache,
    ) {
        return Some(path);
    }
    // Fallback: try hyphenated name (glob_source → glob-source)
    let hyphenated = source.replace('_', "-");
    if hyphenated != source
        && let Ok(path) = rustdoc_json::generate_rustdoc_json(
            &hyphenated,
            toolchain,
            manifest_path,
            false,
            target_dir,
            verbose,
            use_cache,
        )
    {
        return Some(path);
    }
    None
}

/// Detect glob re-exports in the target module and expand each by generating
/// rustdoc JSON for the source crate and enumerating its public items.
///
/// Returns both item names (for Phase 1 `pub use` lines) and source models
/// (for Phase 2 full definition inlining). Recursively follows cross-crate
/// glob chains (max depth 8).
fn expand_glob_reexports(
    model: &CrateModel,
    target_module_path: Option<&str>,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
    workspace_members: &HashSet<String>,
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
            named_reexports: HashMap::new(),
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

        // Cache non-workspace deps (immutable once resolved via Cargo.lock)
        let dep_use_cache = !workspace_members.contains(source.as_str())
            && !workspace_members.contains(&source.replace('_', "-"));

        // Generate JSON for the source crate (pub items only, no private items)
        let Some(json_path) = try_generate_rustdoc_json(
            source,
            toolchain,
            manifest_path,
            target_dir,
            verbose,
            dep_use_cache,
        ) else {
            continue;
        };
        let Ok(source_krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
            continue;
        };

        let source_model = CrateModel::from_crate(source_krate);
        let mut all_items = Vec::new();
        let mut all_models = Vec::new();
        let mut visited = HashSet::new();
        visited.insert(source.clone());

        collect_glob_items_recursive(
            &source_model,
            toolchain,
            manifest_path,
            target_dir,
            verbose,
            workspace_members,
            &mut visited,
            &mut all_items,
            &mut all_models,
            0,
        );

        all_items.sort();
        all_items.dedup();

        // Direct source model goes first (index 0)
        let mut models = vec![source_model];
        models.extend(all_models);

        item_names.insert(source.clone(), all_items);
        source_models.insert(source.clone(), models);
    }

    // Second pass: named cross-crate re-exports
    let mut named_reexports: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for (_id, child) in model.module_children(target_item) {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if use_item.is_glob {
            continue;
        }

        // Cross-crate: target id exists but is not in the local model's index
        let is_cross_crate = match &use_item.id {
            Some(id) => !model.krate.index.contains_key(id),
            None => continue, // unresolvable (primitive re-export) — skip
        };
        if !is_cross_crate {
            continue;
        }

        // Extract source crate name (first :: segment) and item name (last :: segment)
        let source_path = &use_item.source;
        let Some((source_prefix, item_name)) = source_path.rsplit_once("::") else {
            continue;
        };
        let crate_name = source_prefix.split("::").next().unwrap();

        // Module re-exports (e.g., `pub use serde_core::de;`) are not filtered here —
        // render_single_inlined_item returns None for modules, leaving pub use line intact.

        // Generate source model if not already present from glob processing
        if !source_models.contains_key(crate_name) {
            let dep_use_cache = !workspace_members.contains(crate_name)
                && !workspace_members.contains(&crate_name.replace('_', "-"));
            let Some(json_path) = try_generate_rustdoc_json(
                crate_name,
                toolchain,
                manifest_path,
                target_dir,
                verbose,
                dep_use_cache,
            ) else {
                continue;
            };
            let Ok(source_krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
                continue;
            };
            source_models.insert(
                crate_name.to_string(),
                vec![CrateModel::from_crate(source_krate)],
            );
        }

        named_reexports
            .entry(crate_name.to_string())
            .or_default()
            .push((item_name.to_string(), source_path.clone()));
    }

    GlobExpansionResult {
        item_names,
        source_models,
        named_reexports,
    }
}

/// Recursively collect public item names and models from a source crate.
///
/// For each child of the source model's root:
/// - `is_glob=true` Use: attempt to generate rustdoc JSON for the source and recurse
/// - Non-glob Use: collect the re-exported name
/// - Direct item (non-module): collect the item name
/// - Module: skip (same as top-level expansion)
#[allow(clippy::too_many_arguments)]
fn collect_glob_items_recursive(
    source_model: &CrateModel,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
    workspace_members: &HashSet<String>,
    visited: &mut HashSet<String>,
    all_items: &mut Vec<String>,
    all_models: &mut Vec<CrateModel>,
    depth: usize,
) {
    const MAX_DEPTH: usize = 8;

    let Some(root) = source_model.root_module() else {
        return;
    };

    for (_, child) in source_model.module_children(root) {
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }
        if matches!(child.inner, ItemEnum::Module(_)) {
            continue;
        }

        if let ItemEnum::Use(use_item) = &child.inner {
            if use_item.is_glob {
                // Cross-crate glob — recurse if within depth limit
                if depth >= MAX_DEPTH {
                    continue;
                }
                let nested_source = &use_item.source;
                if !visited.insert(nested_source.clone()) {
                    continue; // cycle prevention
                }
                if verbose {
                    eprintln!(
                        "[cargo-brief] Following nested glob re-export: {nested_source} (depth {})",
                        depth + 1
                    );
                }
                let nested_use_cache = !workspace_members.contains(nested_source.as_str())
                    && !workspace_members.contains(&nested_source.replace('_', "-"));
                let Some(json_path) = try_generate_rustdoc_json(
                    nested_source,
                    toolchain,
                    manifest_path,
                    target_dir,
                    verbose,
                    nested_use_cache,
                ) else {
                    continue; // intra-crate path or missing dep — skip
                };
                let Ok(nested_krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
                    continue;
                };
                let nested_model = CrateModel::from_crate(nested_krate);
                collect_glob_items_recursive(
                    &nested_model,
                    toolchain,
                    manifest_path,
                    target_dir,
                    verbose,
                    workspace_members,
                    visited,
                    all_items,
                    all_models,
                    depth + 1,
                );
                all_models.push(nested_model);
            } else {
                // Non-glob Use: collect the re-exported name
                if let Some(name) = child.name.as_ref().or(Some(&use_item.name)) {
                    all_items.push(name.clone());
                }
            }
        } else {
            // Direct item (struct, trait, fn, etc.)
            if let Some(name) = &child.name {
                all_items.push(name.clone());
            }
        }
    }
}
