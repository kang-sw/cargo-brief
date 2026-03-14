use rustdoc_types::{
    Constant, Enum, Function, GenericArg, GenericArgs, GenericBound, GenericParamDefKind, Item,
    ItemEnum, Static, Struct, StructKind, Trait, Type, TypeAlias, Union, VariantKind, Visibility,
};

use crate::cli::BriefArgs;
use crate::model::{CrateModel, is_visible_from};

/// Render the API of a target module as pseudo-Rust.
///
/// Returns an error if the target module path is specified but not found.
pub fn render_module_api(
    model: &CrateModel,
    target_module_path: Option<&str>,
    args: &BriefArgs,
    observer_module_path: Option<&str>,
    same_crate: bool,
) -> String {
    let mut output = String::new();

    let crate_name = model.crate_name();
    output.push_str(&format!("// crate {crate_name}\n"));

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
        args,
        &observer,
        same_crate,
        depth,
        0,
        mod_display_path,
        &mut output,
    );

    output
}

#[allow(clippy::too_many_arguments)]
fn render_module_contents(
    model: &CrateModel,
    module_item: &Item,
    args: &BriefArgs,
    observer: &str,
    same_crate: bool,
    max_depth: u32,
    current_depth: u32,
    display_path: &str,
    output: &mut String,
) {
    // Indent level: root (depth=0) has no wrapper, so children at depth=0 get no indent.
    // Submodules (depth>0) get indent based on depth-1 so top-level modules start at column 0.
    let indent = "    ".repeat(current_depth.saturating_sub(1) as usize);

    if current_depth > 0 {
        output.push_str(&format!("{indent}mod {} {{\n", last_segment(display_path)));
    }

    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        // Check visibility (skip items that aren't visible from observer)
        if !matches!(child.visibility, Visibility::Default) {
            if !is_visible_from(model, child, child_id, observer, same_crate) {
                continue;
            }
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
                        output,
                    );
                } else {
                    output.push_str(&format!("{child_indent}mod {name} {{ /* ... */ }}\n"));
                }
            }
            ItemEnum::Struct(_) if !args.no_structs => {
                render_item(
                    model,
                    child,
                    child_id,
                    &child_indent,
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
                    observer,
                    same_crate,
                    output,
                );
            }
            ItemEnum::Use(use_item) => {
                if use_item.is_glob {
                    // Glob re-exports: render as `pub use source::*;`
                    // (run_pipeline may replace this with expanded individual items)
                    let vis = format_visibility(&child.visibility);
                    output.push_str(&format!("{child_indent}{vis}use {}::*;\n", use_item.source));
                } else if let Some(target_id) = &use_item.id {
                    if let Some(target_item) = model.krate.index.get(target_id) {
                        render_use(child, use_item, target_item, &child_indent, output);
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
        output,
    );

    if current_depth > 0 {
        output.push_str(&format!("{indent}}}\n"));
    }
}

fn render_impl_blocks(
    model: &CrateModel,
    module_item: &Item,
    args: &BriefArgs,
    observer: &str,
    same_crate: bool,
    current_depth: u32,
    output: &mut String,
) {
    // Match the indent logic from render_module_contents
    let indent = "    ".repeat(current_depth.saturating_sub(1) as usize);
    let child_indent = if current_depth > 0 {
        format!("{indent}    ")
    } else {
        String::new()
    };

    // Collect impl IDs from visible structs/enums/unions in this module
    let children = model.module_children(module_item);
    let mut impl_ids: Vec<Id> = Vec::new();

    for (child_id, child) in &children {
        // Only collect impls for types visible from the observer
        if !matches!(child.visibility, Visibility::Default) {
            if !is_visible_from(model, child, child_id, observer, same_crate) {
                continue;
            }
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

        // Skip blanket impls and synthetic impls unless --all
        if !args.all && (impl_block.is_synthetic || impl_block.blanket_impl.is_some()) {
            continue;
        }

        let type_name = format_type(&impl_block.for_);
        if type_name.is_empty() {
            continue;
        }

        let generics = format_generics(&impl_block.generics);
        let is_trait_impl = impl_block.trait_.is_some();
        let impl_header = if let Some(trait_) = &impl_block.trait_ {
            let trait_path = format_path(trait_);
            format!("{child_indent}impl{generics} {trait_path} for {type_name}")
        } else {
            format!("{child_indent}impl{generics} {type_name}")
        };

        if is_trait_impl {
            // Trait impls: collect only associated types/constants (omit methods)
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

            render_docs(impl_item, &child_indent, output);
            if assoc_items.is_empty() {
                // No associated types/constants → one-liner
                output.push_str(&format!("{impl_header} {{ .. }}\n"));
            } else {
                // Has associated types/constants → show those, plus .. if methods omitted
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
            // Inherent impls: show all visible items (methods, types, constants)
            let mut rendered_items = Vec::new();
            let inner_indent = format!("{child_indent}    ");

            for item_id in &impl_block.items {
                if let Some(item) = model.krate.index.get(item_id) {
                    if !matches!(item.visibility, Visibility::Default | Visibility::Public) {
                        if !is_visible_from(model, item, item_id, observer, same_crate) {
                            continue;
                        }
                    }

                    if let Some(r) = render_impl_item(item, &inner_indent, args) {
                        rendered_items.push(r);
                    }
                }
            }

            if !rendered_items.is_empty() {
                render_docs(impl_item, &child_indent, output);
                output.push_str(&format!("{impl_header} {{\n"));
                for item_str in &rendered_items {
                    output.push_str(item_str);
                }
                output.push_str(&format!("{child_indent}}}\n"));
            }
        }
    }
}

fn render_item(
    model: &CrateModel,
    item: &Item,
    _item_id: &Id,
    indent: &str,
    observer: &str,
    same_crate: bool,
    output: &mut String,
) {
    render_docs(item, indent, output);
    let vis = format_visibility(&item.visibility);

    match &item.inner {
        ItemEnum::Struct(s) => {
            render_struct(model, item, s, indent, &vis, observer, same_crate, output);
        }
        ItemEnum::Enum(e) => {
            render_enum(model, item, e, indent, &vis, output);
        }
        ItemEnum::Trait(t) => {
            render_trait(model, item, t, indent, &vis, output);
        }
        ItemEnum::Function(f) => {
            let name = item.name.as_deref().unwrap_or("?");
            let sig = format_function_sig(name, f, &vis);
            output.push_str(&format!("{indent}{sig};\n"));
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

fn render_struct(
    model: &CrateModel,
    item: &Item,
    s: &Struct,
    indent: &str,
    vis: &str,
    observer: &str,
    same_crate: bool,
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&s.generics);

    match &s.kind {
        StructKind::Unit => {
            output.push_str(&format!("{indent}{vis}struct {name}{generics};\n"));
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
                "{indent}{vis}struct {name}{generics}({});\n",
                field_strs.join(", ")
            ));
        }
        StructKind::Plain {
            fields,
            has_stripped_fields,
        } => {
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
                        render_docs(field_item, &format!("{indent}    "), &mut body);
                        body.push_str(&format!(
                            "{indent}    {fvis}{fname}: {},\n",
                            format_type(ty)
                        ));
                    }
                }
            }
            let has_hidden = *has_stripped_fields || hidden_count > 0;
            if body.is_empty() && has_hidden {
                output.push_str(&format!("{indent}{vis}struct {name}{generics} {{ .. }}\n"));
            } else if body.is_empty() {
                output.push_str(&format!("{indent}{vis}struct {name}{generics} {{}}\n"));
            } else {
                if has_hidden {
                    body.push_str(&format!("{indent}    // .. private fields\n"));
                }
                output.push_str(&format!("{indent}{vis}struct {name}{generics} {{\n"));
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
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&e.generics);
    output.push_str(&format!("{indent}{vis}enum {name}{generics} {{\n"));

    for variant_id in &e.variants {
        if let Some(variant_item) = model.krate.index.get(variant_id) {
            render_docs(variant_item, &format!("{indent}    "), output);
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
    output: &mut String,
) {
    let name = item.name.as_deref().unwrap_or("?");
    let generics = format_generics(&t.generics);

    let bounds = if t.bounds.is_empty() {
        String::new()
    } else {
        let bound_strs: Vec<String> = t.bounds.iter().map(format_generic_bound).collect();
        format!(": {}", bound_strs.join(" + "))
    };

    output.push_str(&format!("{indent}{vis}trait {name}{generics}{bounds} {{\n"));

    for item_id in &t.items {
        if let Some(trait_item) = model.krate.index.get(item_id) {
            let inner_indent = format!("{indent}    ");
            render_docs(trait_item, &inner_indent, output);
            match &trait_item.inner {
                ItemEnum::Function(f) => {
                    let mname = trait_item.name.as_deref().unwrap_or("?");
                    let sig = format_function_sig(mname, f, "");
                    output.push_str(&format!("{inner_indent}{sig};\n"));
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
    output.push_str(&format!(
        "{indent}{vis}type {name}{generics} = {};\n",
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
    output.push_str(&format!("{indent}{vis}union {name}{generics} {{\n"));

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
    _target_item: &Item,
    indent: &str,
    output: &mut String,
) {
    let vis = format_visibility(&item.visibility);
    let source = &use_item.source;
    let alias = &use_item.name;
    if source.ends_with(alias.as_str()) {
        output.push_str(&format!("{indent}{vis}use {source};\n"));
    } else {
        output.push_str(&format!("{indent}{vis}use {source} as {alias};\n"));
    }
}

fn render_impl_item(item: &Item, indent: &str, _args: &BriefArgs) -> Option<String> {
    let mut out = String::new();

    match &item.inner {
        ItemEnum::Function(f) => {
            let name = item.name.as_deref().unwrap_or("?");
            let vis = format_visibility(&item.visibility);
            render_docs(item, indent, &mut out);
            let sig = format_function_sig(name, f, &vis);
            out.push_str(&format!("{indent}{sig};\n"));
            Some(out)
        }
        ItemEnum::AssocType {
            generics: _,
            bounds: _,
            type_,
        } => {
            let name = item.name.as_deref().unwrap_or("?");
            if let Some(ty) = type_ {
                out.push_str(&format!("{indent}type {name} = {};\n", format_type(ty)));
            }
            Some(out)
        }
        ItemEnum::AssocConst { type_, value } => {
            let name = item.name.as_deref().unwrap_or("?");
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

fn render_docs(item: &Item, indent: &str, output: &mut String) {
    if let Some(docs) = &item.docs {
        for line in docs.lines() {
            if line.is_empty() {
                output.push_str(&format!("{indent}///\n"));
            } else {
                output.push_str(&format!("{indent}/// {line}\n"));
            }
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
                format!("<{self_ty} as {}>::{name}", format_path(trait_path))
            } else {
                format!("{self_ty}::{name}")
            }
        }
    }
}

fn format_path(path: &rustdoc_types::Path) -> String {
    let name = &path.path;
    if let Some(args) = &path.args {
        let args_str = format_generic_args(args);
        if args_str.is_empty() {
            name.clone()
        } else {
            format!("{name}{args_str}")
        }
    } else {
        name.clone()
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

fn format_generics(generics: &rustdoc_types::Generics) -> String {
    if generics.params.is_empty() {
        return String::new();
    }

    let params: Vec<String> = generics
        .params
        .iter()
        .map(|p| {
            let name = &p.name;
            match &p.kind {
                GenericParamDefKind::Lifetime { outlives } => {
                    if outlives.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}: {}", outlives.join(" + "))
                    }
                }
                GenericParamDefKind::Type {
                    bounds,
                    default,
                    is_synthetic: _,
                } => {
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
                    format!("{name}{bounds_str}{default_str}")
                }
                GenericParamDefKind::Const { type_, default } => {
                    let default_str = default
                        .as_deref()
                        .map(|d| format!(" = {d}"))
                        .unwrap_or_default();
                    format!("const {name}: {}{default_str}", format_type(type_))
                }
            }
        })
        .collect();

    format!("<{}>", params.join(", "))
}

fn format_function_sig(name: &str, f: &Function, vis: &str) -> String {
    let generics = format_generics(&f.generics);

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
            let ty = format_type(ptype);
            // Detect self parameters
            if pname == "self" {
                match ptype {
                    Type::BorrowedRef { is_mutable, .. } => {
                        if *is_mutable {
                            "&mut self".to_string()
                        } else {
                            "&self".to_string()
                        }
                    }
                    _ => "self".to_string(),
                }
            } else {
                format!("{pname}: {ty}")
            }
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

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

use rustdoc_types::Id;
