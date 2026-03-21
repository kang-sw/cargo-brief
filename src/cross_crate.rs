//! Cross-crate module following for facade crates.
//!
//! Facade crates like `bevy` re-export modules from sub-crates:
//! `bevy → pub use bevy_internal::* → pub use bevy_ecs as ecs`.
//! This module resolves such chains by generating rustdoc JSON for
//! intermediate crates and following Use items.
//!
//! ## CrossCrateIndex
//!
//! `build_cross_crate_index()` builds a unified accessible-path index for
//! facade crates. Each item gets an `accessible_path` reflecting how users
//! actually `use` it (e.g. `render::render_resource::AsBindGroup` instead of
//! `bevy_render::render_resource::bind_group::AsBindGroup`).

// Vec<Box<...>> is intentional for pointer stability across Vec reallocation.
#![allow(clippy::vec_box)]

use std::collections::HashSet;
use std::path::Path;

use rustdoc_types::{Id, Item, ItemEnum, Visibility};

use crate::model::{CrateModel, ReachableInfo, compute_reachable_set};
use crate::rustdoc_json::{self, LockfilePackages};

/// An item accessible from the facade crate's public API, with its
/// resolved accessible path and a pointer to its definition.
pub struct AccessibleEntry {
    /// Path from the facade root, e.g. "render::render_resource::AsBindGroup"
    pub accessible_path: String,
    /// Index into `CrossCrateIndex.source_models`
    pub crate_idx: usize,
    /// Item ID within `source_models[crate_idx].krate.index`
    pub item_id: Id,
    /// The kind of item (for search filtering / rendering dispatch)
    pub item_kind: AccessibleItemKind,
}

/// Coarse item kind — enough for search/summary, not full rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibleItemKind {
    Module,
    Function,
    Struct,
    Enum,
    Trait,
    Union,
    TypeAlias,
    Constant,
    Static,
    Macro,
}

impl AccessibleItemKind {
    fn from_item(item: &Item) -> Option<Self> {
        match &item.inner {
            ItemEnum::Module(_) => Some(Self::Module),
            ItemEnum::Function(_) => Some(Self::Function),
            ItemEnum::Struct(_) => Some(Self::Struct),
            ItemEnum::Enum(_) => Some(Self::Enum),
            ItemEnum::Trait(_) => Some(Self::Trait),
            ItemEnum::Union(_) => Some(Self::Union),
            ItemEnum::TypeAlias(_) => Some(Self::TypeAlias),
            ItemEnum::Constant { .. } => Some(Self::Constant),
            ItemEnum::Static(_) => Some(Self::Static),
            ItemEnum::Macro(_) => Some(Self::Macro),
            _ => None,
        }
    }
}

/// Unified cross-crate accessible-path index.
pub struct CrossCrateIndex {
    /// All loaded source crate models, each paired with its ReachableInfo.
    /// Box provides stable heap addresses — raw pointers into source_models
    /// remain valid even when the Vec reallocates on push. This is intentional,
    /// not redundant boxing.
    #[allow(clippy::vec_box)]
    pub source_models: Vec<Box<(CrateModel, ReachableInfo)>>,
    /// All accessible items, sorted by `accessible_path`.
    pub items: Vec<AccessibleEntry>,
}

/// Result of resolving a cross-crate module path.
pub struct CrossCrateResolution {
    /// The CrateModel of the resolved sub-crate.
    pub model: CrateModel,
    /// Remaining module path within the sub-crate (e.g., "system" from "ecs::system").
    pub inner_module_path: Option<String>,
}

/// Try to resolve a module path by following cross-crate re-exports.
///
/// Given `module_path = "ecs::system"`, scans the primary model's root for
/// a Use item named "ecs", follows the re-export chain to the leaf crate,
/// and returns its model plus the remaining inner path ("system").
pub fn resolve_cross_crate_module(
    primary_model: &CrateModel,
    module_path: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
) -> Option<CrossCrateResolution> {
    let (first_segment, rest) = match module_path.split_once("::") {
        Some((first, rest)) => (first, Some(rest.to_string())),
        None => (module_path, None),
    };

    let crate_name = primary_model.crate_name();
    let root = primary_model.root_module()?;
    let children = primary_model.module_children(root);

    // Strategy 1: Look for a named Use item matching first_segment
    for (_id, child) in &children {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if use_item.is_glob {
            continue;
        }
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }
        if is_intra_crate_source(&use_item.source, crate_name) {
            continue;
        }

        let name = child.name.as_deref().unwrap_or(&use_item.name);
        if name != first_segment {
            continue;
        }

        // Found a match — follow the source to the actual crate
        if let Some(resolution) = follow_use_chain(
            &use_item.source,
            first_segment,
            rest.clone(),
            toolchain,
            manifest_path,
            target_dir,
            verbose,
        ) {
            return Some(resolution);
        }
    }

    // Strategy 2: For glob re-exports, generate the source crate's JSON
    // and look for first_segment in its root
    for (_id, child) in &children {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if !use_item.is_glob {
            continue;
        }
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }
        if is_intra_crate_source(&use_item.source, crate_name) {
            continue;
        }

        let source_crate = extract_crate_name(&use_item.source);
        let Ok(json_path) = rustdoc_json::generate_rustdoc_json(
            &source_crate,
            toolchain,
            manifest_path,
            true,
            target_dir,
            verbose,
            true, // cross-crate — use cache
        ) else {
            continue;
        };
        let Ok(krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
            continue;
        };
        let source_model = CrateModel::from_crate(krate);

        // Check if first_segment is a module in the source
        if source_model.find_module(first_segment).is_some() {
            return Some(CrossCrateResolution {
                model: source_model,
                inner_module_path: if let Some(r) = &rest {
                    Some(format!("{first_segment}::{r}"))
                } else {
                    Some(first_segment.to_string())
                },
            });
        }

        // Check if first_segment is a named re-export in the source
        if let Some(root) = source_model.root_module() {
            for (_sid, schild) in source_model.module_children(root) {
                let ItemEnum::Use(su) = &schild.inner else {
                    continue;
                };
                if su.is_glob {
                    continue;
                }
                let sname = schild.name.as_deref().unwrap_or(&su.name);
                if sname != first_segment {
                    continue;
                }
                if let Some(resolution) = follow_use_chain(
                    &su.source,
                    first_segment,
                    rest.clone(),
                    toolchain,
                    manifest_path,
                    target_dir,
                    verbose,
                ) {
                    return Some(resolution);
                }
            }
        }
    }

    None
}

/// Check if the root module has cross-crate re-exports (Use items pointing
/// to external crates with no corresponding Module in the index).
pub fn root_has_cross_crate_reexports(model: &CrateModel) -> bool {
    let crate_name = model.crate_name();
    let Some(root) = model.root_module() else {
        return false;
    };

    let children = model.module_children(root);

    for (_id, child) in &children {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }

        // Skip intra-crate re-exports (self::, or same crate name)
        if is_intra_crate_source(&use_item.source, crate_name) {
            continue;
        }

        if use_item.is_glob {
            return true;
        }

        // Named use targeting an external crate (no local module with this name)
        let name = child.name.as_deref().unwrap_or(&use_item.name);
        if use_item.id.is_none() || model.find_module(name).is_none() {
            return true;
        }
    }

    false
}

/// Collect external crate names from root-level re-exports.
///
/// Walks the root module's children and collects crate names from public Use items
/// that point to external crates (not intra-crate, not local modules).
/// Includes both named and glob re-exports.
pub fn collect_external_crate_names(model: &CrateModel) -> Vec<String> {
    let crate_name = model.crate_name();
    let Some(root) = model.root_module() else {
        return Vec::new();
    };

    let children = model.module_children(root);
    let mut seen = HashSet::new();

    for (_id, child) in &children {
        let ItemEnum::Use(use_item) = &child.inner else {
            continue;
        };
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }
        if is_intra_crate_source(&use_item.source, crate_name) {
            continue;
        }

        let source_crate = extract_crate_name(&use_item.source);

        // Skip if it's a local module (not an external crate)
        if model.find_module(&source_crate).is_some() {
            continue;
        }

        seen.insert(source_crate);
    }

    seen.into_iter().collect()
}

/// Build a unified cross-crate accessible-path index for a facade crate.
///
/// Walks the primary model's public API top-down, following glob/named
/// re-exports into sub-crates. Each discovered item gets an `accessible_path`
/// reflecting how users actually `use` it through the facade.
#[allow(clippy::too_many_arguments)]
pub fn build_cross_crate_index(
    primary_model: &CrateModel,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
    workspace_members: &HashSet<String>,
    available_packages: &LockfilePackages,
) -> CrossCrateIndex {
    let mut source_models: Vec<Box<(CrateModel, ReachableInfo)>> = Vec::new();
    let mut items: Vec<AccessibleEntry> = Vec::new();
    let mut visited_crates: HashSet<String> = HashSet::new();
    visited_crates.insert(primary_model.crate_name().to_string());

    let ctx = WalkParams {
        toolchain,
        manifest_path,
        target_dir,
        verbose,
        workspace_members,
        available_packages,
    };

    let Some(root) = primary_model.root_module() else {
        return CrossCrateIndex {
            source_models,
            items,
        };
    };

    let primary_reachable = compute_reachable_set(primary_model);
    walk_accessible(
        primary_model,
        root,
        "",
        None, // no crate_idx — primary model items are rendered by the main pipeline
        &primary_reachable,
        0,
        &mut visited_crates,
        &mut source_models,
        &mut items,
        &ctx,
    );

    // Dedup: group by (crate_idx, item_id), keep shortest non-prelude path
    dedup_accessible_entries(&mut items);

    // Sort by accessible_path
    items.sort_by(|a, b| a.accessible_path.cmp(&b.accessible_path));

    CrossCrateIndex {
        source_models,
        items,
    }
}

/// Immutable walk parameters threaded through the recursion.
struct WalkParams<'a> {
    toolchain: &'a str,
    manifest_path: Option<&'a str>,
    target_dir: &'a Path,
    verbose: bool,
    workspace_members: &'a HashSet<String>,
    /// Packages known from Cargo.lock — used to skip names that aren't real packages.
    available_packages: &'a LockfilePackages,
}

/// Recursive walk collecting accessible items with prefix tracking.
///
/// `source_models` and `items` are split to allow loading new models
/// while walking into already-loaded ones.
#[allow(clippy::too_many_arguments)]
fn walk_accessible(
    model: &CrateModel,
    module_item: &Item,
    prefix: &str,
    crate_idx: Option<usize>, // None = primary model (skip registering)
    reachable: &ReachableInfo,
    depth: usize,
    visited_crates: &mut HashSet<String>,
    source_models: &mut Vec<Box<(CrateModel, ReachableInfo)>>,
    items: &mut Vec<AccessibleEntry>,
    ctx: &WalkParams,
) {
    if depth > 8 {
        return;
    }

    let crate_name = model.crate_name().to_string();
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Visibility: use reachable set for external view
        if !reachable.reachable.contains(child_id) {
            continue;
        }

        match &child.inner {
            ItemEnum::Module(_) => {
                let name = match &child.name {
                    Some(n) => n.as_str(),
                    None => continue,
                };

                // Skip glob-private modules (flatten their items at current level)
                if reachable.glob_private_modules.contains(child_id) {
                    walk_accessible(
                        model,
                        child,
                        prefix,
                        crate_idx,
                        reachable,
                        depth + 1,
                        visited_crates,
                        source_models,
                        items,
                        ctx,
                    );
                    continue;
                }

                // Public module — only register if it's actually visible
                if !matches!(child.visibility, Visibility::Public) {
                    continue;
                }

                let child_prefix = join_path(prefix, name);

                if let Some(ci) = crate_idx {
                    items.push(AccessibleEntry {
                        accessible_path: child_prefix.clone(),
                        crate_idx: ci,
                        item_id: Id(child_id.0),
                        item_kind: AccessibleItemKind::Module,
                    });
                }

                walk_accessible(
                    model,
                    child,
                    &child_prefix,
                    crate_idx,
                    reachable,
                    depth + 1,
                    visited_crates,
                    source_models,
                    items,
                    ctx,
                );
            }
            ItemEnum::Use(use_item) => {
                if !matches!(child.visibility, Visibility::Public) {
                    continue;
                }

                if use_item.is_glob {
                    handle_glob_reexport(
                        model,
                        use_item,
                        &crate_name,
                        prefix,
                        crate_idx,
                        reachable,
                        depth,
                        visited_crates,
                        source_models,
                        items,
                        ctx,
                    );
                } else {
                    handle_named_reexport(
                        model,
                        child,
                        use_item,
                        &crate_name,
                        prefix,
                        crate_idx,
                        reachable,
                        depth,
                        visited_crates,
                        source_models,
                        items,
                        ctx,
                    );
                }
            }
            // Leaf items (fn, struct, enum, trait, etc.)
            _ => {
                if let Some(ci) = crate_idx {
                    let name = match &child.name {
                        Some(n) => n.as_str(),
                        None => continue,
                    };
                    if let Some(kind) = AccessibleItemKind::from_item(child) {
                        items.push(AccessibleEntry {
                            accessible_path: join_path(prefix, name),
                            crate_idx: ci,
                            item_id: Id(child_id.0),
                            item_kind: kind,
                        });
                    }
                }
            }
        }
    }
}

/// Handle a `pub use source::*` glob re-export during the walk.
#[allow(clippy::too_many_arguments)]
fn handle_glob_reexport(
    model: &CrateModel,
    use_item: &rustdoc_types::Use,
    crate_name: &str,
    prefix: &str,
    crate_idx: Option<usize>,
    reachable: &ReachableInfo,
    depth: usize,
    visited_crates: &mut HashSet<String>,
    source_models: &mut Vec<Box<(CrateModel, ReachableInfo)>>,
    items: &mut Vec<AccessibleEntry>,
    ctx: &WalkParams,
) {
    // Try to resolve the source as an intra-crate module first.
    // Rustdoc may emit relative paths (e.g. "graph_map" instead of "bevy_ecs::graph_map").
    let source_path = use_item
        .source
        .strip_prefix("self::")
        .or_else(|| use_item.source.strip_prefix(&format!("{crate_name}::")))
        .unwrap_or(&use_item.source);

    if let Some(source_mod) = model.find_module(source_path) {
        let is_private = !matches!(source_mod.visibility, Visibility::Public);
        let walk_prefix = if is_private {
            prefix.to_string()
        } else {
            join_path(
                prefix,
                source_path.rsplit("::").next().unwrap_or(source_path),
            )
        };
        walk_accessible(
            model,
            source_mod,
            &walk_prefix,
            crate_idx,
            reachable,
            depth + 1,
            visited_crates,
            source_models,
            items,
            ctx,
        );
    } else if is_intra_crate_source(&use_item.source, crate_name) {
        // Recognized as intra-crate but module not found — skip silently
    } else {
        // Cross-crate glob: load the source crate
        let source_crate = extract_crate_name(&use_item.source);
        let sub_path = use_item.source.split_once("::").map(|(_, p)| p.to_string());

        if let Some(ci) =
            load_or_find_source_crate(&source_crate, source_models, visited_crates, ctx)
        {
            // Get the start module Id, then drop the borrow before recursing.
            // We need to find the right module item to walk into.
            let start_id = if let Some(ref sp) = sub_path {
                source_models[ci].0.find_module(sp).and_then(|_| {
                    // Find the Id for this module
                    let full = format!("{}::{sp}", source_models[ci].0.crate_name());
                    source_models[ci].0.module_index.get(&full).cloned()
                })
            } else {
                Some(source_models[ci].0.krate.root)
            };

            if let Some(start_id) = start_id {
                // SAFETY: source_models uses Box, so the heap-allocated data
                // at index `ci` has a stable address even when the Vec
                // reallocates on push. The raw pointer bypasses Rust's borrow
                // checker — walk_accessible needs &mut Vec but we hold &model.
                let box_ptr: *const (CrateModel, ReachableInfo) = &*source_models[ci];
                let (sub_model, sub_reachable) = unsafe { &*box_ptr };
                if let Some(start_mod) = sub_model.krate.index.get(&start_id) {
                    walk_accessible(
                        sub_model,
                        start_mod,
                        prefix,
                        Some(ci),
                        sub_reachable,
                        depth + 1,
                        visited_crates,
                        source_models,
                        items,
                        ctx,
                    );
                }
            }
        }
    }
}

/// Handle a `pub use source as alias` named re-export during the walk.
#[allow(clippy::too_many_arguments)]
fn handle_named_reexport(
    model: &CrateModel,
    child: &Item,
    use_item: &rustdoc_types::Use,
    crate_name: &str,
    prefix: &str,
    crate_idx: Option<usize>,
    reachable: &ReachableInfo,
    depth: usize,
    visited_crates: &mut HashSet<String>,
    source_models: &mut Vec<Box<(CrateModel, ReachableInfo)>>,
    items: &mut Vec<AccessibleEntry>,
    ctx: &WalkParams,
) {
    let alias = child.name.as_deref().unwrap_or(&use_item.name);

    // Intra-crate: use_item.id is Some and target exists in current model's index.
    // (use_item.id is None for cross-crate targets — rustdoc can't resolve them.)
    if let Some(target_id) = &use_item.id
        && let Some(target) = model.krate.index.get(target_id)
    {
        let target_path = join_path(prefix, alias);
        if matches!(target.inner, ItemEnum::Module(_)) {
            if let Some(ci) = crate_idx {
                items.push(AccessibleEntry {
                    accessible_path: target_path.clone(),
                    crate_idx: ci,
                    item_id: *target_id,
                    item_kind: AccessibleItemKind::Module,
                });
            }
            walk_accessible(
                model,
                target,
                &target_path,
                crate_idx,
                reachable,
                depth + 1,
                visited_crates,
                source_models,
                items,
                ctx,
            );
        } else if let Some(ci) = crate_idx
            && let Some(kind) = AccessibleItemKind::from_item(target)
        {
            items.push(AccessibleEntry {
                accessible_path: target_path,
                crate_idx: ci,
                item_id: *target_id,
                item_kind: kind,
            });
        }
    } else if is_intra_crate_source(&use_item.source, crate_name) {
        // Recognized as intra-crate but target not resolved — skip
    } else {
        // Cross-crate named re-export (use_item.id is None)
        let source_crate = extract_crate_name(&use_item.source);
        let sub_path = use_item.source.split_once("::").map(|(_, p)| p.to_string());

        if let Some(ci) =
            load_or_find_source_crate(&source_crate, source_models, visited_crates, ctx)
        {
            let alias_path = join_path(prefix, alias);

            if let Some(ref sp) = sub_path {
                // Source points to something inside the crate
                let has_module = source_models[ci].0.find_module(sp).is_some();

                if has_module {
                    let mod_id = {
                        let full = format!("{}::{sp}", source_models[ci].0.crate_name());
                        source_models[ci].0.module_index.get(&full).cloned()
                    };
                    items.push(AccessibleEntry {
                        accessible_path: alias_path.clone(),
                        crate_idx: ci,
                        item_id: source_models[ci].0.krate.root,
                        item_kind: AccessibleItemKind::Module,
                    });
                    if let Some(mod_id) = mod_id {
                        // SAFETY: Box provides stable heap address (see handle_glob_reexport)
                        let box_ptr: *const (CrateModel, ReachableInfo) = &*source_models[ci];
                        let (sub_model, sub_reachable) = unsafe { &*box_ptr };
                        if let Some(mod_item) = sub_model.krate.index.get(&mod_id) {
                            walk_accessible(
                                sub_model,
                                mod_item,
                                &alias_path,
                                Some(ci),
                                sub_reachable,
                                depth + 1,
                                visited_crates,
                                source_models,
                                items,
                                ctx,
                            );
                        }
                    }
                } else {
                    let item_name = sp.rsplit("::").next().unwrap_or(sp);
                    find_and_register_item(&source_models[ci].0, item_name, &alias_path, ci, items);
                }
            } else {
                // Source is just a crate name → recurse from root with alias
                let root_id = source_models[ci].0.krate.root;
                items.push(AccessibleEntry {
                    accessible_path: alias_path.clone(),
                    crate_idx: ci,
                    item_id: root_id,
                    item_kind: AccessibleItemKind::Module,
                });
                // SAFETY: Box provides stable heap address (see handle_glob_reexport)
                let box_ptr: *const (CrateModel, ReachableInfo) = &*source_models[ci];
                let (sub_model, sub_reachable) = unsafe { &*box_ptr };
                if let Some(root) = sub_model.krate.index.get(&root_id) {
                    walk_accessible(
                        sub_model,
                        root,
                        &alias_path,
                        Some(ci),
                        sub_reachable,
                        depth + 1,
                        visited_crates,
                        source_models,
                        items,
                        ctx,
                    );
                }
            }
        }
    }
}

/// Load a source crate model, or find its index if already loaded.
fn load_or_find_source_crate(
    source_crate_name: &str,
    source_models: &mut Vec<Box<(CrateModel, ReachableInfo)>>,
    visited_crates: &mut HashSet<String>,
    ctx: &WalkParams,
) -> Option<usize> {
    // Check if already loaded
    for (i, entry) in source_models.iter().enumerate() {
        let model = &entry.0;
        if model.crate_name() == source_crate_name {
            return Some(i);
        }
    }

    // Validate: skip if name isn't a known package (avoids trying to generate
    // rustdoc JSON for intra-crate module names like "graph_map" or "crate")
    if !ctx.available_packages.is_empty() {
        let hyphenated = source_crate_name.replace('_', "-");
        if !ctx.available_packages.contains(source_crate_name)
            && !ctx.available_packages.contains(&hyphenated)
        {
            return None;
        }
    }

    // Resolve to version-qualified spec if multi-version (e.g. "hashbrown@0.16.1")
    let resolved_name = ctx
        .available_packages
        .resolve_spec(source_crate_name)
        .unwrap_or_else(|| source_crate_name.to_string());

    // Not yet loaded — generate JSON and parse
    let use_cache = !ctx.workspace_members.contains(source_crate_name)
        && !ctx
            .workspace_members
            .contains(&source_crate_name.replace('_', "-"));

    let json_path = rustdoc_json::generate_rustdoc_json(
        &resolved_name,
        ctx.toolchain,
        ctx.manifest_path,
        true, // document private items
        ctx.target_dir,
        ctx.verbose,
        use_cache,
    )
    .ok()
    .or_else(|| {
        // Fallback: try hyphenated name (only if resolve_spec didn't already do this)
        if !resolved_name.contains('-') {
            let hyphenated = resolved_name.replace('_', "-");
            if hyphenated != resolved_name {
                return rustdoc_json::generate_rustdoc_json(
                    &hyphenated,
                    ctx.toolchain,
                    ctx.manifest_path,
                    true,
                    ctx.target_dir,
                    ctx.verbose,
                    use_cache,
                )
                .ok();
            }
        }
        None
    })?;

    let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path).ok()?;
    let model = CrateModel::from_crate(krate);

    visited_crates.insert(source_crate_name.to_string());

    let reachable = compute_reachable_set(&model);
    let ci = source_models.len();
    source_models.push(Box::new((model, reachable)));

    Some(ci)
}

/// Find an item by name in a model's root and register it.
fn find_and_register_item(
    model: &CrateModel,
    item_name: &str,
    accessible_path: &str,
    crate_idx: usize,
    items: &mut Vec<AccessibleEntry>,
) {
    let Some(root) = model.root_module() else {
        return;
    };
    for (child_id, child) in model.module_children(root) {
        let name = child.name.as_deref().or(match &child.inner {
            ItemEnum::Use(u) => Some(u.name.as_str()),
            _ => None,
        });
        if name == Some(item_name)
            && let Some(kind) = AccessibleItemKind::from_item(child)
        {
            items.push(AccessibleEntry {
                accessible_path: accessible_path.to_string(),
                crate_idx,
                item_id: Id(child_id.0),
                item_kind: kind,
            });
            return;
        }
    }
}

/// Join path segments, handling empty prefix.
fn join_path(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

/// Dedup entries: group by (crate_idx, item_id), keep shortest non-prelude path.
fn dedup_accessible_entries(items: &mut Vec<AccessibleEntry>) {
    use std::collections::HashMap;

    // Group by (crate_idx, item_id) — keep best entry index
    let mut best: HashMap<(usize, u32), usize> = HashMap::new();

    for (i, entry) in items.iter().enumerate() {
        let key = (entry.crate_idx, entry.item_id.0);
        let dominated = if let Some(&existing_idx) = best.get(&key) {
            let existing = &items[existing_idx];
            prefer_path(&entry.accessible_path, &existing.accessible_path)
        } else {
            true // no existing entry
        };
        if dominated {
            best.insert(key, i);
        }
    }

    let keep: HashSet<usize> = best.into_values().collect();
    let mut idx = 0;
    items.retain(|_| {
        let k = keep.contains(&idx);
        idx += 1;
        k
    });
}

/// Returns true if `candidate` should replace `existing`.
/// Prefers non-prelude paths; among those, shorter wins.
fn prefer_path(candidate: &str, existing: &str) -> bool {
    let candidate_prelude = candidate.split("::").any(|seg| seg == "prelude");
    let existing_prelude = existing.split("::").any(|seg| seg == "prelude");

    match (candidate_prelude, existing_prelude) {
        (false, true) => true,  // candidate has no prelude, existing does
        (true, false) => false, // candidate has prelude, existing doesn't
        _ => candidate.len() < existing.len(), // both same prelude status: shorter wins
    }
}

// === Internal helpers ===

/// Extract the crate name from a use source path like "bevy_internal" or "bevy_ecs::system".
pub(crate) fn extract_crate_name(source: &str) -> String {
    source.split("::").next().unwrap_or(source).to_string()
}

/// Check if a use source path is intra-crate (starts with "self::" or matches crate name).
pub(crate) fn is_intra_crate_source(source: &str, crate_name: &str) -> bool {
    source.starts_with("self::") || extract_crate_name(source) == crate_name
}

/// Follow a re-export chain from a Use source to the leaf crate.
///
/// `source` is like "bevy_internal::ecs" — we extract the crate name,
/// generate its JSON, and look for the target within it.
/// Max 5 hops to prevent infinite loops.
fn follow_use_chain(
    source: &str,
    _display_name: &str,
    rest: Option<String>,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
    verbose: bool,
) -> Option<CrossCrateResolution> {
    let mut visited = HashSet::new();
    let mut current_source = source.to_string();

    for _ in 0..5 {
        let crate_name = extract_crate_name(&current_source);
        if !visited.insert(crate_name.clone()) {
            break; // cycle detected
        }

        let json_path = rustdoc_json::generate_rustdoc_json(
            &crate_name,
            toolchain,
            manifest_path,
            true,
            target_dir,
            verbose,
            true, // cross-crate — use cache
        )
        .ok()?;
        let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path).ok()?;
        let model = CrateModel::from_crate(krate);

        // Check if source has a sub-path (e.g., "bevy_internal::ecs")
        let sub_path: Option<String> = current_source.split_once("::").map(|(_, p)| p.to_string());

        if let Some(sub) = sub_path {
            // The source points to a specific item within the crate.
            // Check if it's a module.
            if model.find_module(&sub).is_some() {
                return Some(CrossCrateResolution {
                    model,
                    inner_module_path: if let Some(r) = &rest {
                        Some(format!("{sub}::{r}"))
                    } else {
                        Some(sub)
                    },
                });
            }

            // Check if the sub-path's first segment is a re-export
            let first_sub: String = sub.split("::").next().unwrap_or(&sub).to_string();
            let mut found_next = false;
            if let Some(root) = model.root_module() {
                for (_id, child) in model.module_children(root) {
                    let ItemEnum::Use(u) = &child.inner else {
                        continue;
                    };
                    if u.is_glob {
                        continue;
                    }
                    let n = child.name.as_deref().unwrap_or(&u.name);
                    if n == first_sub {
                        current_source = u.source.clone();
                        found_next = true;
                        break;
                    }
                }
            }
            if !found_next {
                // Can't follow further — return the model with the rest path
                return Some(CrossCrateResolution {
                    model,
                    inner_module_path: rest,
                });
            }
        } else {
            // Source is just a crate name — the model IS the target
            return Some(CrossCrateResolution {
                model,
                inner_module_path: rest,
            });
        }
    }

    None
}
