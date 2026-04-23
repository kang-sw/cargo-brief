use std::collections::{BTreeMap, HashSet};

use rustdoc_types::{
    Attribute, Constant, Enum, Function, GenericArg, GenericArgs, GenericBound,
    GenericParamDefKind, Id, Impl, Item, ItemEnum, ReprKind, Static, Struct, StructKind, Term,
    Trait, Type, TypeAlias, Union, VariantKind, Visibility, WherePredicate,
};

use crate::cfg_parse::{parse_cfg_attribute, reconstruct_cfg_attr};
use crate::cli::{ApiArgs, FilterArgs};
use crate::model::{CrateModel, ReachableInfo, is_visible_from};

/// Render the API of a target module as pseudo-Rust.
///
/// Returns an error if the target module path is specified but not found.
pub fn render_module_api(
    model: &CrateModel,
    target_module_path: Option<&str>,
    args: &ApiArgs,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
) -> String {
    let mut output = String::new();

    let crate_name = model.crate_name();
    output.push_str(&format!("// crate {crate_name}\n"));
    render_crate_docs(model, &args.filter, &mut output);

    let target_item = if let Some(path) = target_module_path {
        model.find_module(path)
    } else {
        model.root_module()
    };

    let Some(target_item) = target_item else {
        if let Some(path) = target_module_path {
            output.push_str(&format!("// ERROR: module '{path}' not found\n"));
            output.push_str("// Available modules:\n");
            let mut paths: Vec<&str> = model.module_index.keys().map(|s| s.as_str()).collect();
            paths.sort();
            for p in paths {
                output.push_str(&format!("//   {p}\n"));
            }
            output.push_str(&format!(
                "// TIP: Try `search {path}` to find items by name across the crate.\n"
            ));
        } else {
            output.push_str("// ERROR: crate root module not found\n");
        }
        return output;
    };

    let observer = observer_module_path
        .map(|p| {
            if p.contains("::") || p == crate_name {
                p.to_string()
            } else {
                format!("{crate_name}::{p}")
            }
        })
        .unwrap_or_else(|| crate_name.to_string());

    let depth = if args.recursive { u32::MAX } else { args.depth };

    let mod_display_path = target_module_path.unwrap_or(crate_name);

    render_module_contents(
        model,
        target_item,
        &args.filter,
        &observer,
        same_crate,
        depth,
        0,
        mod_display_path,
        reachable,
        &mut output,
    );

    output
}

/// Render full definitions from a source crate's root module for glob expansion inlining.
///
/// Iterates public, non-module items from the source model's root, renders each using
/// the existing `render_item()` functions. For `Use` items (re-exports from submodules),
/// follows the target to render the actual definition. Tracks rendered names in
/// `seen_names` for cross-source deduplication.
pub fn render_inlined_items(
    source_model: &CrateModel,
    args: &FilterArgs,
    seen_names: &mut HashSet<String>,
) -> String {
    let mut output = String::new();

    let Some(root) = source_model.root_module() else {
        return output;
    };

    let observer = source_model.crate_name().to_string();
    let children = source_model.module_children(root);

    // Collect impl IDs from inlined types for later rendering
    let mut impl_ids: Vec<Id> = Vec::new();

    for (child_id, child) in &children {
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }
        if matches!(child.inner, ItemEnum::Module(_)) {
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

        if !seen_names.insert(name.to_string()) {
            continue; // already rendered from another source
        }

        // For Use items, follow the target to render the actual definition
        if let ItemEnum::Use(use_item) = &child.inner {
            if let Some(target_id) = &use_item.id
                && let Some(target_item) = source_model.krate.index.get(target_id)
            {
                if matches!(target_item.inner, ItemEnum::Module(_)) {
                    continue;
                }
                if should_render_item(target_item, args) {
                    render_item(
                        source_model,
                        target_item,
                        target_id,
                        "",
                        args,
                        &observer,
                        false,
                        &mut output,
                    );
                }
                collect_impl_ids(target_item, &mut impl_ids);
            }
            continue;
        }

        // Direct definitions (not re-exports)
        if should_render_item(child, args) {
            render_item(
                source_model,
                child,
                child_id,
                "",
                args,
                &observer,
                false,
                &mut output,
            );
        }
        collect_impl_ids(child, &mut impl_ids);
    }

    // Render collected impl blocks
    render_inlined_impl_blocks(source_model, args, &observer, &impl_ids, "", &mut output);

    output
}

/// Render a single named item from source models, with its impl blocks.
///
/// Used for named cross-crate re-export expansion (e.g., `pub use serde_core::Serialize;`).
/// Returns `None` if the item is not found, is a module, or was already rendered (dedup).
pub fn render_single_inlined_item(
    source_models: &[CrateModel],
    item_name: &str,
    filter: &FilterArgs,
    seen_names: &mut HashSet<String>,
) -> Option<String> {
    for model in source_models {
        let Some(root) = model.root_module() else {
            continue;
        };
        let observer = model.crate_name().to_string();

        for (child_id, child) in model.module_children(root) {
            if !matches!(child.visibility, Visibility::Public) {
                continue;
            }
            if matches!(child.inner, ItemEnum::Module(_)) {
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
            if name != item_name {
                continue;
            }
            // Check dedup but don't insert yet — only insert on successful render.
            // A source crate that can't render the item (e.g., proc macro crate)
            // must not block other source crates from trying.
            if seen_names.contains(name) {
                return None;
            }

            let mut output = String::new();
            let mut impl_ids = Vec::new();

            if let ItemEnum::Use(use_item) = &child.inner {
                if let Some(target_id) = &use_item.id
                    && let Some(target_item) = model.krate.index.get(target_id)
                {
                    if matches!(target_item.inner, ItemEnum::Module(_)) {
                        continue; // not renderable here — try next model
                    }
                    if !should_render_item(target_item, filter) {
                        return None;
                    }
                    render_item(
                        model,
                        target_item,
                        target_id,
                        "",
                        filter,
                        &observer,
                        false,
                        &mut output,
                    );
                    collect_impl_ids(target_item, &mut impl_ids);
                }
            } else {
                if !should_render_item(child, filter) {
                    return None;
                }
                render_item(
                    model,
                    child,
                    child_id,
                    "",
                    filter,
                    &observer,
                    false,
                    &mut output,
                );
                collect_impl_ids(child, &mut impl_ids);
            }

            render_inlined_impl_blocks(model, filter, &observer, &impl_ids, "", &mut output);
            if !output.is_empty() {
                seen_names.insert(name.to_string());
                return Some(output);
            }
            // Render produced nothing (e.g., proc macro) — don't claim the name,
            // let other source crates try
        }
    }
    None
}

/// Render a single leaf item (struct, enum, trait, fn, etc.) with its impl blocks.
///
/// Used when the user targets a specific item by path (e.g., `cargo brief api crate outer::PubStruct`).
/// Shows only the matched item + its impls, not sibling items.
#[allow(clippy::too_many_arguments)]
pub fn render_leaf_item(
    model: &CrateModel,
    item: &Item,
    item_id: &Id,
    args: &ApiArgs,
    observer_module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
) -> String {
    let mut output = String::new();
    let crate_name = model.crate_name();

    // Crate header + docs
    output.push_str(&format!("// crate {crate_name}\n"));
    render_crate_docs(model, &args.filter, &mut output);

    // Normalize observer
    let observer = observer_module_path
        .map(|p| {
            if p.contains("::") || p == crate_name {
                p.to_string()
            } else {
                format!("{crate_name}::{p}")
            }
        })
        .unwrap_or_else(|| crate_name.to_string());

    // Visibility check
    if let Some(info) = reachable {
        if !info.reachable.contains(item_id) {
            output.push_str(&format!(
                "// ERROR: item '{}' is not visible from observer position\n",
                item.name.as_deref().unwrap_or("?")
            ));
            return output;
        }
    } else if !matches!(item.visibility, Visibility::Default)
        && !is_visible_from(model, item, item_id, &observer, same_crate)
    {
        output.push_str(&format!(
            "// ERROR: item '{}' is not visible from observer position\n",
            item.name.as_deref().unwrap_or("?")
        ));
        return output;
    }

    // Check filter
    if !should_render_item(item, &args.filter) {
        output.push_str(&format!(
            "// Item '{}' is excluded by filter flags\n",
            item.name.as_deref().unwrap_or("?")
        ));
        return output;
    }

    // Render the item definition
    render_item(
        model,
        item,
        item_id,
        "",
        &args.filter,
        &observer,
        same_crate,
        &mut output,
    );

    // Render impl blocks for types with impls (struct/enum/union)
    let impls = match &item.inner {
        ItemEnum::Struct(s) => Some(&s.impls),
        ItemEnum::Enum(e) => Some(&e.impls),
        ItemEnum::Union(u) => Some(&u.impls),
        _ => None,
    };

    if let Some(impl_ids) = impls {
        let source_type_name = item.name.as_deref().unwrap_or("?");
        let mut simple_trait_impls: Vec<&rustdoc_types::Impl> = Vec::new();

        for impl_id in impl_ids {
            let Some(impl_item) = model.krate.index.get(impl_id) else {
                continue;
            };
            let ItemEnum::Impl(impl_block) = &impl_item.inner else {
                continue;
            };

            if !args.filter.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
                continue;
            }

            let type_name = format_type(&impl_block.for_);
            if type_name.is_empty() {
                continue;
            }

            let generics = format_generics(&impl_block.generics);
            let wc = format_where_clause(&impl_block.generics, "");
            let is_trait_impl = impl_block.trait_.is_some();

            if is_trait_impl {
                if !args.filter.all
                    && !impl_block.is_negative
                    && !has_assoc_items(model, impl_block)
                {
                    simple_trait_impls.push(impl_block);
                    continue;
                }

                let impl_header = if let Some(trait_) = &impl_block.trait_ {
                    let trait_path = format_path(trait_);
                    format!("impl{generics} {trait_path} for {type_name}{wc}")
                } else {
                    format!("impl{generics} {type_name}{wc}")
                };

                let mut assoc_items = Vec::new();
                let mut has_other_items = false;

                for assoc_id in &impl_block.items {
                    if let Some(assoc_item) = model.krate.index.get(assoc_id) {
                        match &assoc_item.inner {
                            ItemEnum::AssocType { .. } | ItemEnum::AssocConst { .. } => {
                                if let Some(r) = render_impl_item(assoc_item, "    ", &args.filter)
                                {
                                    assoc_items.push(r);
                                }
                            }
                            _ => {
                                has_other_items = true;
                            }
                        }
                    }
                }

                render_docs(impl_item, "", &args.filter, &mut output);
                if assoc_items.is_empty() {
                    output.push_str(&format!("{impl_header} {{ .. }}\n"));
                } else {
                    output.push_str(&format!("{impl_header} {{\n"));
                    for item_str in &assoc_items {
                        output.push_str(item_str);
                    }
                    if has_other_items {
                        output.push_str("    // ..\n");
                    }
                    output.push_str("}\n");
                }
            } else {
                let impl_header = format!("impl{generics} {type_name}{wc}");

                if args.filter.compact {
                    render_docs(impl_item, "", &args.filter, &mut output);
                    output.push_str(&format!("{impl_header} {{ .. }}\n"));
                    continue;
                }

                let mut rendered_items = Vec::new();

                for assoc_id in &impl_block.items {
                    if let Some(assoc_item) = model.krate.index.get(assoc_id) {
                        if !matches!(
                            assoc_item.visibility,
                            Visibility::Default | Visibility::Public
                        ) && !is_visible_from(model, assoc_item, assoc_id, &observer, same_crate)
                        {
                            continue;
                        }
                        if let Some(r) = render_impl_item(assoc_item, "    ", &args.filter) {
                            rendered_items.push(r);
                        }
                    }
                }

                if !rendered_items.is_empty() {
                    render_docs(impl_item, "", &args.filter, &mut output);
                    output.push_str(&format!("{impl_header} {{\n"));
                    for item_str in &rendered_items {
                        output.push_str(item_str);
                    }
                    output.push_str("}\n");
                }
            }
        }

        render_trait_impl_summary(source_type_name, &simple_trait_impls, "", &mut output);
    }

    output
}

/// Render an error message when a leaf item is not found in a parent module.
///
/// Lists available non-module items with their kind annotation.
pub fn render_leaf_not_found(
    model: &CrateModel,
    parent_module_path: &str,
    leaf_name: &str,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
) -> String {
    let mut output = String::new();
    let crate_name = model.crate_name();

    output.push_str(&format!("// crate {crate_name}\n"));

    let parent_item = if parent_module_path.is_empty() {
        model.root_module()
    } else {
        model.find_module(parent_module_path)
    };

    let Some(parent_item) = parent_item else {
        output.push_str(&format!(
            "// ERROR: module '{parent_module_path}' not found\n"
        ));
        return output;
    };

    let display_path = if parent_module_path.is_empty() {
        crate_name
    } else {
        parent_module_path
    };

    output.push_str(&format!(
        "// ERROR: item '{leaf_name}' not found in module '{display_path}'\n"
    ));
    output.push_str("// Available items:\n");

    let children = model.module_children(parent_item);
    let mut items: Vec<(&str, &str)> = Vec::new();

    let observer = model.crate_name().to_string();

    for (child_id, child) in &children {
        if matches!(child.inner, ItemEnum::Module(_)) {
            continue;
        }

        // Filter by visibility — don't leak private items to external observers
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Public | Visibility::Default)
            && !is_visible_from(model, child, child_id, &observer, same_crate)
        {
            continue;
        }

        let name = child.name.as_deref().or_else(|| {
            if let ItemEnum::Use(u) = &child.inner {
                Some(u.name.as_str())
            } else {
                None
            }
        });

        let Some(name) = name else { continue };

        let kind = match &child.inner {
            ItemEnum::Struct(_) => "struct",
            ItemEnum::Enum(_) => "enum",
            ItemEnum::Trait(_) => "trait",
            ItemEnum::Function(_) => "fn",
            ItemEnum::TypeAlias(_) => "type",
            ItemEnum::Constant { .. } => "const",
            ItemEnum::Static(_) => "static",
            ItemEnum::Union(_) => "union",
            ItemEnum::Macro(_) => "macro",
            ItemEnum::Use(_) => "use",
            _ => "item",
        };

        items.push((name, kind));
    }

    items.sort_by_key(|(name, _)| *name);
    items.dedup();

    for (name, kind) in &items {
        output.push_str(&format!("//   {name} ({kind})\n"));
    }

    output.push_str(&format!(
        "// TIP: Try `search {leaf_name}` to find items by name across the crate.\n"
    ));

    output
}

/// Collect impl IDs from a type item (struct/enum/union).
fn collect_impl_ids(item: &Item, impl_ids: &mut Vec<Id>) {
    let impls = match &item.inner {
        ItemEnum::Struct(s) => Some(&s.impls),
        ItemEnum::Enum(e) => Some(&e.impls),
        ItemEnum::Union(u) => Some(&u.impls),
        _ => None,
    };
    if let Some(impls) = impls {
        impl_ids.extend(impls.iter().cloned());
    }
}

/// Render impl blocks collected from inlined types.
fn render_inlined_impl_blocks(
    model: &CrateModel,
    args: &FilterArgs,
    observer: &str,
    impl_ids: &[Id],
    source_type_name: &str,
    output: &mut String,
) {
    let mut simple_trait_impls: Vec<&Impl> = Vec::new();

    for impl_id in impl_ids {
        let Some(impl_item) = model.krate.index.get(impl_id) else {
            continue;
        };
        let ItemEnum::Impl(impl_block) = &impl_item.inner else {
            continue;
        };

        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        let type_name = format_type(&impl_block.for_);
        if type_name.is_empty() {
            continue;
        }

        let generics = format_generics(&impl_block.generics);
        let wc = format_where_clause(&impl_block.generics, "");
        let is_trait_impl = impl_block.trait_.is_some();

        if is_trait_impl {
            // Collapse simple trait impls unless --all or negative
            if !args.all && !impl_block.is_negative && !has_assoc_items(model, impl_block) {
                simple_trait_impls.push(impl_block);
                continue;
            }

            // Rich/negative/--all trait impl: render expanded
            let impl_header = if let Some(trait_) = &impl_block.trait_ {
                let trait_path = format_path(trait_);
                format!("impl{generics} {trait_path} for {type_name}{wc}")
            } else {
                format!("impl{generics} {type_name}{wc}")
            };

            let mut assoc_items = Vec::new();
            let mut has_other_items = false;

            for item_id in &impl_block.items {
                if let Some(item) = model.krate.index.get(item_id) {
                    match &item.inner {
                        ItemEnum::AssocType { .. } | ItemEnum::AssocConst { .. } => {
                            if let Some(r) = render_impl_item(item, "    ", args) {
                                assoc_items.push(r);
                            }
                        }
                        _ => {
                            has_other_items = true;
                        }
                    }
                }
            }

            render_docs(impl_item, "", args, output);
            if assoc_items.is_empty() {
                output.push_str(&format!("{impl_header} {{ .. }}\n"));
            } else {
                output.push_str(&format!("{impl_header} {{\n"));
                for item_str in &assoc_items {
                    output.push_str(item_str);
                }
                if has_other_items {
                    output.push_str("    // ..\n");
                }
                output.push_str("}\n");
            }
        } else {
            let impl_header = format!("impl{generics} {type_name}{wc}");

            if args.compact {
                render_docs(impl_item, "", args, output);
                output.push_str(&format!("{impl_header} {{ .. }}\n"));
                continue;
            }

            let mut rendered_items = Vec::new();

            for item_id in &impl_block.items {
                if let Some(item) = model.krate.index.get(item_id) {
                    if !matches!(item.visibility, Visibility::Default | Visibility::Public)
                        && !is_visible_from(model, item, item_id, observer, false)
                    {
                        continue;
                    }
                    if let Some(r) = render_impl_item(item, "    ", args) {
                        rendered_items.push(r);
                    }
                }
            }

            if !rendered_items.is_empty() {
                render_docs(impl_item, "", args, output);
                output.push_str(&format!("{impl_header} {{\n"));
                for item_str in &rendered_items {
                    output.push_str(item_str);
                }
                output.push_str("}\n");
            }
        }
    }

    render_trait_impl_summary(source_type_name, &simple_trait_impls, "", output);
}

/// Check if an item should be rendered based on the args exclusion flags.
fn should_render_item(item: &Item, args: &FilterArgs) -> bool {
    match &item.inner {
        ItemEnum::Struct(_) => !args.no_structs,
        ItemEnum::Enum(_) => !args.no_enums,
        ItemEnum::Trait(_) => !args.no_traits,
        ItemEnum::Function(_) => !args.no_functions,
        ItemEnum::TypeAlias(_) => !args.no_aliases,
        ItemEnum::Constant { .. } => !args.no_constants,
        ItemEnum::Static(_) => !args.no_constants,
        ItemEnum::Union(_) => !args.no_unions,
        ItemEnum::Macro(_) => !args.no_macros,
        _ => true,
    }
}

/// Render children of a module at the given indent level, WITHOUT a `mod` wrapper.
/// Used for inlining glob-reexported private modules into their parent.
#[allow(clippy::too_many_arguments)]
fn render_inline_children(
    model: &CrateModel,
    private_module: &Item,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    max_depth: u32,
    current_depth: u32,
    display_path: &str,
    reachable: Option<&ReachableInfo>,
    output: &mut String,
) {
    // Use a temporary wrapper that renders children at current_depth but
    // without the mod header/footer. We save the output position, render at
    // depth 0 (which suppresses the wrapper), then re-indent if needed.
    //
    // Simpler approach: render at current_depth by temporarily pretending
    // it's a root-level render. We just don't emit the wrapper.
    let indent = "    ".repeat(current_depth.saturating_sub(1) as usize);
    let child_indent = if current_depth > 0 {
        format!("{indent}    ")
    } else {
        String::new()
    };

    let children = model.module_children(private_module);

    for (child_id, child) in &children {
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }

        let name = child.name.as_deref().or_else(|| {
            if let ItemEnum::Use(u) = &child.inner {
                Some(u.name.as_str())
            } else {
                None
            }
        });
        let Some(name) = name else { continue };

        match &child.inner {
            ItemEnum::Module(_) => {
                if reachable.is_some_and(|info| info.glob_private_modules.contains(child_id)) {
                    continue;
                }
                if current_depth < max_depth {
                    let child_path = format!("{display_path}::{name}");
                    render_module_contents(
                        model,
                        child,
                        args,
                        observer,
                        same_crate,
                        max_depth,
                        current_depth + 1,
                        &child_path,
                        reachable,
                        output,
                    );
                } else {
                    render_attrs(
                        child,
                        &child_indent,
                        args.verbose_metadata,
                        args.no_feature_gates,
                        output,
                    );
                    output.push_str(&format!("{child_indent}mod {name} {{ /* ... */ }}\n"));
                }
            }
            ItemEnum::Use(use_item) => {
                if use_item.is_glob {
                    let vis = format_visibility(&child.visibility);
                    output.push_str(&format!("{child_indent}{vis}use {}::*;\n", use_item.source));
                } else if let Some(target_id) = &use_item.id {
                    if let Some(target_item) = model.krate.index.get(target_id) {
                        render_use(child, use_item, target_item, &child_indent, output);
                    } else {
                        let vis = format_visibility(&child.visibility);
                        output.push_str(&format!("{child_indent}{vis}use {};\n", use_item.source));
                    }
                }
            }
            _ => {
                if should_render_item(child, args) {
                    render_item(
                        model,
                        child,
                        child_id,
                        &child_indent,
                        args,
                        observer,
                        same_crate,
                        output,
                    );
                }
            }
        }
    }

    // Render impl blocks for types in the private module
    render_impl_blocks(
        model,
        private_module,
        args,
        observer,
        same_crate,
        current_depth,
        reachable,
        output,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_module_contents(
    model: &CrateModel,
    module_item: &Item,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    max_depth: u32,
    current_depth: u32,
    display_path: &str,
    reachable: Option<&ReachableInfo>,
    output: &mut String,
) {
    // Indent level: root (depth=0) has no wrapper, so children at depth=0 get no indent.
    // Submodules (depth>0) get indent based on depth-1 so top-level modules start at column 0.
    let indent = "    ".repeat(current_depth.saturating_sub(1) as usize);

    if current_depth > 0 {
        render_attrs(
            module_item,
            &indent,
            args.verbose_metadata,
            args.no_feature_gates,
            output,
        );
        output.push_str(&format!("{indent}mod {} {{\n", last_segment(display_path)));
    }

    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Check visibility (skip items that aren't visible from observer)
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
                continue;
            }
        } else if !matches!(child.visibility, Visibility::Default)
            && !is_visible_from(model, child, child_id, observer, same_crate)
        {
            continue;
        }

        // Use items may have name=None at the item level; name is inside inner.use
        let name = child.name.as_deref().or_else(|| {
            if let ItemEnum::Use(u) = &child.inner {
                Some(u.name.as_str())
            } else {
                None
            }
        });
        let Some(name) = name else { continue };

        let child_indent = if current_depth > 0 {
            format!("{indent}    ")
        } else {
            String::new()
        };

        match &child.inner {
            ItemEnum::Module(_) => {
                // Skip glob-private modules — their items are inlined via the glob Use arm
                if reachable.is_some_and(|info| info.glob_private_modules.contains(child_id)) {
                    continue;
                }
                if current_depth < max_depth {
                    let child_path = format!("{display_path}::{name}");
                    render_module_contents(
                        model,
                        child,
                        args,
                        observer,
                        same_crate,
                        max_depth,
                        current_depth + 1,
                        &child_path,
                        reachable,
                        output,
                    );
                } else {
                    render_attrs(
                        child,
                        &child_indent,
                        args.verbose_metadata,
                        args.no_feature_gates,
                        output,
                    );
                    output.push_str(&format!("{child_indent}mod {name} {{ /* ... */ }}\n"));
                }
            }
            ItemEnum::Struct(_) if !args.no_structs => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Enum(_) if !args.no_enums => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Trait(_) if !args.no_traits => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Function(_) if !args.no_functions => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::TypeAlias(_) if !args.no_aliases => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Constant { .. } if !args.no_constants => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Static(_) if !args.no_constants => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Union(_) if !args.no_unions => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Macro(_) if !args.no_macros => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
                    args,
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Use(use_item) => {
                if use_item.is_glob {
                    // Inline private module contents instead of emitting `pub use source::*;`
                    if let Some(info) = reachable
                        && let Some(private_mod_id) = info.glob_inlined.get(child_id)
                        && let Some(private_mod) = model.krate.index.get(private_mod_id)
                    {
                        // Render the private module's children at this level,
                        // WITHOUT a mod wrapper. Use a temporary depth of 0 to
                        // suppress the wrapper, then restore impl block rendering
                        // via render_impl_blocks.
                        render_inline_children(
                            model,
                            private_mod,
                            args,
                            observer,
                            same_crate,
                            max_depth,
                            current_depth,
                            display_path,
                            reachable,
                            output,
                        );
                        continue;
                    }
                    // Fall through: cross-crate globs still emit `pub use source::*;`
                    let vis = format_visibility(&child.visibility);
                    output.push_str(&format!("{child_indent}{vis}use {}::*;\n", use_item.source));
                } else if let Some(target_id) = &use_item.id {
                    if let Some(target_item) = model.krate.index.get(target_id) {
                        render_use(child, use_item, target_item, &child_indent, output);
                    } else {
                        // Target is in an external crate (not in this index).
                        // Render as a simple `pub use source::Name;` line.
                        let vis = format_visibility(&child.visibility);
                        output.push_str(&format!("{child_indent}{vis}use {};\n", use_item.source));
                    }
                }
            }
            _ => {}
        }
    }

    // Render impl blocks for types in this module
    render_impl_blocks(
        model,
        module_item,
        args,
        observer,
        same_crate,
        current_depth,
        reachable,
        output,
    );

    if current_depth > 0 {
        output.push_str(&format!("{indent}}}\n"));
    }
}

/// Check if a trait impl has associated types or constants (making it "rich").
fn has_assoc_items(model: &CrateModel, impl_block: &Impl) -> bool {
    impl_block.items.iter().any(|id| {
        model.krate.index.get(id).is_some_and(|item| {
            matches!(
                item.inner,
                ItemEnum::AssocType { .. } | ItemEnum::AssocConst { .. }
            )
        })
    })
}

/// Render a summary comment line for collapsed simple trait impls.
///
/// Groups by `for_` type: forward impls (matching source type) show just the trait name,
/// reverse impls show `Trait for ForType`.
fn render_trait_impl_summary(
    source_type_name: &str,
    simple_impls: &[&Impl],
    indent: &str,
    output: &mut String,
) {
    if simple_impls.is_empty() {
        return;
    }

    let mut forward_labels: Vec<String> = Vec::new();
    let mut reverse_groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for impl_block in simple_impls {
        let trait_label = format_path(impl_block.trait_.as_ref().unwrap());
        let for_type = format_type(&impl_block.for_);
        // format_type may return "crate::TypeName" for cross-module refs
        let for_type_clean = for_type.strip_prefix("crate::").unwrap_or(&for_type);

        if for_type_clean == source_type_name {
            forward_labels.push(trait_label);
        } else {
            reverse_groups
                .entry(for_type.clone())
                .or_default()
                .push(trait_label);
        }
    }

    forward_labels.sort();
    if !forward_labels.is_empty() {
        output.push_str(&format!(
            "{indent}// {source_type_name}: {}\n",
            forward_labels.join(", ")
        ));
    }

    for (for_type, mut labels) in reverse_groups {
        labels.sort();
        output.push_str(&format!("{indent}// {for_type}: {}\n", labels.join(", ")));
    }
}

#[allow(clippy::too_many_arguments)]
fn render_impl_blocks(
    model: &CrateModel,
    module_item: &Item,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    current_depth: u32,
    reachable: Option<&ReachableInfo>,
    output: &mut String,
) {
    // Match the indent logic from render_module_contents
    let indent = "    ".repeat(current_depth.saturating_sub(1) as usize);
    let child_indent = if current_depth > 0 {
        format!("{indent}    ")
    } else {
        String::new()
    };

    // Process impls per source type (struct/enum/union)
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Only process impls for types visible from the observer
        if let Some(info) = reachable {
            if !info.reachable.contains(child_id) {
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

        let source_type_name = child.name.as_deref().unwrap_or("?");
        let mut simple_trait_impls: Vec<&Impl> = Vec::new();

        for impl_id in impls {
            let Some(impl_item) = model.krate.index.get(impl_id) else {
                continue;
            };
            let ItemEnum::Impl(impl_block) = &impl_item.inner else {
                continue;
            };

            // Skip blanket impls and synthetic impls unless --all
            if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
                continue;
            }

            let type_name = format_type(&impl_block.for_);
            if type_name.is_empty() {
                continue;
            }

            let generics = format_generics(&impl_block.generics);
            let wc = format_where_clause(&impl_block.generics, &child_indent);
            let is_trait_impl = impl_block.trait_.is_some();

            if is_trait_impl {
                // Collapse simple trait impls (no assoc items) unless --all or negative
                if !args.all && !impl_block.is_negative && !has_assoc_items(model, impl_block) {
                    simple_trait_impls.push(impl_block);
                    continue;
                }

                // Rich/negative/--all trait impl: render expanded
                let impl_header = if let Some(trait_) = &impl_block.trait_ {
                    let trait_path = format_path(trait_);
                    format!("{child_indent}impl{generics} {trait_path} for {type_name}{wc}")
                } else {
                    format!("{child_indent}impl{generics} {type_name}{wc}")
                };

                let mut assoc_items = Vec::new();
                let mut has_other_items = false;
                let inner_indent = format!("{child_indent}    ");

                for item_id in &impl_block.items {
                    if let Some(item) = model.krate.index.get(item_id) {
                        match &item.inner {
                            ItemEnum::AssocType { .. } | ItemEnum::AssocConst { .. } => {
                                if let Some(r) = render_impl_item(item, &inner_indent, args) {
                                    assoc_items.push(r);
                                }
                            }
                            _ => {
                                has_other_items = true;
                            }
                        }
                    }
                }

                render_docs(impl_item, &child_indent, args, output);
                if assoc_items.is_empty() {
                    output.push_str(&format!("{impl_header} {{ .. }}\n"));
                } else {
                    output.push_str(&format!("{impl_header} {{\n"));
                    for item_str in &assoc_items {
                        output.push_str(item_str);
                    }
                    if has_other_items {
                        output.push_str(&format!("{inner_indent}// ..\n"));
                    }
                    output.push_str(&format!("{child_indent}}}\n"));
                }
            } else {
                // Inherent impl: render as before
                let impl_header = format!("{child_indent}impl{generics} {type_name}{wc}");

                if args.compact {
                    render_docs(impl_item, &child_indent, args, output);
                    output.push_str(&format!("{impl_header} {{ .. }}\n"));
                    continue;
                }

                let mut rendered_items = Vec::new();
                let inner_indent = format!("{child_indent}    ");

                for item_id in &impl_block.items {
                    if let Some(item) = model.krate.index.get(item_id) {
                        if !matches!(item.visibility, Visibility::Default | Visibility::Public)
                            && !is_visible_from(model, item, item_id, observer, same_crate)
                        {
                            continue;
                        }

                        if let Some(r) = render_impl_item(item, &inner_indent, args) {
                            rendered_items.push(r);
                        }
                    }
                }

                if !rendered_items.is_empty() {
                    render_docs(impl_item, &child_indent, args, output);
                    output.push_str(&format!("{impl_header} {{\n"));
                    for item_str in &rendered_items {
                        output.push_str(item_str);
                    }
                    output.push_str(&format!("{child_indent}}}\n"));
                }
            }
        }

        // Emit summary comment for collapsed simple trait impls
        render_trait_impl_summary(source_type_name, &simple_trait_impls, &child_indent, output);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_item(
    model: &CrateModel,
    item: &Item,
    _item_id: &Id,
    indent: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    output: &mut String,
) {
    render_docs(item, indent, args, output);
    render_attrs(
        item,
        indent,
        args.verbose_metadata,
        args.no_feature_gates,
        output,
    );
    let vis = format_visibility(&item.visibility);

    match &item.inner {
        ItemEnum::Struct(s) => {
            render_struct(
                model, item, s, indent, &vis, args, observer, same_crate, output,
            );
        }
        ItemEnum::Enum(e) => {
            render_enum(model, item, e, indent, &vis, args, output);
        }
        ItemEnum::Trait(t) => {
            render_trait(model, item, t, indent, &vis, args, output);
        }
        ItemEnum::Function(f) => {
            let name = item.name.as_deref().unwrap_or("?");
            let sig = format_function_sig(name, f, &vis);
            let wc = format_where_clause(&f.generics, indent);
            output.push_str(&format!("{indent}{sig}{wc};\n"));
        }
        ItemEnum::TypeAlias(ta) => {
            render_type_alias(item, ta, indent, &vis, output);
        }
        ItemEnum::Constant { type_, const_: c } => {
            render_constant(item, type_, c, indent, &vis, output);
        }
        ItemEnum::Static(s) => {
            render_static(item, s, indent, &vis, output);
        }
        ItemEnum::Union(u) => {
            render_union(model, item, u, indent, &vis, observer, same_crate, output);
        }
        ItemEnum::Macro(_) => {
            let name = item.name.as_deref().unwrap_or("?");
            output.push_str(&format!("{indent}macro_rules! {name} {{ /* ... */ }}\n"));
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn render_struct(
    model: &CrateModel,
    item: &Item,
    s: &Struct,
    indent: &str,
    vis: &str,
    args: &FilterArgs,
    observer: &str,
    same_crate: bool,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&s.generics);
    let wc = format_where_clause(&s.generics, indent);

    match &s.kind {
        StructKind::Unit => {
            output.push_str(&format!("{indent}{vis}struct {name}{generics}{wc};\n"));
        }
        StructKind::Tuple(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f_id| {
                    f_id.as_ref()
                        .and_then(|id| model.krate.index.get(id))
                        .map(|f| {
                            if let ItemEnum::StructField(ty) = &f.inner {
                                let fvis = format_visibility(&f.visibility);
                                format!("{fvis}{}", format_type(ty))
                            } else {
                                "?".to_string()
                            }
                        })
                        .unwrap_or_else(|| "/* private */".to_string())
                })
                .collect();
            output.push_str(&format!(
                "{indent}{vis}struct {name}{generics}({}){wc};\n",
                field_strs.join(", ")
            ));
        }
        StructKind::Plain {
            fields,
            has_stripped_fields,
        } => {
            // Compact: always collapse plain structs to `{ .. }`
            if args.compact {
                output.push_str(&format!(
                    "{indent}{vis}struct {name}{generics}{wc} {{ .. }}\n"
                ));
                return;
            }

            let mut body = String::new();
            let mut hidden_count = 0u32;
            for field_id in fields {
                if let Some(field_item) = model.krate.index.get(field_id) {
                    // Check field visibility
                    if !is_visible_from(model, field_item, field_id, observer, same_crate)
                        && !matches!(field_item.visibility, Visibility::Public)
                    {
                        hidden_count += 1;
                        continue;
                    }
                    if let ItemEnum::StructField(ty) = &field_item.inner {
                        let fname = field_item.name.as_deref().unwrap_or("?");
                        let fvis = format_visibility(&field_item.visibility);
                        render_docs(field_item, &format!("{indent}    "), args, &mut body);
                        body.push_str(&format!(
                            "{indent}    {fvis}{fname}: {},\n",
                            format_type(ty)
                        ));
                    }
                }
            }
            let has_hidden = *has_stripped_fields || hidden_count > 0;
            if body.is_empty() && has_hidden {
                output.push_str(&format!(
                    "{indent}{vis}struct {name}{generics}{wc} {{ .. }}\n"
                ));
            } else if body.is_empty() {
                output.push_str(&format!("{indent}{vis}struct {name}{generics}{wc} {{}}\n"));
            } else {
                if has_hidden {
                    body.push_str(&format!("{indent}    // .. private fields\n"));
                }
                output.push_str(&format!("{indent}{vis}struct {name}{generics}{wc} {{\n"));
                output.push_str(&body);
                output.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

fn render_enum(
    model: &CrateModel,
    item: &Item,
    e: &Enum,
    indent: &str,
    vis: &str,
    args: &FilterArgs,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&e.generics);
    let wc = format_where_clause(&e.generics, indent);

    if args.compact {
        // Compact: render all variants name-only on one line
        let mut variant_parts = Vec::new();
        for variant_id in &e.variants {
            if let Some(variant_item) = model.krate.index.get(variant_id) {
                let vname = variant_item.name.as_deref().unwrap_or("?");
                if let ItemEnum::Variant(variant) = &variant_item.inner {
                    let part = match &variant.kind {
                        VariantKind::Plain => vname.to_string(),
                        VariantKind::Tuple(_) => format!("{vname}(..)"),
                        VariantKind::Struct { .. } => format!("{vname} {{ .. }}"),
                    };
                    variant_parts.push(part);
                }
            }
        }
        let one_liner = format!(
            "{indent}{vis}enum {name}{generics}{wc} {{ {} }}",
            variant_parts.join(", ")
        );
        if one_liner.len() > 120 {
            // Fall back to one-variant-per-line
            output.push_str(&format!("{indent}{vis}enum {name}{generics}{wc} {{\n"));
            for part in &variant_parts {
                output.push_str(&format!("{indent}    {part},\n"));
            }
            output.push_str(&format!("{indent}}}\n"));
        } else {
            output.push_str(&one_liner);
            output.push('\n');
        }
        return;
    }

    output.push_str(&format!("{indent}{vis}enum {name}{generics}{wc} {{\n"));

    for variant_id in &e.variants {
        if let Some(variant_item) = model.krate.index.get(variant_id) {
            render_docs(variant_item, &format!("{indent}    "), args, output);
            let vname = variant_item.name.as_deref().unwrap_or("?");
            if let ItemEnum::Variant(variant) = &variant_item.inner {
                match &variant.kind {
                    VariantKind::Plain => {
                        output.push_str(&format!("{indent}    {vname},\n"));
                    }
                    VariantKind::Tuple(fields) => {
                        let field_strs: Vec<String> = fields
                            .iter()
                            .map(|f_id| {
                                f_id.as_ref()
                                    .and_then(|id| model.krate.index.get(id))
                                    .map(|f| {
                                        if let ItemEnum::StructField(ty) = &f.inner {
                                            format_type(ty)
                                        } else {
                                            "?".to_string()
                                        }
                                    })
                                    .unwrap_or_else(|| "_".to_string())
                            })
                            .collect();
                        output.push_str(&format!(
                            "{indent}    {vname}({}),\n",
                            field_strs.join(", ")
                        ));
                    }
                    VariantKind::Struct {
                        fields,
                        has_stripped_fields,
                    } => {
                        output.push_str(&format!("{indent}    {vname} {{\n"));
                        for field_id in fields {
                            if let Some(field_item) = model.krate.index.get(field_id) {
                                if let ItemEnum::StructField(ty) = &field_item.inner {
                                    let fname = field_item.name.as_deref().unwrap_or("?");
                                    output.push_str(&format!(
                                        "{indent}        {fname}: {},\n",
                                        format_type(ty)
                                    ));
                                }
                            }
                        }
                        if *has_stripped_fields {
                            output.push_str(&format!("{indent}        // ... private fields\n"));
                        }
                        output.push_str(&format!("{indent}    }},\n"));
                    }
                }
            }
        }
    }

    output.push_str(&format!("{indent}}}\n"));
}

fn render_trait(
    model: &CrateModel,
    item: &Item,
    t: &Trait,
    indent: &str,
    vis: &str,
    args: &FilterArgs,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&t.generics);
    let wc = format_where_clause(&t.generics, indent);

    let bounds = if t.bounds.is_empty() {
        String::new()
    } else {
        let bound_strs: Vec<String> = t.bounds.iter().map(format_generic_bound).collect();
        format!(": {}", bound_strs.join(" + "))
    };

    if args.compact {
        output.push_str(&format!(
            "{indent}{vis}trait {name}{generics}{bounds}{wc} {{ .. }}\n"
        ));
        return;
    }

    output.push_str(&format!(
        "{indent}{vis}trait {name}{generics}{bounds}{wc} {{\n"
    ));

    for item_id in &t.items {
        if let Some(trait_item) = model.krate.index.get(item_id) {
            let inner_indent = format!("{indent}    ");
            render_docs(trait_item, &inner_indent, args, output);
            match &trait_item.inner {
                ItemEnum::Function(f) => {
                    let mname = trait_item.name.as_deref().unwrap_or("?");
                    let sig = format_function_sig(mname, f, "");
                    let mwc = format_where_clause(&f.generics, &inner_indent);
                    output.push_str(&format!("{inner_indent}{sig}{mwc};\n"));
                }
                ItemEnum::AssocType {
                    generics: _,
                    bounds,
                    type_,
                } => {
                    let tname = trait_item.name.as_deref().unwrap_or("?");
                    let bounds_str = if bounds.is_empty() {
                        String::new()
                    } else {
                        let b: Vec<String> = bounds.iter().map(format_generic_bound).collect();
                        format!(": {}", b.join(" + "))
                    };
                    if let Some(default) = type_ {
                        output.push_str(&format!(
                            "{inner_indent}type {tname}{bounds_str} = {};\n",
                            format_type(default)
                        ));
                    } else {
                        output.push_str(&format!("{inner_indent}type {tname}{bounds_str};\n"));
                    }
                }
                ItemEnum::AssocConst { type_, value } => {
                    let cname = trait_item.name.as_deref().unwrap_or("?");
                    let val = value
                        .as_deref()
                        .map_or(String::new(), |v| format!(" = {v}"));
                    output.push_str(&format!(
                        "{inner_indent}const {cname}: {}{val};\n",
                        format_type(type_)
                    ));
                }
                _ => {}
            }
        }
    }

    output.push_str(&format!("{indent}}}\n"));
}

fn render_type_alias(item: &Item, ta: &TypeAlias, indent: &str, vis: &str, output: &mut String) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&ta.generics);
    let wc = format_where_clause(&ta.generics, indent);
    output.push_str(&format!(
        "{indent}{vis}type {name}{generics}{wc} = {};\n",
        format_type(&ta.type_)
    ));
}

fn render_constant(
    item: &Item,
    type_: &Type,
    c: &Constant,
    indent: &str,
    vis: &str,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let val = c.value.as_deref().unwrap_or("_");
    output.push_str(&format!(
        "{indent}{vis}const {name}: {} = {val};\n",
        format_type(type_)
    ));
}

fn render_static(item: &Item, s: &Static, indent: &str, vis: &str, output: &mut String) {
    let name = item.name.as_deref().unwrap_or("?");
    let mutability = if s.is_mutable { "mut " } else { "" };
    let val = if s.expr.is_empty() { "_" } else { &s.expr };
    output.push_str(&format!(
        "{indent}{vis}static {mutability}{name}: {} = {val};\n",
        format_type(&s.type_)
    ));
}

fn render_union(
    model: &CrateModel,
    item: &Item,
    u: &Union,
    indent: &str,
    vis: &str,
    observer: &str,
    same_crate: bool,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&u.generics);
    let wc = format_where_clause(&u.generics, indent);
    output.push_str(&format!("{indent}{vis}union {name}{generics}{wc} {{\n"));

    for field_id in &u.fields {
        if let Some(field_item) = model.krate.index.get(field_id) {
            if !is_visible_from(model, field_item, field_id, observer, same_crate)
                && !matches!(field_item.visibility, Visibility::Public)
            {
                continue;
            }
            if let ItemEnum::StructField(ty) = &field_item.inner {
                let fname = field_item.name.as_deref().unwrap_or("?");
                let fvis = format_visibility(&field_item.visibility);
                output.push_str(&format!(
                    "{indent}    {fvis}{fname}: {},\n",
                    format_type(ty)
                ));
            }
        }
    }

    if u.has_stripped_fields {
        output.push_str(&format!("{indent}    // ... private fields\n"));
    }
    output.push_str(&format!("{indent}}}\n"));
}

fn render_use(
    item: &Item,
    use_item: &rustdoc_types::Use,
    target_item: &Item,
    indent: &str,
    output: &mut String,
) {
    let vis = format_visibility(&item.visibility);
    let source = &use_item.source;
    let alias = &use_item.name;
    let kind = item_kind_suffix(target_item);
    if source.ends_with(alias.as_str()) {
        output.push_str(&format!("{indent}{vis}use {source};{kind}\n"));
    } else {
        output.push_str(&format!("{indent}{vis}use {source} as {alias};{kind}\n"));
    }
}

fn item_kind_suffix(item: &Item) -> &'static str {
    match &item.inner {
        ItemEnum::Struct(_) => " // struct",
        ItemEnum::Enum(_) => " // enum",
        ItemEnum::Trait(_) => " // trait",
        ItemEnum::Function(_) => " // fn",
        ItemEnum::TypeAlias(_) => " // type",
        ItemEnum::Constant { .. } => " // const",
        ItemEnum::Static(_) => " // static",
        ItemEnum::Union(_) => " // union",
        ItemEnum::Macro(_) => " // macro",
        _ => "",
    }
}

fn render_impl_item(item: &Item, indent: &str, args: &FilterArgs) -> Option<String> {
    let mut out = String::new();

    match &item.inner {
        ItemEnum::Function(f) => {
            let name = item.name.as_deref().unwrap_or("?");
            let vis = format_visibility(&item.visibility);
            render_docs(item, indent, args, &mut out);
            render_attrs(
                item,
                indent,
                args.verbose_metadata,
                args.no_feature_gates,
                &mut out,
            );
            let sig = format_function_sig(name, f, &vis);
            let wc = format_where_clause(&f.generics, indent);
            out.push_str(&format!("{indent}{sig}{wc};\n"));
            Some(out)
        }
        ItemEnum::AssocType {
            generics: _,
            bounds: _,
            type_,
        } => {
            let name = item.name.as_deref().unwrap_or("?");
            render_attrs(
                item,
                indent,
                args.verbose_metadata,
                args.no_feature_gates,
                &mut out,
            );
            if let Some(ty) = type_ {
                out.push_str(&format!("{indent}type {name} = {};\n", format_type(ty)));
            }
            Some(out)
        }
        ItemEnum::AssocConst { type_, value } => {
            let name = item.name.as_deref().unwrap_or("?");
            render_attrs(
                item,
                indent,
                args.verbose_metadata,
                args.no_feature_gates,
                &mut out,
            );
            let val = value.as_deref().unwrap_or("_");
            out.push_str(&format!(
                "{indent}const {name}: {} = {val};\n",
                format_type(type_)
            ));
            Some(out)
        }
        _ => None,
    }
}

fn render_attrs(
    item: &Item,
    indent: &str,
    verbose: bool,
    no_feature_gates: bool,
    output: &mut String,
) {
    // Deprecation (always rendered)
    if let Some(dep) = &item.deprecation {
        match (&dep.since, &dep.note) {
            (Some(since), Some(note)) => {
                output.push_str(&format!(
                    "{indent}#[deprecated(since = \"{since}\", note = \"{note}\")]\n"
                ));
            }
            (None, Some(note)) => {
                output.push_str(&format!("{indent}#[deprecated = \"{note}\"]\n"));
            }
            (Some(since), None) => {
                output.push_str(&format!("{indent}#[deprecated(since = \"{since}\")]\n"));
            }
            (None, None) => {
                output.push_str(&format!("{indent}#[deprecated]\n"));
            }
        }
    }

    for attr in &item.attrs {
        match attr {
            Attribute::NonExhaustive => {
                output.push_str(&format!("{indent}#[non_exhaustive]\n"));
            }
            Attribute::MustUse { reason } if verbose => {
                if let Some(reason) = reason {
                    output.push_str(&format!("{indent}#[must_use = \"{reason}\"]\n"));
                } else {
                    output.push_str(&format!("{indent}#[must_use]\n"));
                }
            }
            Attribute::Repr(repr) if verbose => {
                let kind = match repr.kind {
                    ReprKind::C => "C",
                    ReprKind::Transparent => "transparent",
                    ReprKind::Simd => "simd",
                    ReprKind::Rust => "Rust",
                };
                let mut parts = vec![kind.to_string()];
                if let Some(align) = repr.align {
                    parts.push(format!("align({align})"));
                }
                if let Some(packed) = repr.packed {
                    if packed == 1 {
                        parts.push("packed".to_string());
                    } else {
                        parts.push(format!("packed({packed})"));
                    }
                }
                if let Some(int) = &repr.int {
                    parts.push(int.clone());
                }
                output.push_str(&format!("{indent}#[repr({})]\n", parts.join(", ")));
            }
            Attribute::NoMangle if verbose => {
                output.push_str(&format!("{indent}#[no_mangle]\n"));
            }
            Attribute::MacroExport if verbose => {
                output.push_str(&format!("{indent}#[macro_export]\n"));
            }
            Attribute::ExportName(name) if verbose => {
                output.push_str(&format!("{indent}#[export_name = \"{name}\"]\n"));
            }
            Attribute::TargetFeature { enable } if verbose => {
                let features = enable.join(", ");
                output.push_str(&format!(
                    "{indent}#[target_feature(enable = \"{features}\")]\n"
                ));
            }
            Attribute::Other(raw) if !no_feature_gates && raw.contains("CfgTrace(") => {
                match parse_cfg_attribute(raw) {
                    Some(pred) => {
                        let attr = reconstruct_cfg_attr(&pred);
                        output.push_str(&format!("{indent}{attr}\n"));
                    }
                    None => output.push_str(&format!("{indent}// cfg: {raw}\n")),
                }
            }
            _ => {} // skip AutomaticallyDerived, Other, and non-verbose attrs
        }
    }
}

fn render_crate_docs(model: &CrateModel, args: &FilterArgs, output: &mut String) {
    if args.no_docs || args.compact || args.no_crate_docs {
        return;
    }
    let Some(root) = model.root_module() else {
        return;
    };
    let Some(docs) = &root.docs else { return };
    let max = args.doc_lines.unwrap_or(usize::MAX);
    if max == 0 {
        return;
    }
    for (i, line) in docs.lines().enumerate() {
        if i >= max {
            break;
        }
        if line.is_empty() {
            output.push_str("//!\n");
        } else {
            output.push_str(&format!("//! {line}\n"));
        }
    }
}

fn render_docs(item: &Item, indent: &str, args: &FilterArgs, output: &mut String) {
    if args.no_docs || args.compact {
        return;
    }
    if let Some(docs) = &item.docs {
        let max = args.doc_lines.unwrap_or(usize::MAX);
        if max == 0 {
            return;
        }
        for (i, line) in docs.lines().enumerate() {
            if i >= max {
                break;
            }
            if line.is_empty() {
                output.push_str(&format!("{indent}///\n"));
            } else {
                output.push_str(&format!("{indent}/// {line}\n"));
            }
        }
    }
}

// === Cross-crate virtual tree rendering ===

use crate::cross_crate::{AccessibleItemKind, CrossCrateIndex};

/// Render a virtual module tree from a `CrossCrateIndex`.
///
/// Groups accessible items by path segments and renders them inside
/// nested `mod { }` wrappers, looking up actual item definitions from
/// `source_models`.
pub fn render_cross_crate_api(
    index: &CrossCrateIndex,
    _facade_crate_name: &str,
    args: &ApiArgs,
) -> String {
    let mut output = String::new();

    // Build a virtual tree from accessible paths
    let mut root = VirtualNode::default();

    for entry in &index.items {
        if entry.item_kind == AccessibleItemKind::Module {
            // Modules create tree structure but don't render as items
            root.ensure_path(&entry.accessible_path);
            continue;
        }
        root.insert_item(&entry.accessible_path, entry.crate_idx, &entry.item_id);
    }

    // Render the tree
    render_virtual_tree(&root, index, args, "", &mut output);

    output
}

/// A node in the virtual module tree.
#[derive(Default)]
struct VirtualNode {
    children: std::collections::BTreeMap<String, VirtualNode>,
    /// Leaf items at this level: (crate_idx, item_id)
    items: Vec<(usize, Id)>,
}

impl VirtualNode {
    /// Ensure all intermediate path segments exist.
    fn ensure_path(&mut self, path: &str) {
        let mut node = self;
        for seg in path.split("::") {
            node = node.children.entry(seg.to_string()).or_default();
        }
    }

    /// Insert a leaf item at the right position in the tree.
    fn insert_item(&mut self, path: &str, crate_idx: usize, item_id: &Id) {
        let segments: Vec<&str> = path.split("::").collect();
        let mut node = self;

        // Navigate to parent
        for &seg in &segments[..segments.len().saturating_sub(1)] {
            node = node.children.entry(seg.to_string()).or_default();
        }

        node.items.push((crate_idx, *item_id));
    }
}

/// Recursively render a virtual module tree.
fn render_virtual_tree(
    node: &VirtualNode,
    index: &CrossCrateIndex,
    args: &ApiArgs,
    indent: &str,
    output: &mut String,
) {
    let child_indent = format!("{indent}    ");

    // Render child modules
    for (name, child_node) in &node.children {
        output.push_str(&format!("\n{indent}mod {name} {{\n"));
        render_virtual_tree(child_node, index, args, &child_indent, output);
        output.push_str(&format!("{indent}}}\n"));
    }

    // Render leaf items at this level
    for &(crate_idx, ref item_id) in &node.items {
        let model = &index.source_models[crate_idx].0;
        let Some(item) = model.krate.index.get(item_id) else {
            continue;
        };

        if !should_render_item(item, &args.filter) {
            continue;
        }

        let observer = model.crate_name().to_string();
        render_item(
            model,
            item,
            item_id,
            indent,
            &args.filter,
            &observer,
            false,
            output,
        );

        // Render impl blocks for types
        let impl_ids: Vec<Id> = match &item.inner {
            ItemEnum::Struct(s) => s.impls.clone(),
            ItemEnum::Enum(e) => e.impls.clone(),
            ItemEnum::Union(u) => u.impls.clone(),
            _ => Vec::new(),
        };

        if !impl_ids.is_empty() {
            render_inlined_impl_blocks(model, &args.filter, &observer, &impl_ids, "", output);
        }
    }
}

// === Type formatting ===

fn format_visibility(vis: &Visibility) -> String {
    match vis {
        Visibility::Public => "pub ".to_string(),
        Visibility::Crate => "pub(crate) ".to_string(),
        Visibility::Restricted { parent: _, path } => format!("pub(in {path}) "),
        Visibility::Default => String::new(),
    }
}

/// Format a `Type` as a string. Public for use by search module.
pub fn format_type_pub(ty: &Type) -> String {
    format_type(ty)
}

fn format_type(ty: &Type) -> String {
    match ty {
        Type::ResolvedPath(path) => format_path(path),
        Type::DynTrait(dyn_trait) => {
            let traits: Vec<String> = dyn_trait
                .traits
                .iter()
                .map(|pt| format_path(&pt.trait_))
                .collect();
            let lifetime = dyn_trait
                .lifetime
                .as_deref()
                .map(|l| format!(" + {l}"))
                .unwrap_or_default();
            format!("dyn {}{lifetime}", traits.join(" + "))
        }
        Type::Generic(name) => name.clone(),
        Type::Primitive(name) => name.clone(),
        Type::FunctionPointer(fp) => {
            let params: Vec<String> = fp.sig.inputs.iter().map(|(_n, t)| format_type(t)).collect();
            let ret = fp
                .sig
                .output
                .as_ref()
                .map(|t| format!(" -> {}", format_type(t)))
                .unwrap_or_default();
            format!("fn({}){ret}", params.join(", "))
        }
        Type::Tuple(types) => {
            let inner: Vec<String> = types.iter().map(format_type).collect();
            format!("({})", inner.join(", "))
        }
        Type::Slice(ty) => format!("[{}]", format_type(ty)),
        Type::Array { type_, len } => format!("[{}; {len}]", format_type(type_)),
        Type::Pat {
            type_,
            __pat_unstable_do_not_use: pat,
        } => {
            format!("{}: {pat}", format_type(type_))
        }
        Type::ImplTrait(bounds) => {
            let bound_strs: Vec<String> = bounds.iter().map(format_generic_bound).collect();
            format!("impl {}", bound_strs.join(" + "))
        }
        Type::Infer => "_".to_string(),
        Type::RawPointer { is_mutable, type_ } => {
            let mutability = if *is_mutable { "mut" } else { "const" };
            format!("*{mutability} {}", format_type(type_))
        }
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let lt = lifetime
                .as_deref()
                .map(|l| format!("{l} "))
                .unwrap_or_default();
            let mutability = if *is_mutable { "mut " } else { "" };
            format!("&{lt}{mutability}{}", format_type(type_))
        }
        Type::QualifiedPath {
            name,
            args: _,
            self_type,
            trait_,
        } => {
            let self_ty = format_type(self_type);
            if let Some(trait_path) = trait_ {
                let tp = format_path(trait_path);
                if tp.is_empty() {
                    // Empty trait path (unresolved by rustdoc) — use shorthand
                    format!("{self_ty}::{name}")
                } else {
                    format!("<{self_ty} as {tp}>::{name}")
                }
            } else {
                format!("{self_ty}::{name}")
            }
        }
    }
}

/// Format a `Path` as a string. Public for use by search module.
pub fn format_path_pub(path: &rustdoc_types::Path) -> String {
    format_path(path)
}

fn format_path(path: &rustdoc_types::Path) -> String {
    // Strip $crate:: macro hygiene artifacts from paths
    let name = path.path.strip_prefix("$crate::").unwrap_or(&path.path);
    if let Some(args) = &path.args {
        let args_str = format_generic_args(args);
        if args_str.is_empty() {
            name.to_string()
        } else {
            format!("{name}{args_str}")
        }
    } else {
        name.to_string()
    }
}

fn format_generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::ReturnTypeNotation => "(..)".to_string(),
        GenericArgs::AngleBracketed { args, constraints } => {
            let mut parts = Vec::new();
            for arg in args {
                match arg {
                    GenericArg::Lifetime(lt) => parts.push(lt.clone()),
                    GenericArg::Type(ty) => parts.push(format_type(ty)),
                    GenericArg::Const(c) => parts.push(c.value.clone().unwrap_or_default()),
                    GenericArg::Infer => parts.push("_".to_string()),
                }
            }
            for c in constraints {
                parts.push(format!("{} = ...", c.name));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("<{}>", parts.join(", "))
            }
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let params: Vec<String> = inputs.iter().map(format_type).collect();
            let ret = output
                .as_ref()
                .map(|t| format!(" -> {}", format_type(t)))
                .unwrap_or_default();
            format!("({}){ret}", params.join(", "))
        }
    }
}

/// Format generics. Public for use by search module.
pub fn format_generics_pub(generics: &rustdoc_types::Generics) -> String {
    format_generics(generics)
}

fn format_generics(generics: &rustdoc_types::Generics) -> String {
    if generics.params.is_empty() {
        return String::new();
    }

    let params: Vec<String> = generics
        .params
        .iter()
        .filter_map(|p| {
            let name = &p.name;
            match &p.kind {
                GenericParamDefKind::Lifetime { outlives } => {
                    if outlives.is_empty() {
                        Some(name.clone())
                    } else {
                        Some(format!("{name}: {}", outlives.join(" + ")))
                    }
                }
                GenericParamDefKind::Type {
                    bounds,
                    default,
                    is_synthetic,
                } => {
                    // Skip synthetic params (desugared `impl Trait`) — they are
                    // re-sugared inline in format_function_sig().
                    if *is_synthetic {
                        return None;
                    }
                    let bounds_str = if bounds.is_empty() {
                        String::new()
                    } else {
                        let b: Vec<String> = bounds.iter().map(format_generic_bound).collect();
                        format!(": {}", b.join(" + "))
                    };
                    let default_str = default
                        .as_ref()
                        .map(|d| format!(" = {}", format_type(d)))
                        .unwrap_or_default();
                    Some(format!("{name}{bounds_str}{default_str}"))
                }
                GenericParamDefKind::Const { type_, default } => {
                    let default_str = default
                        .as_deref()
                        .map(|d| format!(" = {d}"))
                        .unwrap_or_default();
                    Some(format!("const {name}: {}{default_str}", format_type(type_)))
                }
            }
        })
        .collect();

    if params.is_empty() {
        return String::new();
    }

    format!("<{}>", params.join(", "))
}

/// Format a function signature. Public for use by search module.
pub fn format_function_sig_pub(name: &str, f: &Function, vis: &str) -> String {
    format_function_sig(name, f, vis)
}

fn format_function_sig(name: &str, f: &Function, vis: &str) -> String {
    let generics = format_generics(&f.generics);

    // Build a map of synthetic generic param names → their bounds string,
    // so we can re-sugar `impl Trait` in the parameter list.
    let mut synthetic_bounds: std::collections::HashMap<&str, String> = Default::default();
    for p in &f.generics.params {
        if let GenericParamDefKind::Type {
            bounds,
            is_synthetic: true,
            ..
        } = &p.kind
        {
            let b: Vec<String> = bounds.iter().map(format_generic_bound).collect();
            synthetic_bounds.insert(&p.name, b.join(" + "));
        }
    }

    let header = &f.header;
    let is_async = header.is_async;
    let is_unsafe = header.is_unsafe;
    let is_const = header.is_const;

    let mut qualifiers = String::new();
    if is_const {
        qualifiers.push_str("const ");
    }
    if is_async {
        qualifiers.push_str("async ");
    }
    if is_unsafe {
        qualifiers.push_str("unsafe ");
    }

    let params: Vec<String> = f
        .sig
        .inputs
        .iter()
        .map(|(pname, ptype)| {
            // Detect self parameters
            if pname == "self" {
                return match ptype {
                    Type::BorrowedRef { is_mutable, .. } => {
                        if *is_mutable {
                            "&mut self".to_string()
                        } else {
                            "&self".to_string()
                        }
                    }
                    _ => "self".to_string(),
                };
            }
            // Re-sugar synthetic generics as `impl Trait`
            if let Type::Generic(gname) = ptype
                && let Some(bounds) = synthetic_bounds.get(gname.as_str())
            {
                return format!("{pname}: impl {bounds}");
            }
            format!("{pname}: {}", format_type(ptype))
        })
        .collect();

    let ret = f
        .sig
        .output
        .as_ref()
        .map(|t| format!(" -> {}", format_type(t)))
        .unwrap_or_default();

    format!(
        "{vis}{qualifiers}fn {name}{generics}({}){ret}",
        params.join(", ")
    )
}

fn format_generic_bound(bound: &GenericBound) -> String {
    match bound {
        GenericBound::TraitBound {
            trait_,
            generic_params: _,
            modifier,
        } => {
            let prefix = match modifier {
                rustdoc_types::TraitBoundModifier::None => "",
                rustdoc_types::TraitBoundModifier::Maybe => "?",
                rustdoc_types::TraitBoundModifier::MaybeConst => "~const ",
            };
            format!("{prefix}{}", format_path(trait_))
        }
        GenericBound::Outlives(lt) => lt.clone(),
        GenericBound::Use(_) => "use<...>".to_string(),
    }
}

fn format_term(term: &Term) -> String {
    match term {
        Term::Type(ty) => format_type(ty),
        Term::Constant(c) => c.value.clone().unwrap_or_else(|| "..".to_string()),
    }
}

fn format_predicate(pred: &WherePredicate) -> String {
    match pred {
        WherePredicate::BoundPredicate {
            type_,
            bounds,
            generic_params,
        } => {
            let hrtb = if generic_params.is_empty() {
                String::new()
            } else {
                let params: Vec<&str> = generic_params.iter().map(|p| p.name.as_str()).collect();
                format!("for<{}> ", params.join(", "))
            };
            let bound_strs: Vec<String> = bounds.iter().map(format_generic_bound).collect();
            format!("{hrtb}{}: {}", format_type(type_), bound_strs.join(" + "))
        }
        WherePredicate::LifetimePredicate { lifetime, outlives } => {
            if outlives.is_empty() {
                lifetime.clone()
            } else {
                format!("{lifetime}: {}", outlives.join(" + "))
            }
        }
        WherePredicate::EqPredicate { lhs, rhs } => {
            format!("{} = {}", format_type(lhs), format_term(rhs))
        }
    }
}

/// Format a where clause in multi-line (normal render) style.
/// Returns "" if no predicates, " where P" for single, or "\n{indent}where\n..." for multiple.
fn format_where_clause(generics: &rustdoc_types::Generics, indent: &str) -> String {
    let preds = &generics.where_predicates;
    if preds.is_empty() {
        return String::new();
    }
    let strs: Vec<String> = preds.iter().map(format_predicate).collect();
    if strs.len() == 1 {
        format!(" where {}", strs[0])
    } else {
        let mut out = format!("\n{indent}where\n");
        for (i, s) in strs.iter().enumerate() {
            let comma = if i + 1 < strs.len() { "," } else { "" };
            out.push_str(&format!("{indent}    {s}{comma}\n"));
        }
        // Remove trailing newline — callers append their own terminators
        out.truncate(out.trim_end_matches('\n').len());
        out
    }
}

/// Public wrapper for `format_where_clause` (used by search.rs).
pub fn format_where_clause_pub(generics: &rustdoc_types::Generics, indent: &str) -> String {
    format_where_clause(generics, indent)
}

/// Format a compact inline where clause: " where P1, P2".
fn format_where_clause_compact(generics: &rustdoc_types::Generics) -> String {
    let preds = &generics.where_predicates;
    if preds.is_empty() {
        return String::new();
    }
    let strs: Vec<String> = preds.iter().map(format_predicate).collect();
    format!(" where {}", strs.join(", "))
}

/// Public wrapper for `format_where_clause_compact` (used by search.rs).
pub fn format_where_clause_compact_pub(generics: &rustdoc_types::Generics) -> String {
    format_where_clause_compact(generics)
}

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}
