use std::collections::BTreeMap;

use rustdoc_types::{Id, Item, ItemEnum, Visibility};

use crate::model::{CrateModel, ReachableInfo, is_visible_from};

/// Kind of item for summary counting.
/// Ord derives display order: traits first, unions last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKind {
    Trait,
    Struct,
    Enum,
    Function,
    TypeAlias,
    Constant,
    Macro,
    Union,
}

impl ItemKind {
    fn plural(self) -> &'static str {
        match self {
            ItemKind::Trait => "traits",
            ItemKind::Struct => "structs",
            ItemKind::Enum => "enums",
            ItemKind::Function => "fns",
            ItemKind::TypeAlias => "types",
            ItemKind::Constant => "consts",
            ItemKind::Macro => "macros",
            ItemKind::Union => "unions",
        }
    }
}

/// Per-module item counts.
#[derive(Default)]
struct ModuleSummary {
    counts: BTreeMap<ItemKind, usize>,
}

impl ModuleSummary {
    fn increment(&mut self, kind: ItemKind) {
        *self.counts.entry(kind).or_insert(0) += 1;
    }

    fn is_empty(&self) -> bool {
        self.counts.values().all(|&c| c == 0)
    }

    fn format_counts(&self) -> String {
        self.counts
            .iter()
            .filter(|&(_, c)| *c > 0)
            .map(|(kind, count)| format!("{count} {}", kind.plural()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Classify an item into an ItemKind, following non-glob `Use` to its target.
fn classify_item(item: &Item, model: &CrateModel) -> Option<ItemKind> {
    match &item.inner {
        ItemEnum::Struct(_) => Some(ItemKind::Struct),
        ItemEnum::Enum(_) => Some(ItemKind::Enum),
        ItemEnum::Function(_) => Some(ItemKind::Function),
        ItemEnum::Trait(_) => Some(ItemKind::Trait),
        ItemEnum::TypeAlias(_) => Some(ItemKind::TypeAlias),
        ItemEnum::Constant { .. } | ItemEnum::Static(_) => Some(ItemKind::Constant),
        ItemEnum::Macro(_) => Some(ItemKind::Macro),
        ItemEnum::Union(_) => Some(ItemKind::Union),
        ItemEnum::Use(use_item) => {
            if use_item.is_glob {
                return None; // glob uses are not counted
            }
            // Follow the re-export to classify the target
            use_item
                .id
                .as_ref()
                .and_then(|target_id| model.krate.index.get(target_id))
                .and_then(|target| classify_item(target, model))
        }
        ItemEnum::Module(_) | ItemEnum::Impl(_) => None,
        _ => None,
    }
}

/// Check if an item passes visibility filtering.
fn is_item_visible(
    item: &Item,
    item_id: &Id,
    observer: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    model: &CrateModel,
) -> bool {
    if let Some(info) = reachable {
        info.reachable.contains(item_id)
    } else if same_crate {
        let obs = observer.unwrap_or("");
        is_visible_from(model, item, item_id, obs, true)
    } else {
        matches!(item.visibility, Visibility::Public)
    }
}

/// Recursively walk the module tree, collecting item counts per module.
#[allow(clippy::too_many_arguments)]
fn count_module_items(
    model: &CrateModel,
    module_item: &Item,
    current_path: &str,
    root_path: &str,
    observer: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
    root_summary: &mut ModuleSummary,
    module_summaries: &mut BTreeMap<String, ModuleSummary>,
) {
    for (child_id, child) in model.module_children(module_item) {
        if !is_item_visible(child, child_id, observer, same_crate, reachable, model) {
            continue;
        }

        if let ItemEnum::Module(_) = &child.inner {
            let is_glob_private =
                reachable.is_some_and(|info| info.glob_private_modules.contains(child_id));

            if is_glob_private {
                // Recurse but count items at PARENT level (same current_path)
                count_module_items(
                    model,
                    child,
                    current_path,
                    root_path,
                    observer,
                    same_crate,
                    reachable,
                    root_summary,
                    module_summaries,
                );
            } else {
                // For modules, check actual visibility — being in the reachable set
                // alone isn't enough (e.g., pub(crate) modules whose items are glob-reexported
                // are reachable but shouldn't appear as named module lines in external view).
                let module_visible = if !same_crate {
                    matches!(child.visibility, Visibility::Public)
                } else {
                    true // already passed is_item_visible above
                };
                if !module_visible {
                    continue;
                }

                let child_name = match &child.name {
                    Some(n) => n.as_str(),
                    None => continue,
                };
                let child_path = if current_path == root_path {
                    child_name.to_string()
                } else {
                    // Strip root_path prefix to get relative module path
                    let rel = current_path
                        .strip_prefix(root_path)
                        .and_then(|s| s.strip_prefix("::"))
                        .unwrap_or(current_path);
                    format!("{rel}::{child_name}")
                };

                // Ensure entry exists
                module_summaries.entry(child_path.clone()).or_default();

                // Build full path for recursive walk
                let full_child_path = format!("{current_path}::{child_name}");
                count_module_items(
                    model,
                    child,
                    &full_child_path,
                    root_path,
                    observer,
                    same_crate,
                    reachable,
                    root_summary,
                    module_summaries,
                );
            }
            continue;
        }

        if let Some(kind) = classify_item(child, model) {
            if current_path == root_path {
                root_summary.increment(kind);
            } else {
                let rel = current_path
                    .strip_prefix(root_path)
                    .and_then(|s| s.strip_prefix("::"))
                    .unwrap_or(current_path);
                module_summaries
                    .entry(rel.to_string())
                    .or_default()
                    .increment(kind);
            }
        }
    }
}

/// Render a compact summary of visible modules and their item counts.
///
/// If `module_path` is Some, scopes to that module. Otherwise uses the crate root.
pub fn render_summary(
    model: &CrateModel,
    module_path: Option<&str>,
    same_crate: bool,
    reachable: Option<&ReachableInfo>,
) -> String {
    let root_item = if let Some(path) = module_path {
        match model.find_module(path) {
            Some(item) => item,
            None => return format!("// module '{path}' not found\n"),
        }
    } else {
        match model.root_module() {
            Some(item) => item,
            None => return String::new(),
        }
    };

    let root_path = if let Some(path) = module_path {
        format!("{}::{path}", model.crate_name())
    } else {
        model.crate_name().to_string()
    };

    let observer = if same_crate {
        Some(root_path.as_str())
    } else {
        None
    };

    let mut root_summary = ModuleSummary::default();
    let mut module_summaries = BTreeMap::new();

    count_module_items(
        model,
        root_item,
        &root_path,
        &root_path,
        observer,
        same_crate,
        reachable,
        &mut root_summary,
        &mut module_summaries,
    );

    // Build output
    let mut output = String::new();

    // Header
    let display_name = if let Some(path) = module_path {
        format!("{}::{path}", model.crate_name())
    } else {
        model.crate_name().to_string()
    };
    output.push_str(&format!("// crate {display_name}\n"));

    // Collect non-empty module lines for column alignment
    let mut mod_lines: Vec<(String, String)> = Vec::new();
    for (path, summary) in &module_summaries {
        if summary.is_empty() {
            continue;
        }
        let decl = format!("mod {path};");
        let comment = summary.format_counts();
        mod_lines.push((decl, comment));
    }

    if !mod_lines.is_empty() {
        // Find max declaration width for column alignment
        let max_decl_width = mod_lines.iter().map(|(d, _)| d.len()).max().unwrap_or(0);

        for (decl, comment) in &mod_lines {
            let padding = max_decl_width - decl.len() + 1;
            output.push_str(decl);
            output.push_str(&" ".repeat(padding));
            output.push_str(&format!("// {comment}\n"));
        }
    }

    // Root items
    if !root_summary.is_empty() {
        let counts = root_summary.format_counts();
        output.push_str(&format!("// root: {counts}\n"));
    }

    output
}

/// Merge a sub-crate summary into the main output, prefixing module paths.
pub fn merge_sub_crate_summary(main_output: &mut String, sub_output: &str, display_name: &str) {
    for line in sub_output.lines() {
        if line.starts_with("// crate ") {
            // Skip the sub-crate header
            continue;
        }
        if let Some(rest) = line.strip_prefix("mod ") {
            main_output.push_str(&format!("mod {display_name}::{rest}\n"));
        } else if let Some(rest) = line.strip_prefix("// root: ") {
            main_output.push_str(&format!("mod {display_name};  // {rest}\n"));
        } else {
            main_output.push_str(line);
            main_output.push('\n');
        }
    }
}

use crate::cross_crate::{AccessibleItemKind, CrossCrateIndex};

/// Summarize a `CrossCrateIndex` by counting items per top-level accessible path segment.
///
/// Returns summary lines in the same format as `render_summary`, ready to
/// be appended to the main crate's summary output.
pub fn summarize_cross_crate_index(index: &CrossCrateIndex) -> String {
    let mut module_summaries: BTreeMap<String, ModuleSummary> = BTreeMap::new();
    let mut root_summary = ModuleSummary::default();

    for entry in &index.items {
        if entry.item_kind == AccessibleItemKind::Module {
            // Ensure the module path entry exists
            module_summaries
                .entry(entry.accessible_path.clone())
                .or_default();
            continue;
        }

        let kind = match entry.item_kind {
            AccessibleItemKind::Struct => ItemKind::Struct,
            AccessibleItemKind::Enum => ItemKind::Enum,
            AccessibleItemKind::Function => ItemKind::Function,
            AccessibleItemKind::Trait => ItemKind::Trait,
            AccessibleItemKind::TypeAlias => ItemKind::TypeAlias,
            AccessibleItemKind::Constant | AccessibleItemKind::Static => ItemKind::Constant,
            AccessibleItemKind::Macro => ItemKind::Macro,
            AccessibleItemKind::Union => ItemKind::Union,
            AccessibleItemKind::Module => continue,
        };

        // Determine which module this item belongs to
        if let Some(last_sep) = entry.accessible_path.rfind("::") {
            let module_path = &entry.accessible_path[..last_sep];
            module_summaries
                .entry(module_path.to_string())
                .or_default()
                .increment(kind);
        } else {
            // Root-level item
            root_summary.increment(kind);
        }
    }

    let mut output = String::new();

    // Collect non-empty module lines for column alignment
    let mut mod_lines: Vec<(String, String)> = Vec::new();
    for (path, summary) in &module_summaries {
        if summary.is_empty() {
            continue;
        }
        let decl = format!("mod {path};");
        let comment = summary.format_counts();
        mod_lines.push((decl, comment));
    }

    if !mod_lines.is_empty() {
        let max_decl_width = mod_lines.iter().map(|(d, _)| d.len()).max().unwrap_or(0);

        for (decl, comment) in &mod_lines {
            let padding = max_decl_width - decl.len() + 1;
            output.push_str(decl);
            output.push_str(&" ".repeat(padding));
            output.push_str(&format!("// {comment}\n"));
        }
    }

    // Root items (rare for cross-crate, but possible from glob flattening)
    if !root_summary.is_empty() {
        let counts = root_summary.format_counts();
        output.push_str(&format!("// root: {counts}\n"));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_kind_ordering() {
        assert!(ItemKind::Trait < ItemKind::Struct);
        assert!(ItemKind::Struct < ItemKind::Enum);
        assert!(ItemKind::Enum < ItemKind::Function);
        assert!(ItemKind::Function < ItemKind::TypeAlias);
        assert!(ItemKind::TypeAlias < ItemKind::Constant);
        assert!(ItemKind::Constant < ItemKind::Macro);
        assert!(ItemKind::Macro < ItemKind::Union);
    }

    #[test]
    fn test_module_summary_format() {
        let mut summary = ModuleSummary::default();
        summary.increment(ItemKind::Trait);
        summary.increment(ItemKind::Trait);
        summary.increment(ItemKind::Struct);
        summary.increment(ItemKind::Function);
        summary.increment(ItemKind::Function);
        summary.increment(ItemKind::Function);

        assert_eq!(summary.format_counts(), "2 traits, 1 structs, 3 fns");
        assert!(!summary.is_empty());
    }

    #[test]
    fn test_empty_summary() {
        let summary = ModuleSummary::default();
        assert!(summary.is_empty());
        assert_eq!(summary.format_counts(), "");
    }
}
