//! Filesystem watcher for detecting external edits to the edit document.
//!
//! Uses the `notify` crate to watch the edit JSON file for modifications
//! from CLI commands or manual edits.  Events are sent over an
//! `mpsc::Receiver` that the TUI event loop polls each tick.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Minimum interval between delivering file-change events to avoid
/// rapid-fire reloads (e.g. editors that write + rename atomically).
const DEBOUNCE: Duration = Duration::from_millis(300);

/// A lightweight handle returned to the event loop.
///
/// Call [`FileWatcher::poll`] each tick to check for changes.
/// Dropping the handle stops the watcher.
pub struct FileWatcher {
    rx: Receiver<()>,
    _watcher: RecommendedWatcher,
    last_event: Instant,
}

impl FileWatcher {
    /// Start watching `path` for modifications.
    ///
    /// Returns `None` if the path does not exist or the watcher cannot be
    /// initialised (non-fatal — the TUI will simply run without live reload).
    pub fn new(path: &Path) -> Option<Self> {
        // Watch the parent directory so we also catch atomic-save patterns
        // (write to tmp + rename) that some editors use.
        let dir = path.parent()?;
        let watched_name: PathBuf = path.file_name()?.into();

        let (tx, rx) = mpsc::channel();

        let target = watched_name.clone();
        let mut watcher =
            RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(ev) = res {
                        let dominated = matches!(
                            ev.kind,
                            EventKind::Modify(_) | EventKind::Create(_)
                        );
                        if dominated
                            && ev.paths.iter().any(|p| {
                                p.file_name()
                                    .map(|n| n == target.as_os_str())
                                    .unwrap_or(false)
                            })
                        {
                            // Best-effort send; if the channel is full the loop
                            // will pick up the next one.
                            let _ = tx.send(());
                        }
                    }
                },
                Config::default(),
            )
            .ok()?;

        watcher.watch(dir, RecursiveMode::NonRecursive).ok()?;

        Some(Self {
            rx,
            _watcher: watcher,
            last_event: Instant::now() - DEBOUNCE, // allow immediate first event
        })
    }

    /// Non-blocking poll: returns `true` if the watched file has changed
    /// since the last poll (debounced).
    pub fn poll(&mut self) -> bool {
        // Drain all queued notifications.
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(()) => changed = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        if changed && self.last_event.elapsed() >= DEBOUNCE {
            self.last_event = Instant::now();
            true
        } else {
            false
        }
    }
}
