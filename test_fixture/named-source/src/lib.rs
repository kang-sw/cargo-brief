/// A struct for testing named cross-crate re-export expansion.
pub struct NamedSourceItem {
    pub ns_field: String,
}

/// A trait for testing named cross-crate re-export expansion.
pub trait NamedSourceTrait {
    fn named_method(&self);
}
