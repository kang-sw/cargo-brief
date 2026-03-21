//! Search mode: find leaf items by name across the entire crate.
//!
//! Walks all items recursively, matches against a pattern (smart-case,
//! multi-word AND on full path, comma-separated OR groups), and renders
//! each match as a one-liner with a kind prefix and full path.

use std::collections::HashSet;

use rustdoc_types::{
    Attribute, Id, Item, ItemEnum, Struct, StructKind, Type, VariantKind, Visibility,
};

use crate::cli::FilterArgs;
use crate::model::{CrateModel, is_visible_from};
use crate::render;

/// A leaf item discovered by the walker, with its full display path and kind.
struct LeafItem<'a> {
    /// Full path without crate prefix, e.g. `outer::PubStruct::pub_field`
    path: String,
    item: &'a Item,
    kind: LeafKind,
    /// Extra rendering context (e.g. parent type name for fields/variants)
    context: LeafContext<'a>,
}

enum LeafKind {
    Function,
    Struct,
    Enum,
    Trait,
    Union,
    Field,
    Variant,
    Constant,
    Static,
    TypeAlias,
    Macro,
    AssocType,
    AssocConst,
    Use,
}

impl LeafKind {
    /// Match against a user-provided kind string (e.g. "fn", "struct").
    fn matches_kind_str(&self, s: &str) -> bool {
        matches!(
            (self, s),
            (LeafKind::Function, "fn")
                | (LeafKind::Struct, "struct")
                | (LeafKind::Enum, "enum")
                | (LeafKind::Trait, "trait")
                | (LeafKind::Union, "union")
                | (LeafKind::Field, "field")
                | (LeafKind::Variant, "variant")
                | (LeafKind::Constant, "const")
                | (LeafKind::Static, "static")
                | (LeafKind::TypeAlias, "type")
                | (LeafKind::Macro, "macro")
                | (LeafKind::AssocType, "type")
                | (LeafKind::AssocConst, "const")
                | (LeafKind::Use, "use")
        )
    }

    /// Sort key: determines display order by kind.
    fn sort_key(&self) -> u8 {
        match self {
            LeafKind::Function => 0,
            LeafKind::Struct => 1,
            LeafKind::Enum => 2,
            LeafKind::Trait => 3,
            LeafKind::Union => 4,
            LeafKind::Field => 5,
            LeafKind::Variant => 6,
            LeafKind::Constant => 7,
            LeafKind::Static => 8,
            LeafKind::TypeAlias => 9,
            LeafKind::Macro => 10,
            LeafKind::AssocType => 11,
            LeafKind::AssocConst => 12,
            LeafKind::Use => 13,
        }
    }
}

/// Extra context needed to render certain leaf types.
enum LeafContext<'a> {
    None,
    /// For fields: the parent struct
    Field {
        field_type: &'a Type,
    },
    /// For enum variants: variant kind data
    Variant,
    /// For functions in impl blocks: extra qualifiers
    ImplMethod,
    /// For associated types in impl blocks
    AssocType,
    /// For associated consts in impl blocks
    AssocConst,
    /// For re-exports: the source path
    Use {
        source: String,
    },
}

/// Parse `--search-limit` value: `"N"` → (0, Some(N)), `"M:N"` → (M, Some(N)), `None` → (0, None).
fn parse_search_limit(raw: Option<&str>) -> (usize, Option<usize>) {
    let Some(raw) = raw else {
        return (0, None);
    };
    if let Some((offset_str, limit_str)) = raw.split_once(':') {
        let offset = offset_str.parse().unwrap_or(0);
        let limit = limit_str.parse().unwrap_or(0);
        (offset, Some(limit))
    } else {
        let limit = raw.parse().unwrap_or(0);
        (0, Some(limit))
    }
}

/// Run search mode: find all leaf items matching the pattern and render them.
pub fn render_search(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        None,
        None,
    )
}

/// Like `render_search`, but with optional exact-parent filtering for `--methods-of`.
/// When `methods_of` is Some, only items whose parent path segment exactly matches
/// the type name are included.
pub fn render_search_methods_of(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
    methods_of: &str,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        Some(methods_of),
        None,
    )
}

/// Full search with all options including kind filter.
pub fn render_search_filtered(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
    methods_of: Option<&str>,
    search_kind: Option<&str>,
) -> String {
    render_search_inner(
        model,
        pattern,
        filter,
        limit,
        observer_module_path,
        same_crate,
        reachable,
        methods_of,
        search_kind,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_search_inner(
    model: &CrateModel,
    pattern: &str,
    filter: &FilterArgs,
    limit: Option<&str>,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
    methods_of: Option<&str>,
    search_kind: Option<&str>,
) -> String {
    let crate_name = model.crate_name();
    let observer = observer_module_path
        .map(|p| {
            if p.contains("::") || p == crate_name {
                p.to_string()
            } else {
                format!("{crate_name}::{p}")
            }
        })
        .unwrap_or_else(|| crate_name.to_string());

    // Collect all leaf items
    let mut leaves = Vec::new();
    if let Some(root) = model.root_module() {
        walk_module(
            model,
            root,
            "",
            filter,
            &observer,
            same_crate,
            reachable,
            &mut leaves,
        );
    }

    // Smart-case: all-lowercase → case-insensitive, any uppercase → case-sensitive
    let case_sensitive = pattern.chars().any(|c| c.is_uppercase());

    // Comma-separated OR groups, each group is AND of space-separated tokens.
    // Empty groups (from trailing commas) are filtered out to avoid matching everything.
    let or_groups: Vec<Vec<String>> = pattern
        .split(',')
        .map(|group| {
            group
                .split_whitespace()
                .map(|t| {
                    if case_sensitive {
                        t.to_string()
                    } else {
                        t.to_lowercase()
                    }
                })
                .collect()
        })
        .filter(|group: &Vec<String>| !group.is_empty())
        .collect();

    // Filter by pattern
    let mut matched: Vec<&LeafItem> = leaves
        .iter()
        .filter(|leaf| {
            let path = if case_sensitive {
                leaf.path.clone()
            } else {
                leaf.path.to_lowercase()
            };
            or_groups
                .iter()
                .any(|group| group.iter().all(|tok| path.contains(tok.as_str())))
        })
        .collect();

    // --methods-of: exact parent-type segment matching
    if let Some(type_name) = methods_of {
        let suffix = format!("::{type_name}::");
        let prefix = format!("{type_name}::");
        matched.retain(|leaf| leaf.path.contains(&suffix) || leaf.path.starts_with(&prefix));
    }

    // --search-kind: include only matching kinds
    if let Some(kind_spec) = search_kind {
        let kinds: Vec<&str> = kind_spec.split(',').map(|s| s.trim()).collect();
        matched.retain(|leaf| kinds.iter().any(|k| leaf.kind.matches_kind_str(k)));
    }

    // Sort: primary by kind, secondary by path alphabetically
    matched.sort_by(|a, b| {
        a.kind
            .sort_key()
            .cmp(&b.kind.sort_key())
            .then_with(|| a.path.cmp(&b.path))
    });

    let total = matched.len();
    let (offset, search_limit) = parse_search_limit(limit);
    let offset = offset.min(total);
    let end = search_limit
        .map(|n| (offset + n).min(total))
        .unwrap_or(total);
    let skipped_before = offset;
    let skipped_after = total - end;

    // Render
    let mut output = format!("// crate {crate_name} — search: \"{pattern}\" ({total} results)\n",);

    if skipped_before > 0 {
        output.push_str(&format!("// (skipped {skipped_before} results)\n"));
    }

    for leaf in &matched[offset..end] {
        render_leaf(&mut output, model, leaf, filter);
    }

    if skipped_after > 0 {
        output.push_str(&format!("// ... and {skipped_after} more results\n"));
    }

    if total == 0 {
        let word_count = pattern.split_whitespace().count();
        if word_count >= 4 {
            output.push_str(&format!(
                "// hint: search uses AND matching — all {word_count} words must appear in one item's path. Try fewer words.\n"
            ));
        }
    }

    output
}

/// Walk a module recursively, collecting all leaf items.
#[allow(clippy::too_many_arguments)]
fn walk_module<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Visibility check
        if let Some(reachable) = reachable {
            if !reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }

        let name = child
            .name
            .as_deref()
            .or(if let ItemEnum::Use(u) = &child.inner {
                Some(u.name.as_str())
            } else {
                None
            });
        let Some(name) = name else { continue };

        let child_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{parent_path}::{name}")
        };

        match &child.inner {
            ItemEnum::Module(_) => {
                walk_module(
                    model,
                    child,
                    &child_path,
                    args,
                    observer,
                    same_crate,
                    reachable,
                    leaves,
                );
            }
            ItemEnum::Struct(s) => {
                if !args.no_structs {
                    // Struct itself is a leaf (name match)
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Struct,
                        context: LeafContext::None,
                    });
                }
                // Named struct fields are always walked (needed for --methods-of)
                walk_struct_fields(model, s, &child_path, observer, same_crate, leaves);
            }
            ItemEnum::Enum(e) => {
                if !args.no_enums {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Enum,
                        context: LeafContext::None,
                    });
                }
                // Enum variants are always walked (needed for --methods-of)
                for variant_id in &e.variants {
                    if let Some(variant_item) = model.krate.index.get(variant_id) {
                        let vname = variant_item.name.as_deref().unwrap_or("?");
                        leaves.push(LeafItem {
                            path: format!("{child_path}::{vname}"),
                            item: variant_item,
                            kind: LeafKind::Variant,
                            context: LeafContext::Variant,
                        });
                    }
                }
            }
            ItemEnum::Trait(t) => {
                if !args.no_traits {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Trait,
                        context: LeafContext::None,
                    });
                }
                // Trait items are always walked (needed for --methods-of)
                for item_id in &t.items {
                    if let Some(trait_item) = model.krate.index.get(item_id) {
                        let iname = trait_item.name.as_deref().unwrap_or("?");
                        let item_path = format!("{child_path}::{iname}");
                        match &trait_item.inner {
                            ItemEnum::Function(_) if !args.no_functions => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::Function,
                                    context: LeafContext::ImplMethod,
                                });
                            }
                            ItemEnum::AssocType { .. } if !args.no_aliases => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::AssocType,
                                    context: LeafContext::AssocType,
                                });
                            }
                            ItemEnum::AssocConst { .. } if !args.no_constants => {
                                leaves.push(LeafItem {
                                    path: item_path,
                                    item: trait_item,
                                    kind: LeafKind::AssocConst,
                                    context: LeafContext::AssocConst,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            ItemEnum::Function(_) if !args.no_functions => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Function,
                    context: LeafContext::None,
                });
            }
            ItemEnum::TypeAlias(_) if !args.no_aliases => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::TypeAlias,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Constant { .. } if !args.no_constants => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Constant,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Static(_) if !args.no_constants => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Static,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Union(u) => {
                if !args.no_unions {
                    leaves.push(LeafItem {
                        path: child_path.clone(),
                        item: child,
                        kind: LeafKind::Union,
                        context: LeafContext::None,
                    });
                }
                // Union fields are always walked (needed for --methods-of)
                for field_id in &u.fields {
                    if let Some(field_item) = model.krate.index.get(field_id) {
                        if !is_visible_from(model, field_item, field_id, observer, same_crate)
                            && !matches!(field_item.visibility, Visibility::Public)
                        {
                            continue;
                        }
                        if let ItemEnum::StructField(ty) = &field_item.inner {
                            let fname = field_item.name.as_deref().unwrap_or("?");
                            leaves.push(LeafItem {
                                path: format!("{child_path}::{fname}"),
                                item: field_item,
                                kind: LeafKind::Field,
                                context: LeafContext::Field { field_type: ty },
                            });
                        }
                    }
                }
            }
            ItemEnum::Macro(_) if !args.no_macros => {
                leaves.push(LeafItem {
                    path: child_path,
                    item: child,
                    kind: LeafKind::Macro,
                    context: LeafContext::None,
                });
            }
            ItemEnum::Use(use_item) if !use_item.is_glob => {
                // Follow the re-export target to check --no-* filters
                let target = use_item
                    .id
                    .as_ref()
                    .and_then(|id| model.krate.index.get(id));
                let skip = target
                    .map(|t| match &t.inner {
                        ItemEnum::Function(_) => args.no_functions,
                        ItemEnum::Struct(_) => args.no_structs,
                        ItemEnum::Enum(_) => args.no_enums,
                        ItemEnum::Trait(_) => args.no_traits,
                        ItemEnum::Union(_) => args.no_unions,
                        ItemEnum::Constant { .. } => args.no_constants,
                        ItemEnum::Static(_) => args.no_constants,
                        ItemEnum::TypeAlias(_) => args.no_aliases,
                        ItemEnum::Macro(_) => args.no_macros,
                        _ => false,
                    })
                    .unwrap_or(false);
                if !skip {
                    let source = &use_item.source;
                    leaves.push(LeafItem {
                        path: child_path,
                        item: child,
                        kind: LeafKind::Use,
                        context: LeafContext::Use {
                            source: source.clone(),
                        },
                    });
                }
            }
            _ => {}
        }
    }

    // Walk impl blocks for types in this module
    walk_impl_blocks(
        model,
        module_item,
        parent_path,
        args,
        observer,
        same_crate,
        reachable,
        leaves,
    );
}

/// Walk named struct fields as leaf items.
fn walk_struct_fields<'a>(
    model: &'a CrateModel,
    s: &Struct,
    struct_path: &str,
    observer: &str,
    same_crate: bool,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    if let StructKind::Plain { fields, .. } = &s.kind {
        for field_id in fields {
            if let Some(field_item) = model.krate.index.get(field_id) {
                if !is_visible_from(model, field_item, field_id, observer, same_crate)
                    && !matches!(field_item.visibility, Visibility::Public)
                {
                    continue;
                }
                if let ItemEnum::StructField(ty) = &field_item.inner {
                    let fname = field_item.name.as_deref().unwrap_or("?");
                    leaves.push(LeafItem {
                        path: format!("{struct_path}::{fname}"),
                        item: field_item,
                        kind: LeafKind::Field,
                        context: LeafContext::Field { field_type: ty },
                    });
                }
            }
        }
    }
}

/// Walk impl blocks for types defined in this module.
#[allow(clippy::too_many_arguments)]
fn walk_impl_blocks<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    reachable: Option<&HashSet<Id>>,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);
    let mut impl_ids: Vec<Id> = Vec::new();

    for (child_id, child) in &children {
        if let Some(reachable) = reachable {
            if !reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }
        let impls = match &child.inner {
            ItemEnum::Struct(s) => &s.impls,
            ItemEnum::Enum(e) => &e.impls,
            ItemEnum::Union(u) => &u.impls,
            _ => continue,
        };
        impl_ids.extend(impls.iter().cloned());
    }

    for impl_id in &impl_ids {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        let type_name = render::format_type_pub(&impl_block.for_);
        if type_name.is_empty() {
            continue;
        }

        let is_trait_impl = impl_block.trait_.is_some();

        // Build the prefix: parent_path::TypeName
        let type_path = if parent_path.is_empty() {
            type_name.clone()
        } else {
            format!("{parent_path}::{type_name}")
        };

        for item_id in &impl_block.items {
            let Some(item) = model.krate.index.get(item_id) else {
                continue;
            };

            // Visibility check for inherent impl items
            if !is_trait_impl
                && !matches!(item.visibility, Visibility::Default | Visibility::Public)
                && !is_visible_from(model, item, item_id, observer, same_crate)
            {
                continue;
            }

            let iname = item.name.as_deref().unwrap_or("?");
            let item_path = format!("{type_path}::{iname}");

            match &item.inner {
                ItemEnum::Function(_) if !args.no_functions => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::Function,
                        context: LeafContext::ImplMethod,
                    });
                }
                ItemEnum::AssocType { .. } if !args.no_aliases => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocType,
                        context: LeafContext::AssocType,
                    });
                }
                ItemEnum::AssocConst { .. } if !args.no_constants => {
                    leaves.push(LeafItem {
                        path: item_path,
                        item,
                        kind: LeafKind::AssocConst,
                        context: LeafContext::AssocConst,
                    });
                }
                _ => {}
            }
        }
    }
}

// === Rendering ===

/// Render a single leaf item as a one-liner.
fn render_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem, args: &FilterArgs) {
    // Doc comment: first line only (suppressed by --no-docs / --compact)
    if !args.no_docs
        && !args.compact
        && let Some(docs) = &leaf.item.docs
        && let Some(first_line) = docs.lines().next()
        && !first_line.is_empty()
    {
        output.push_str(&format!("/// {first_line}\n"));
    }

    // Attribute markers for search results (deprecated / non_exhaustive only)
    if leaf.item.deprecation.is_some() {
        output.push_str("[deprecated] ");
    }
    if leaf
        .item
        .attrs
        .iter()
        .any(|a| matches!(a, Attribute::NonExhaustive))
    {
        output.push_str("[non_exhaustive] ");
    }

    match leaf.kind {
        LeafKind::Function => render_function_leaf(output, leaf),
        LeafKind::Struct => render_struct_leaf(output, model, leaf, args),
        LeafKind::Enum => {
            let summary = collect_impl_summary(model, leaf.item, args);
            if summary.is_empty() {
                output.push_str(&format!("enum {};\n", leaf.path));
            } else {
                output.push_str(&format!("enum {}; {summary}\n", leaf.path));
            }
        }
        LeafKind::Trait => {
            output.push_str(&format!("trait {};\n", leaf.path));
        }
        LeafKind::Union => {
            let summary = collect_impl_summary(model, leaf.item, args);
            if summary.is_empty() {
                output.push_str(&format!("union {};\n", leaf.path));
            } else {
                output.push_str(&format!("union {}; {summary}\n", leaf.path));
            }
        }
        LeafKind::Field => render_field_leaf(output, leaf),
        LeafKind::Variant => render_variant_leaf(output, model, leaf),
        LeafKind::Constant => render_constant_leaf(output, leaf),
        LeafKind::Static => render_static_leaf(output, leaf),
        LeafKind::TypeAlias => render_type_alias_leaf(output, leaf),
        LeafKind::Macro => {
            output.push_str(&format!("macro {}!;\n", leaf.path));
        }
        LeafKind::AssocType => render_assoc_type_leaf(output, leaf),
        LeafKind::AssocConst => render_assoc_const_leaf(output, leaf),
        LeafKind::Use => render_use_leaf(output, leaf),
    }
}

fn render_function_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Function(f) = &leaf.item.inner {
        let sig = render::format_function_sig_pub(
            &leaf.path, f, "", // no visibility prefix in search results
        );
        let wc = render::format_where_clause_compact_pub(&f.generics);
        output.push_str(&format!("{sig}{wc};\n"));
    }
}

fn render_struct_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem, args: &FilterArgs) {
    let ItemEnum::Struct(s) = &leaf.item.inner else {
        return;
    };
    let generics = render::format_generics_pub(&s.generics);
    let wc = render::format_where_clause_compact_pub(&s.generics);
    let summary = collect_impl_summary(model, leaf.item, args);
    let suffix = if summary.is_empty() {
        String::new()
    } else {
        format!(" {summary}")
    };

    match &s.kind {
        StructKind::Unit => {
            output.push_str(&format!("struct {}{}{wc};\n", leaf.path, generics));
        }
        StructKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                render::format_type_pub(ty)
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "_".to_string())
                })
                .collect();
            output.push_str(&format!(
                "struct {}{}({}){wc};{suffix}\n",
                leaf.path,
                generics,
                field_strs.join(", ")
            ));
        }
        StructKind::Plain { .. } => {
            output.push_str(&format!(
                "struct {}{}{wc} {{ .. }};{suffix}\n",
                leaf.path, generics
            ));
        }
    }
}

fn render_field_leaf(output: &mut String, leaf: &LeafItem) {
    if let LeafContext::Field { field_type } = &leaf.context {
        output.push_str(&format!(
            "field {}: {};\n",
            leaf.path,
            render::format_type_pub(field_type)
        ));
    }
}

fn render_variant_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem) {
    let ItemEnum::Variant(variant) = &leaf.item.inner else {
        return;
    };
    match &variant.kind {
        VariantKind::Plain => {
            output.push_str(&format!("variant {};\n", leaf.path));
        }
        VariantKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                render::format_type_pub(ty)
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "_".to_string())
                })
                .collect();
            output.push_str(&format!(
                "variant {}({});\n",
                leaf.path,
                field_strs.join(", ")
            ));
        }
        VariantKind::Struct { fields, .. } => {
            let field_strs: Vec<String> = fields
                .iter()
                .filter_map(|fid| model.krate.index.get(fid))
                .filter_map(|f| {
                    if let ItemEnum::StructField(ty) = &f.inner {
                        let fname = f.name.as_deref().unwrap_or("?");
                        Some(format!("{fname}: {}", render::format_type_pub(ty)))
                    } else {
                        None
                    }
                })
                .collect();
            output.push_str(&format!(
                "variant {} {{ {} }};\n",
                leaf.path,
                field_strs.join(", ")
            ));
        }
    }
}

fn render_constant_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Constant { type_, const_: c } = &leaf.item.inner {
        let val = c.value.as_deref().unwrap_or("..");
        output.push_str(&format!(
            "const {}: {} = {val};\n",
            leaf.path,
            render::format_type_pub(type_)
        ));
    }
}

fn render_static_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Static(s) = &leaf.item.inner {
        let mutability = if s.is_mutable { "mut " } else { "" };
        output.push_str(&format!(
            "static {mutability}{}: {};\n",
            leaf.path,
            render::format_type_pub(&s.type_)
        ));
    }
}

fn render_type_alias_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::TypeAlias(ta) = &leaf.item.inner {
        let generics = render::format_generics_pub(&ta.generics);
        let wc = render::format_where_clause_compact_pub(&ta.generics);
        output.push_str(&format!(
            "type {}{generics}{wc} = {};\n",
            leaf.path,
            render::format_type_pub(&ta.type_)
        ));
    }
}

fn render_assoc_type_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::AssocType { type_, .. } = &leaf.item.inner {
        if let Some(ty) = type_ {
            output.push_str(&format!(
                "type {} = {};\n",
                leaf.path,
                render::format_type_pub(ty)
            ));
        } else {
            output.push_str(&format!("type {};\n", leaf.path));
        }
    }
}

fn render_assoc_const_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::AssocConst { type_, value } = &leaf.item.inner {
        let val = value.as_deref().unwrap_or("..");
        output.push_str(&format!(
            "const {}: {} = {val};\n",
            leaf.path,
            render::format_type_pub(type_)
        ));
    }
}

fn render_use_leaf(output: &mut String, leaf: &LeafItem) {
    if let LeafContext::Use { source } = &leaf.context {
        let name = leaf.path.rsplit("::").next().unwrap_or(&leaf.path);
        // Check if it's an alias (name differs from source's last segment)
        let source_name = source.rsplit("::").next().unwrap_or(source);
        if name == source_name {
            output.push_str(&format!("use {source};\n"));
        } else {
            output.push_str(&format!("use {source} as {name};\n"));
        }
    }
}

/// Build an inline `// impl (N methods), impl Trait1, impl Trait2` summary for a type.
fn collect_impl_summary(model: &CrateModel, item: &Item, args: &FilterArgs) -> String {
    let impls = match &item.inner {
        ItemEnum::Struct(s) => &s.impls,
        ItemEnum::Enum(e) => &e.impls,
        ItemEnum::Union(u) => &u.impls,
        _ => return String::new(),
    };

    let mut parts: Vec<String> = Vec::new();

    for impl_id in impls {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        if let Some(trait_) = &impl_block.trait_ {
            parts.push(format!("impl {}", render::format_path_pub(trait_)));
        } else {
            // Inherent impl: count visible methods
            let method_count = impl_block
                .items
                .iter()
                .filter(|id| {
                    model
                        .krate
                        .index
                        .get(id)
                        .map(|i| matches!(i.inner, ItemEnum::Function(_)))
                        .unwrap_or(false)
                })
                .count();
            if method_count > 0 {
                parts.push(format!("impl ({method_count} methods)"));
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    if parts.len() > 5 {
        let extra = parts.len() - 5;
        parts.truncate(5);
        parts.push(format!("+ {extra} more"));
    }

    format!("// {}", parts.join(", "))
}
