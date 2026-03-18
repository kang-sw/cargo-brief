//! Cross-crate module following for facade crates.
//!
//! Facade crates like `bevy` re-export modules from sub-crates:
//! `bevy → pub use bevy_internal::* → pub use bevy_ecs as ecs`.
//! This module resolves such chains by generating rustdoc JSON for
//! intermediate crates and following Use items.

use std::collections::HashSet;
use std::path::Path;

use rustdoc_types::{ItemEnum, Visibility};

use crate::model::CrateModel;
use crate::rustdoc_json;

/// Result of resolving a cross-crate module path.
pub struct CrossCrateResolution {
    /// The CrateModel of the resolved sub-crate.
    pub model: CrateModel,
    /// Remaining module path within the sub-crate (e.g., "system" from "ecs::system").
    pub inner_module_path: Option<String>,
}

/// A discovered sub-crate re-exported from a facade crate.
pub struct SubCrate {
    /// Display name as seen from the facade (e.g., "ecs").
    pub display_name: String,
    /// The CrateModel of the sub-crate.
    pub model: CrateModel,
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
        let Ok(json_path) = rustdoc_json::generate_rustdoc_json_cached(
            &source_crate,
            toolchain,
            manifest_path,
            true,
            target_dir,
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
                ) {
                    return Some(resolution);
                }
            }
        }
    }

    None
}

/// Discover all cross-crate re-exported sub-crate models.
///
/// For `--search` and `--recursive` modes — enumerates all named Use items
/// at root level (through glob intermediaries) and builds models for each.
pub fn discover_all_reexported_crates(
    primary_model: &CrateModel,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
) -> Vec<SubCrate> {
    let crate_name = primary_model.crate_name();
    let Some(root) = primary_model.root_module() else {
        return Vec::new();
    };

    let children = primary_model.module_children(root);
    let mut results = Vec::new();
    let mut seen_names = HashSet::new();

    // Collect named Use items directly at root (skip intra-crate re-exports)
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
        if !seen_names.insert(name.to_string()) {
            continue;
        }

        if let Some(sub) =
            resolve_single_reexport(name, &use_item.source, toolchain, manifest_path, target_dir)
        {
            results.push(sub);
        }
    }

    // Follow glob re-exports to find named items in source crates
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

        let source_crate = extract_crate_name(&use_item.source);
        let Ok(json_path) = rustdoc_json::generate_rustdoc_json_cached(
            &source_crate,
            toolchain,
            manifest_path,
            true,
            target_dir,
        ) else {
            eprintln!("warning: failed to generate JSON for '{source_crate}', skipping");
            continue;
        };
        let Ok(krate) = rustdoc_json::parse_rustdoc_json_cached(&json_path) else {
            eprintln!("warning: failed to parse JSON for '{source_crate}', skipping");
            continue;
        };
        let source_model = CrateModel::from_crate(krate);

        let Some(sroot) = source_model.root_module() else {
            continue;
        };

        for (_sid, schild) in source_model.module_children(sroot) {
            let ItemEnum::Use(su) = &schild.inner else {
                continue;
            };
            if su.is_glob {
                continue;
            }
            if !matches!(schild.visibility, Visibility::Public) {
                continue;
            }

            let sname = schild.name.as_deref().unwrap_or(&su.name);
            if !seen_names.insert(sname.to_string()) {
                continue;
            }

            if let Some(sub) =
                resolve_single_reexport(sname, &su.source, toolchain, manifest_path, target_dir)
            {
                results.push(sub);
            }
        }
    }

    results
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

// === Internal helpers ===

/// Extract the crate name from a use source path like "bevy_internal" or "bevy_ecs::system".
fn extract_crate_name(source: &str) -> String {
    source.split("::").next().unwrap_or(source).to_string()
}

/// Check if a use source path is intra-crate (starts with "self::" or matches crate name).
fn is_intra_crate_source(source: &str, crate_name: &str) -> bool {
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
) -> Option<CrossCrateResolution> {
    let mut visited = HashSet::new();
    let mut current_source = source.to_string();

    for _ in 0..5 {
        let crate_name = extract_crate_name(&current_source);
        if !visited.insert(crate_name.clone()) {
            break; // cycle detected
        }

        let json_path = rustdoc_json::generate_rustdoc_json_cached(
            &crate_name,
            toolchain,
            manifest_path,
            true,
            target_dir,
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

/// Resolve a single named re-export to a SubCrate.
fn resolve_single_reexport(
    display_name: &str,
    source: &str,
    toolchain: &str,
    manifest_path: Option<&str>,
    target_dir: &Path,
) -> Option<SubCrate> {
    let mut visited = HashSet::new();
    let mut current_source = source.to_string();

    for _ in 0..5 {
        let crate_name = extract_crate_name(&current_source);
        if !visited.insert(crate_name.clone()) {
            break;
        }

        let json_path = rustdoc_json::generate_rustdoc_json_cached(
            &crate_name,
            toolchain,
            manifest_path,
            true,
            target_dir,
        )
        .ok()?;
        let krate = rustdoc_json::parse_rustdoc_json_cached(&json_path).ok()?;
        let model = CrateModel::from_crate(krate);

        let sub_path: Option<&str> = current_source.split_once("::").map(|(_, p)| p);

        if let Some(sub) = sub_path {
            // Check if sub is a module — if so, return this model with the path
            if model.find_module(sub).is_some() {
                return Some(SubCrate {
                    display_name: display_name.to_string(),
                    model,
                });
            }

            // Follow further re-exports
            let first_sub = sub.split("::").next().unwrap_or(sub);
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
                // Can't follow further — return what we have
                return Some(SubCrate {
                    display_name: display_name.to_string(),
                    model,
                });
            }
        } else {
            // Source is just a crate name — return its model directly
            return Some(SubCrate {
                display_name: display_name.to_string(),
                model,
            });
        }
    }

    None
}
