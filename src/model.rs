use std::collections::{HashMap, HashSet};

use rustdoc_types::{Crate, Id, Item, ItemEnum, Visibility};

/// A processed view of a crate's items, organized by module hierarchy.
pub struct CrateModel {
    pub krate: Crate,
    /// Maps module paths (e.g., "outer::inner") to their item IDs.
    pub module_index: HashMap<String, Id>,
    /// Maps item IDs to their containing module path.
    pub item_module_path: HashMap<Id, String>,
}

impl CrateModel {
    /// Build a CrateModel from a parsed rustdoc JSON Crate.
    pub fn from_crate(krate: Crate) -> Self {
        let mut module_index = HashMap::new();
        let mut item_module_path = HashMap::new();

        // Walk the module tree starting from the crate root
        let crate_name = krate
            .index
            .get(&krate.root)
            .and_then(|item| item.name.as_deref())
            .unwrap_or("unknown")
            .to_string();

        if let Some(root_item) = krate.index.get(&krate.root) {
            Self::walk_modules(
                &krate,
                root_item,
                &krate.root,
                &crate_name,
                &mut module_index,
                &mut item_module_path,
            );
        }

        Self {
            krate,
            module_index,
            item_module_path,
        }
    }

    fn walk_modules(
        krate: &Crate,
        item: &Item,
        item_id: &Id,
        current_path: &str,
        module_index: &mut HashMap<String, Id>,
        item_module_path: &mut HashMap<Id, String>,
    ) {
        module_index.insert(current_path.to_string(), item_id.clone());
        item_module_path.insert(item_id.clone(), current_path.to_string());

        if let ItemEnum::Module(module) = &item.inner {
            for child_id in &module.items {
                if let Some(child_item) = krate.index.get(child_id) {
                    let child_path = if let Some(name) = &child_item.name {
                        format!("{current_path}::{name}")
                    } else {
                        continue;
                    };

                    item_module_path.insert(child_id.clone(), current_path.to_string());

                    if matches!(child_item.inner, ItemEnum::Module(_)) {
                        Self::walk_modules(
                            krate,
                            child_item,
                            child_id,
                            &child_path,
                            module_index,
                            item_module_path,
                        );
                    }
                }
            }
        }
    }

    /// Get the crate name.
    pub fn crate_name(&self) -> &str {
        self.krate
            .index
            .get(&self.krate.root)
            .and_then(|item| item.name.as_deref())
            .unwrap_or("unknown")
    }

    /// Find a module by its path relative to the crate root.
    /// Accepts paths like "outer::inner" (without crate name prefix).
    pub fn find_module(&self, module_path: &str) -> Option<&Item> {
        self.find_module_entry(module_path).map(|(_id, item)| item)
    }

    /// Find a module by path, returning both its Id and Item.
    fn find_module_entry(&self, module_path: &str) -> Option<(&Id, &Item)> {
        let full_path = format!("{}::{}", self.crate_name(), module_path);
        let id = self
            .module_index
            .get(&full_path)
            .or_else(|| self.module_index.get(module_path))?;
        self.krate.index.get(id).map(|item| (id, item))
    }

    /// Find the root module of the crate.
    pub fn root_module(&self) -> Option<&Item> {
        self.krate.index.get(&self.krate.root)
    }

    /// Get children items of a module.
    pub fn module_children<'a>(&'a self, module_item: &'a Item) -> Vec<(&'a Id, &'a Item)> {
        match &module_item.inner {
            ItemEnum::Module(module) => module
                .items
                .iter()
                .filter_map(|id| self.krate.index.get(id).map(|item| (id, item)))
                .collect(),
            _ => vec![],
        }
    }

    /// Resolve the full module path for a given item ID.
    /// Returns the path of the module *containing* the item.
    #[allow(dead_code)]
    pub fn containing_module_path(&self, item_id: &Id) -> Option<&str> {
        self.item_module_path.get(item_id).map(|s| s.as_str())
    }

    /// Get the full module path for a module ID.
    pub fn module_path(&self, module_id: &Id) -> Option<&str> {
        // Check module_index values
        for (path, id) in &self.module_index {
            if id == module_id {
                return Some(path.as_str());
            }
        }
        None
    }

    /// Check if `ancestor_path` is an ancestor of (or equal to) `descendant_path`.
    pub fn is_ancestor_or_equal(ancestor_path: &str, descendant_path: &str) -> bool {
        if ancestor_path == descendant_path {
            return true;
        }
        descendant_path.starts_with(ancestor_path)
            && descendant_path.as_bytes().get(ancestor_path.len()) == Some(&b':')
    }
}

/// Determine if an item is visible from a given observer module.
pub fn is_visible_from(
    model: &CrateModel,
    item: &Item,
    _item_id: &Id,
    observer_module_path: &str,
    same_crate: bool,
) -> bool {
    match &item.visibility {
        Visibility::Public => true,
        Visibility::Crate => same_crate,
        Visibility::Restricted { parent, path: _ } => {
            if !same_crate {
                return false;
            }
            // The item is visible within the module identified by `parent`.
            // Check if the observer is within that module.
            if let Some(restricted_path) = model.module_path(parent) {
                CrateModel::is_ancestor_or_equal(restricted_path, observer_module_path)
            } else {
                false
            }
        }
        Visibility::Default => {
            // `default` visibility is used for impl blocks and their items.
            // These are visible if they're on a type that is visible.
            // For simplicity, we treat default as "same module only" for non-impl items,
            // and delegate impl visibility to the parent type.
            false
        }
    }
}

/// Compute the set of item IDs reachable through the crate's public API.
///
/// Walks from the root module following only `Visibility::Public` children.
/// For `pub use` re-exports targeting local items, marks the target and its
/// ancestor modules as reachable (so private modules containing reachable
/// items are rendered). For reachable structs/enums/unions, marks their
/// impl blocks as reachable.
pub fn compute_reachable_set(model: &CrateModel) -> HashSet<Id> {
    let mut reachable = HashSet::new();

    let Some(root) = model.root_module() else {
        return reachable;
    };

    // Root module is always reachable
    reachable.insert(model.krate.root);

    walk_public(model, root, model.crate_name(), &mut reachable);
    reachable
}

fn walk_public(
    model: &CrateModel,
    module_item: &Item,
    module_path: &str,
    reachable: &mut HashSet<Id>,
) {
    let children = model.module_children(module_item);

    for (child_id, child) in &children {
        if !matches!(child.visibility, Visibility::Public) {
            continue;
        }

        match &child.inner {
            ItemEnum::Module(_) => {
                reachable.insert(**child_id);
                let child_mod_path = match &child.name {
                    Some(name) => format!("{module_path}::{name}"),
                    None => module_path.to_string(),
                };
                walk_public(model, child, &child_mod_path, reachable);
            }
            ItemEnum::Use(use_item) if !use_item.is_glob => {
                reachable.insert(**child_id);
                if let Some(target_id) = &use_item.id {
                    mark_reachable_with_ancestors(model, target_id, reachable);
                }
            }
            ItemEnum::Use(use_item) => {
                reachable.insert(**child_id);
                // Follow intra-crate glob re-exports: resolve the source module
                // and walk its public items into the reachable set.
                // Cross-crate sources won't be in module_index → harmlessly skipped.
                let source = use_item
                    .source
                    .strip_prefix("self::")
                    .unwrap_or(&use_item.source);
                // Try relative to current module first (handles nested private modules
                // like bevy's `mod bind_group; pub use bind_group::*;` inside render_resource)
                let relative = format!("{module_path}::{source}");
                if let Some((mod_id, mod_item)) = model.find_module_entry(&relative) {
                    if reachable.insert(*mod_id) {
                        walk_public(model, mod_item, &relative, reachable);
                    }
                } else if let Some((mod_id, mod_item)) = model.find_module_entry(source) {
                    if reachable.insert(*mod_id) {
                        let full = format!("{}::{source}", model.crate_name());
                        walk_public(model, mod_item, &full, reachable);
                    }
                }
            }
            _ => {
                reachable.insert(**child_id);
                mark_impls(model, child, reachable);
            }
        }
    }
}

/// Mark an item reachable, along with its ancestor modules and impl blocks.
fn mark_reachable_with_ancestors(model: &CrateModel, item_id: &Id, reachable: &mut HashSet<Id>) {
    let Some(item) = model.krate.index.get(item_id) else {
        return;
    };

    reachable.insert(*item_id);
    mark_impls(model, item, reachable);

    // Walk ancestor modules up to root
    if let Some(parent_path) = model.item_module_path.get(item_id) {
        let mut path = parent_path.as_str();
        loop {
            if let Some(mod_id) = model.module_index.get(path) {
                reachable.insert(*mod_id);
            }
            match path.rsplit_once("::") {
                Some((parent, _)) => path = parent,
                None => break,
            }
        }
    }
}

/// Mark impl blocks of a struct/enum/union as reachable.
fn mark_impls(model: &CrateModel, item: &Item, reachable: &mut HashSet<Id>) {
    let impls = match &item.inner {
        ItemEnum::Struct(s) => &s.impls,
        ItemEnum::Enum(e) => &e.impls,
        ItemEnum::Union(u) => &u.impls,
        _ => return,
    };
    for impl_id in impls {
        if model.krate.index.contains_key(impl_id) {
            reachable.insert(*impl_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ancestor_or_equal() {
        assert!(CrateModel::is_ancestor_or_equal("foo", "foo"));
        assert!(CrateModel::is_ancestor_or_equal("foo", "foo::bar"));
        assert!(CrateModel::is_ancestor_or_equal("foo", "foo::bar::baz"));
        assert!(!CrateModel::is_ancestor_or_equal("foo", "foobar"));
        assert!(!CrateModel::is_ancestor_or_equal("foo::bar", "foo"));
        assert!(!CrateModel::is_ancestor_or_equal("foo::bar", "foo::baz"));
    }
}
