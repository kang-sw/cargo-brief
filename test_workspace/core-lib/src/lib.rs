pub mod utils;

/// Configuration for the system.
pub struct Config {
    /// The public name field.
    pub name: String,
    /// Internal ID, only visible within the crate.
    pub(crate) internal_id: u64,
    /// Private implementation detail.
    secret: Vec<u8>,
}

impl Config {
    /// Create a new config with a name.
    pub fn new(name: &str) -> Self {
        Config {
            name: name.to_string(),
            internal_id: 0,
            secret: Vec::new(),
        }
    }

    /// Get the internal ID (crate-only).
    pub(crate) fn get_internal_id(&self) -> u64 {
        self.internal_id
    }

    fn private_method(&self) -> &[u8] {
        &self.secret
    }
}

/// Internal state, only visible within the crate.
pub(crate) struct InternalState {
    pub active: bool,
    pub counter: u32,
}

/// A public trait for processing items.
pub trait Processor {
    /// Process a single item.
    fn process(&self, input: &str) -> String;
}

impl Processor for Config {
    fn process(&self, input: &str) -> String {
        format!("{}: {input}", self.name)
    }
}

/// Create a default config.
pub fn create_default_config() -> Config {
    Config::new("default")
}

/// Internal helper function.
pub(crate) fn internal_helper() -> bool {
    true
}

/// Public constant.
pub const VERSION: &str = "0.1.0";

/// Crate-only constant.
pub(crate) const INTERNAL_LIMIT: u32 = 100;

/// Re-export format_name at crate root.
pub use utils::format_name;
