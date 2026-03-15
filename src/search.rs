//! Search mode: find leaf items by name across the entire crate.
//!
//! Walks all items recursively, matches against a pattern (case-insensitive,
//! multi-word AND on full path), and renders each match as a one-liner with
//! a kind prefix and full path.

use rustdoc_types::{Id, Item, ItemEnum, Struct, StructKind, Type, VariantKind, Visibility};

use crate::cli::BriefArgs;
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
}

/// Run search mode: find all leaf items matching the pattern and render them.
pub fn render_search(
    model: &CrateModel,
    pattern: &str,
    args: &BriefArgs,
    observer_module_path: Option<&str>,
    same_crate: bool,
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
        walk_module(model, root, "", args, &observer, same_crate, &mut leaves);
    }

    // Parse search tokens (case-insensitive, AND-matched)
    let tokens: Vec<String> = pattern
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();

    // Filter by pattern
    let matched: Vec<&LeafItem> = leaves
        .iter()
        .filter(|leaf| {
            let path_lower = leaf.path.to_lowercase();
            tokens.iter().all(|tok| path_lower.contains(tok.as_str()))
        })
        .collect();

    // Render
    let mut output = format!(
        "// crate {crate_name} — search: \"{pattern}\" ({} results)\n",
        matched.len()
    );

    for leaf in &matched {
        render_leaf(&mut output, model, leaf);
    }

    output
}

/// Walk a module recursively, collecting all leaf items.
fn walk_module<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &BriefArgs,
    observer: &str,
    same_crate: bool,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Visibility check
        if !matches!(child.visibility, Visibility::Default)
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
                    leaves,
                );
            }
            ItemEnum::Struct(s) if !args.no_structs => {
                // Struct itself is a leaf (name match)
                leaves.push(LeafItem {
                    path: child_path.clone(),
                    item: child,
                    kind: LeafKind::Struct,
                    context: LeafContext::None,
                });
                // Named struct fields are leaves
                walk_struct_fields(model, s, &child_path, observer, same_crate, leaves);
            }
            ItemEnum::Enum(e) if !args.no_enums => {
                leaves.push(LeafItem {
                    path: child_path.clone(),
                    item: child,
                    kind: LeafKind::Enum,
                    context: LeafContext::None,
                });
                // Enum variants are leaves
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
            ItemEnum::Trait(t) if !args.no_traits => {
                leaves.push(LeafItem {
                    path: child_path.clone(),
                    item: child,
                    kind: LeafKind::Trait,
                    context: LeafContext::None,
                });
                // Trait items are leaves
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
            ItemEnum::Union(u) if !args.no_unions => {
                leaves.push(LeafItem {
                    path: child_path.clone(),
                    item: child,
                    kind: LeafKind::Union,
                    context: LeafContext::None,
                });
                // Union fields are leaves
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
fn walk_impl_blocks<'a>(
    model: &'a CrateModel,
    module_item: &'a Item,
    parent_path: &str,
    args: &BriefArgs,
    observer: &str,
    same_crate: bool,
    leaves: &mut Vec<LeafItem<'a>>,
) {
    let children = model.module_children(module_item);
    let mut impl_ids: Vec<Id> = Vec::new();

    for (child_id, child) in &children {
        if !matches!(child.visibility, Visibility::Default)
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
fn render_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem) {
    // Doc comment: first line only
    if let Some(docs) = &leaf.item.docs
        && let Some(first_line) = docs.lines().next()
        && !first_line.is_empty()
    {
        output.push_str(&format!("/// {first_line}\n"));
    }

    match leaf.kind {
        LeafKind::Function => render_function_leaf(output, leaf),
        LeafKind::Struct => render_struct_leaf(output, model, leaf),
        LeafKind::Enum => {
            output.push_str(&format!("enum {};\n", leaf.path));
        }
        LeafKind::Trait => {
            output.push_str(&format!("trait {};\n", leaf.path));
        }
        LeafKind::Union => {
            output.push_str(&format!("union {};\n", leaf.path));
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
    }
}

fn render_function_leaf(output: &mut String, leaf: &LeafItem) {
    if let ItemEnum::Function(f) = &leaf.item.inner {
        let sig = render::format_function_sig_pub(
            &leaf.path, f, "", // no visibility prefix in search results
        );
        output.push_str(&format!("{sig};\n"));
    }
}

fn render_struct_leaf(output: &mut String, model: &CrateModel, leaf: &LeafItem) {
    let ItemEnum::Struct(s) = &leaf.item.inner else {
        return;
    };
    let generics = render::format_generics_pub(&s.generics);

    match &s.kind {
        StructKind::Unit => {
            output.push_str(&format!("struct {}{};\n", leaf.path, generics));
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
                "struct {}{}({});\n",
                leaf.path,
                generics,
                field_strs.join(", ")
            ));
        }
        StructKind::Plain { .. } => {
            output.push_str(&format!("struct {}{} {{ .. }};\n", leaf.path, generics));
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
        output.push_str(&format!(
            "type {}{generics} = {};\n",
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
