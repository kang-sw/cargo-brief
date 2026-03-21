/// A struct from the innermost crate in a cross-crate glob chain.
pub struct GlobInnerItem {
    pub deep_field: String,
}

/// A trait from the innermost crate in a cross-crate glob chain.
pub trait GlobInnerTrait {
    fn inner_method(&self);
}
