/// The main application struct.
pub struct App {
    /// Public config from core-lib.
    pub config: core_lib::Config,
    /// Private app state.
    running: bool,
}

impl App {
    /// Create a new app with default config.
    pub fn new() -> Self {
        App {
            config: core_lib::create_default_config(),
            running: false,
        }
    }

    /// Start the app.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Internal shutdown procedure.
    pub(crate) fn shutdown_internal(&mut self) {
        self.running = false;
    }
}

/// Run the application.
pub fn run() {
    let mut app = App::new();
    app.start();
}
