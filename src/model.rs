use std::collections::HashMap;

use rustdoc_types::{Crate, Id, Item, ItemEnum, Visibility};

/// A processed view of a crate's items, organized by module hierarchy.
pub struct CrateModel {
    pub krate: Crate,
    /// Maps module paths (e.g., "outer::inner") to their item IDs.
    pub module_index: HashMap<String, Id>,
    /// Maps item IDs to their containing module path.
    #[allow(dead_code)]
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
        let full_path = format!("{}::{}", self.crate_name(), module_path);
        let id = self
            .module_index
            .get(&full_path)
            .or_else(|| self.module_index.get(module_path))?;
        self.krate.index.get(id)
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
