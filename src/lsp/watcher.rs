//! Filesystem watcher for LSP daemon: monitors `.rs`, `Cargo.toml`, `Cargo.lock`
//! changes and translates them into LSP `workspace/didChangeWatchedFiles` notifications.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// A pre-translated LSP file change event.
#[derive(Debug, Clone)]
pub struct FileEvent {
    pub uri: String,
    pub change_type: u32,
}

/// Start a filesystem watcher on the workspace root.
///
/// Returns the watcher handle (must be kept alive) and a receiver for file events.
/// The watcher filters for `.rs` files, `Cargo.toml`, and `Cargo.lock`, rejecting
/// paths under `target/` or hidden directories.
pub fn start_watcher(
    workspace_root: &Path,
) -> anyhow::Result<(RecommendedWatcher, Receiver<FileEvent>)> {
    let (tx, rx) = mpsc::channel::<FileEvent>();

    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            let event = match result {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[lsp-daemon] watch error: {e}");
                    return;
                }
            };

            let change_type = match event.kind {
                EventKind::Create(_) => 1,
                EventKind::Modify(_) => 2,
                EventKind::Remove(_) => 3,
                _ => return,
            };

            for path in &event.paths {
                if !should_watch_path(path) {
                    continue;
                }
                let uri = format!("file://{}", path.display());
                tx.send(FileEvent { uri, change_type }).ok();
            }
        },
        notify::Config::default(),
    )?;

    watcher.watch(workspace_root, RecursiveMode::Recursive)?;

    Ok((watcher, rx))
}

/// Check if a path should be watched (not in target/ or hidden dirs, and has
/// an accepted extension/filename).
fn should_watch_path(path: &Path) -> bool {
    // Reject paths with `target` component or hidden dirs (starting with `.`)
    for component in path.components() {
        if let std::path::Component::Normal(s) = component {
            let s = s.to_string_lossy();
            if s == "target" || s.starts_with('.') {
                return false;
            }
        }
    }

    // Accept .rs files
    if path.extension().is_some_and(|ext| ext == "rs") {
        return true;
    }

    // Accept Cargo.toml and Cargo.lock
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        if name == "Cargo.toml" || name == "Cargo.lock" {
            return true;
        }
    }

    false
}

/// Debounce buffer that collects file events and flushes them after 300ms.
pub struct DebounceBuffer {
    events: Vec<FileEvent>,
    first_event_time: Option<Instant>,
}

const DEBOUNCE_MS: u128 = 300;

impl DebounceBuffer {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            first_event_time: None,
        }
    }

    pub fn push(&mut self, event: FileEvent) {
        if self.first_event_time.is_none() {
            self.first_event_time = Some(Instant::now());
        }
        self.events.push(event);
    }

    pub fn should_flush(&self) -> bool {
        self.first_event_time
            .is_some_and(|t| t.elapsed().as_millis() > DEBOUNCE_MS)
    }

    /// Drain the buffer, deduplicating by URI (keeping the latest change_type per URI).
    pub fn drain(&mut self) -> Vec<FileEvent> {
        self.first_event_time = None;

        // Use IndexMap-like approach: iterate forward, last insert wins
        let mut seen = std::collections::HashMap::<String, u32>::new();
        let mut order = Vec::<String>::new();

        for event in self.events.drain(..) {
            if !seen.contains_key(&event.uri) {
                order.push(event.uri.clone());
            }
            seen.insert(event.uri, event.change_type);
        }

        order
            .into_iter()
            .map(|uri| {
                let change_type = seen[&uri];
                FileEvent { uri, change_type }
            })
            .collect()
    }
}

/// Build the LSP `workspace/didChangeWatchedFiles` notification params.
pub fn build_did_change_notification(events: &[FileEvent]) -> serde_json::Value {
    let changes: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "uri": e.uri,
                "type": e.change_type,
            })
        })
        .collect();

    serde_json::json!({ "changes": changes })
}

#[cfg(test)]
mod tests {
    use super::*;

    // === TDD: DebounceBuffer tests ===

    #[test]
    fn empty_buffer_should_not_flush() {
        let buf = DebounceBuffer::new();
        assert!(!buf.should_flush());
        assert!(buf.events.is_empty());
    }

    #[test]
    fn empty_buffer_drain_returns_empty() {
        let mut buf = DebounceBuffer::new();
        let result = buf.drain();
        assert!(result.is_empty());
    }

    #[test]
    fn single_event_not_flushed_immediately() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/main.rs".to_string(),
            change_type: 2,
        });
        // Immediately after push, should not flush (< 300ms)
        assert!(!buf.should_flush());
    }

    #[test]
    fn single_event_flushed_after_delay() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/main.rs".to_string(),
            change_type: 2,
        });
        // Override first_event_time to simulate passage of time
        buf.first_event_time = Some(Instant::now() - std::time::Duration::from_millis(400));
        assert!(buf.should_flush());

        let events = buf.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].uri, "file:///src/main.rs");
        assert_eq!(events[0].change_type, 2);
    }

    #[test]
    fn dedup_keeps_latest_change_type() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/lib.rs".to_string(),
            change_type: 1, // Created
        });
        buf.push(FileEvent {
            uri: "file:///src/lib.rs".to_string(),
            change_type: 2, // Modified
        });
        buf.first_event_time = Some(Instant::now() - std::time::Duration::from_millis(400));

        let events = buf.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change_type, 2); // latest wins
    }

    #[test]
    fn different_uris_all_preserved() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/a.rs".to_string(),
            change_type: 1,
        });
        buf.push(FileEvent {
            uri: "file:///src/b.rs".to_string(),
            change_type: 2,
        });
        buf.push(FileEvent {
            uri: "file:///Cargo.toml".to_string(),
            change_type: 2,
        });
        buf.first_event_time = Some(Instant::now() - std::time::Duration::from_millis(400));

        let events = buf.drain();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn create_then_delete_results_in_delete() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/temp.rs".to_string(),
            change_type: 1, // Created
        });
        buf.push(FileEvent {
            uri: "file:///src/temp.rs".to_string(),
            change_type: 3, // Deleted
        });
        buf.first_event_time = Some(Instant::now() - std::time::Duration::from_millis(400));

        let events = buf.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change_type, 3); // Delete wins
    }

    #[test]
    fn after_drain_buffer_is_empty() {
        let mut buf = DebounceBuffer::new();
        buf.push(FileEvent {
            uri: "file:///src/main.rs".to_string(),
            change_type: 2,
        });
        buf.first_event_time = Some(Instant::now() - std::time::Duration::from_millis(400));

        let _ = buf.drain();
        assert!(!buf.should_flush());
        assert!(buf.events.is_empty());
    }

    // === Post-impl: event filtering tests ===

    #[test]
    fn accept_rs_file() {
        assert!(should_watch_path(Path::new("/project/src/main.rs")));
    }

    #[test]
    fn accept_cargo_toml() {
        assert!(should_watch_path(Path::new("/project/Cargo.toml")));
    }

    #[test]
    fn accept_cargo_lock() {
        assert!(should_watch_path(Path::new("/project/Cargo.lock")));
    }

    #[test]
    fn reject_target_dir() {
        assert!(!should_watch_path(Path::new(
            "/project/target/debug/foo.rs"
        )));
    }

    #[test]
    fn reject_hidden_dir() {
        assert!(!should_watch_path(Path::new("/project/.git/HEAD")));
    }

    #[test]
    fn reject_non_rs_file() {
        assert!(!should_watch_path(Path::new("/project/src/foo.txt")));
        assert!(!should_watch_path(Path::new("/project/README.md")));
    }

    #[test]
    fn reject_nested_hidden_dir() {
        assert!(!should_watch_path(Path::new("/project/.cargo/config.toml")));
    }

    #[test]
    fn accept_nested_rs_file() {
        assert!(should_watch_path(Path::new("/project/src/lsp/watcher.rs")));
    }

    // === build_did_change_notification tests ===

    #[test]
    fn notification_format() {
        let events = vec![
            FileEvent {
                uri: "file:///src/main.rs".to_string(),
                change_type: 2,
            },
            FileEvent {
                uri: "file:///Cargo.toml".to_string(),
                change_type: 1,
            },
        ];
        let params = build_did_change_notification(&events);
        let changes = params["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["uri"], "file:///src/main.rs");
        assert_eq!(changes[0]["type"], 2);
        assert_eq!(changes[1]["uri"], "file:///Cargo.toml");
        assert_eq!(changes[1]["type"], 1);
    }
}
