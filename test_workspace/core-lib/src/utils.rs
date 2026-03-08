/// Format a name for display.
pub fn format_name(name: &str) -> String {
    name.trim().to_uppercase()
}

/// Helper function visible to parent module only.
pub(super) fn parent_visible_helper() -> bool {
    true
}

/// Crate-level utility.
pub(crate) fn crate_util(x: u32) -> u32 {
    x * 2
}

/// Private helper, never visible outside this module.
fn private_helper() -> &'static str {
    "secret"
}

/// A utility struct visible only within the crate.
pub(crate) struct UtilConfig {
    pub value: i32,
}

/// A public enum in utils.
pub enum LogLevel {
    /// Debug level.
    Debug,
    /// Info level.
    Info,
    /// Warning level.
    Warn,
    /// Error level.
    Error,
}
