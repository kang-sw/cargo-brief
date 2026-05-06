pub use glob_inner::*;
pub use glob_inner as inner_alias;

/// A struct defined directly in the source crate.
pub struct GlobSourceItem {
    pub field: i32,
}

/// A function defined in a re-exported source crate.
pub fn glob_source_fn(
    path: std::path::PathBuf,
    label: Option<String>,
) -> Result<String, String> {
    Ok(format!("{}:{label:?}", path.display()))
}
