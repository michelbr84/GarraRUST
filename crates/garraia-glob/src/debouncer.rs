//! GAR-260: Event debouncer for the glob file watcher.
//!
//! Wraps an [`ActiveWatcher`] with a coalescing buffer: events for the same
//! path that arrive within a configurable `window` are merged into a single
//! [`WatchEvent`] before being forwarded to the consumer.
//!
//! ## Coalescing rules
//!
//! | Pending → New      | Result        | Rationale |
//! |--------------------|---------------|-----------|
//! | Created + Modified | Created       | File created then immediately edited |
//! | Created + Removed  | *(dropped)*   | Temp file — never existed from consumer's POV |
//! | Created + Renamed  | Renamed       | Created, then renamed before window expired |
//! | Modified + Modified| Modified      | Multiple rapid saves → one event |
//! | Modified + Removed | Removed       | File was deleted |
//! | Modified + Renamed | Renamed       | Renamed, prior edits irrelevant |
//! | Removed + Created  | Modified      | File replaced (delete + create = modify) |
//! | Removed + Modified | Modified      | Unlikely but handled |
//! | Renamed + Modified | Renamed       | Content edited after rename; Renamed wins |
//! | Renamed + Removed  | Removed       | Renamed then immediately deleted |
//! | *(any)* + *(same)* | Latest        | Fallback: latest event wins |
//!
//! ## Usage
//!
//! ```no_run
//! use garraia_glob::watcher::WatcherBuilder;
//! use garraia_glob::debouncer::Debouncer;
//! use garraia_glob::pattern::GlobConfig;
//!
//! let raw = WatcherBuilder::new("./src", GlobConfig::default())
//!     .include("**/*.rs").unwrap()
//!     .start().unwrap();
//!
//! let debouncer = Debouncer::new(raw, std::time::Duration::from_millis(300));
//!
//! while let Some(event) = debouncer.recv() {
//!     println!("{:?} {:?}", event.kind, event.path);
//! }
//! ```

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use crate::watcher::{ActiveWatcher, WatchEvent, WatchEventKind};

// ── Public API ────────────────────────────────────────────────────────────────

/// A debounced filesystem watcher.
///
/// Wraps an [`ActiveWatcher`] and coalesces rapid events per path within a
/// configurable time window. Dropping the `Debouncer` stops both the background
/// debounce thread and the underlying OS file watcher.
pub struct Debouncer {
    /// Stream of debounced events. Consume via [`recv`](Debouncer::recv) or
    /// [`try_recv`](Debouncer::try_recv).
    pub receiver: mpsc::Receiver<WatchEvent>,
    // Keep the notify watcher alive (via WatcherGuard) for the lifetime of Debouncer.
    _guard: crate::watcher::WatcherGuard,
    // Background coalescing thread; joined on drop via the channel disconnect.
    _thread: std::thread::JoinHandle<()>,
}

impl Debouncer {
    /// Wrap `watcher` with the default debounce window (300 ms).
    pub fn with_defaults(watcher: ActiveWatcher) -> Self {
        Self::new(watcher, Duration::from_millis(300))
    }

    /// Wrap `watcher` with a custom debounce `window`.
    ///
    /// Events for the same path that arrive within `window` of each other
    /// are coalesced into a single event.
    pub fn new(watcher: ActiveWatcher, window: Duration) -> Self {
        let (raw_rx, guard) = watcher.split();
        let (tx, rx) = mpsc::channel();

        let thread = std::thread::Builder::new()
            .name("garraia-glob-debouncer".into())
            .spawn(move || debounce_loop(raw_rx, tx, window))
            .expect("failed to spawn debouncer thread");

        Debouncer {
            receiver: rx,
            _guard: guard,
            _thread: thread,
        }
    }

    /// Block until the next debounced event arrives, or return `None` if the
    /// watcher has been shut down.
    pub fn recv(&self) -> Option<WatchEvent> {
        self.receiver.recv().ok()
    }

    /// Non-blocking: return the next debounced event if one is ready.
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.receiver.try_recv().ok()
    }
}

// ── Debounce loop (background thread) ────────────────────────────────────────

/// Internal representation of a pending (not-yet-emitted) event.
struct Pending {
    /// Coalesced event kind (may change as new events arrive).
    kind: PendingKind,
    /// First event timestamp — used as the emitted event's `timestamp`.
    timestamp: SystemTime,
    /// Time of the most recent raw event — used to detect the quiet period.
    last_seen: Instant,
}

impl Pending {
    fn new(event: WatchEvent) -> Self {
        Pending {
            kind: PendingKind::from(event.kind),
            timestamp: event.timestamp,
            last_seen: Instant::now(),
        }
    }

    /// Merge a new raw event into this pending entry.
    fn merge(&mut self, new_kind: WatchEventKind) {
        self.last_seen = Instant::now();
        self.kind = coalesce(
            std::mem::replace(&mut self.kind, PendingKind::Transient),
            new_kind,
        );
    }

    /// Convert to a `WatchEvent` for emission. Returns `None` for transient entries.
    fn into_event(self, path: String) -> Option<WatchEvent> {
        let kind = match self.kind {
            PendingKind::Created => WatchEventKind::Created,
            PendingKind::Modified => WatchEventKind::Modified,
            PendingKind::Removed => WatchEventKind::Removed,
            PendingKind::Renamed { from } => WatchEventKind::Renamed { from },
            PendingKind::Transient => return None,
        };
        Some(WatchEvent {
            path,
            kind,
            timestamp: self.timestamp,
        })
    }
}

/// Internal event kind that adds a `Transient` sentinel for suppressed events.
#[derive(Debug)]
enum PendingKind {
    Created,
    Modified,
    Removed,
    Renamed {
        from: String,
    },
    /// Created then immediately removed — drop, don't emit.
    Transient,
}

impl From<WatchEventKind> for PendingKind {
    fn from(k: WatchEventKind) -> Self {
        match k {
            WatchEventKind::Created => PendingKind::Created,
            WatchEventKind::Modified => PendingKind::Modified,
            WatchEventKind::Removed => PendingKind::Removed,
            WatchEventKind::Renamed { from } => PendingKind::Renamed { from },
        }
    }
}

/// Apply the coalescing table.
fn coalesce(pending: PendingKind, new: WatchEventKind) -> PendingKind {
    use PendingKind::*;
    use WatchEventKind as W;

    match (pending, new) {
        // Temp file: Created then immediately Removed → drop
        (Created, W::Removed) => Transient,
        // Created then Modified → still Created (will be emitted as Created)
        (Created, W::Modified) => Created,
        // Created then Renamed → Renamed (new name is the canonical path)
        (Created, W::Renamed { from }) => Renamed { from },

        // Modified repeatedly → one Modified
        (Modified, W::Modified) => Modified,
        // Modified then Removed → Removed
        (Modified, W::Removed) => Removed,
        // Modified then Renamed → Renamed
        (Modified, W::Renamed { from }) => Renamed { from },

        // Removed then Created → treat as Modified (file was replaced)
        (Removed, W::Created) => Modified,
        // Removed then Modified → Modified (recreated + edited)
        (Removed, W::Modified) => Modified,

        // Renamed then Modified → keep Renamed (content change is secondary)
        (Renamed { from }, W::Modified) => Renamed { from },
        // Renamed then Removed → Removed
        (Renamed { .. }, W::Removed) => Removed,

        // Transient events absorb everything (keep suppressed)
        (Transient, _) => Transient,

        // Fallback: new event wins
        (_, new) => PendingKind::from(new),
    }
}

/// Background thread: read raw events, coalesce, emit after quiet period.
fn debounce_loop(
    raw_rx: mpsc::Receiver<WatchEvent>,
    out_tx: mpsc::Sender<WatchEvent>,
    window: Duration,
) {
    // Poll at window/4 so we detect the quiet period with ≤25% extra latency.
    let check_interval = (window / 4).max(Duration::from_millis(25));
    let mut pending: HashMap<String, Pending> = HashMap::new();

    loop {
        // Drain all immediately available raw events before sleeping.
        loop {
            match raw_rx.try_recv() {
                Ok(event) => {
                    let path = event.path.clone();
                    let kind = event.kind.clone();
                    pending
                        .entry(path)
                        .and_modify(|p| p.merge(kind.clone()))
                        .or_insert_with(|| Pending::new(WatchEvent { kind, ..event }));
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    flush_all(pending, &out_tx);
                    return;
                }
            }
        }

        // Sleep for check_interval, then wake to flush ready entries.
        match raw_rx.recv_timeout(check_interval) {
            Ok(event) => {
                let path = event.path.clone();
                let kind = event.kind.clone();
                pending
                    .entry(path)
                    .and_modify(|p| p.merge(kind.clone()))
                    .or_insert_with(|| Pending::new(WatchEvent { kind, ..event }));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                flush_all(pending, &out_tx);
                return;
            }
        }

        // Emit all entries that have been quiet for at least `window`.
        let now = Instant::now();
        let ready: Vec<String> = pending
            .iter()
            .filter(|(_, p)| now.duration_since(p.last_seen) >= window)
            .map(|(k, _)| k.clone())
            .collect();

        for path in ready {
            if let Some(entry) = pending.remove(&path) {
                if let Some(event) = entry.into_event(path) {
                    if out_tx.send(event).is_err() {
                        return; // consumer dropped
                    }
                }
            }
        }
    }
}

/// Flush remaining pending events on shutdown.
fn flush_all(pending: HashMap<String, Pending>, tx: &mpsc::Sender<WatchEvent>) {
    for (path, entry) in pending {
        if let Some(event) = entry.into_event(path) {
            let _ = tx.send(event);
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use WatchEventKind as W;

    fn coal(a: PendingKind, b: W) -> PendingKind {
        coalesce(a, b)
    }

    #[test]
    fn coalesce_created_then_modified_stays_created() {
        assert!(matches!(
            coal(PendingKind::Created, W::Modified),
            PendingKind::Created
        ));
    }

    #[test]
    fn coalesce_created_then_removed_is_transient() {
        assert!(matches!(
            coal(PendingKind::Created, W::Removed),
            PendingKind::Transient
        ));
    }

    #[test]
    fn coalesce_modified_then_modified_stays_modified() {
        assert!(matches!(
            coal(PendingKind::Modified, W::Modified),
            PendingKind::Modified
        ));
    }

    #[test]
    fn coalesce_modified_then_removed_is_removed() {
        assert!(matches!(
            coal(PendingKind::Modified, W::Removed),
            PendingKind::Removed
        ));
    }

    #[test]
    fn coalesce_removed_then_created_is_modified() {
        assert!(matches!(
            coal(PendingKind::Removed, W::Created),
            PendingKind::Modified
        ));
    }

    #[test]
    fn coalesce_renamed_then_modified_keeps_renamed() {
        let from = "old.rs".to_string();
        let result = coal(PendingKind::Renamed { from: from.clone() }, W::Modified);
        assert!(matches!(result, PendingKind::Renamed { from: f } if f == from));
    }

    #[test]
    fn coalesce_renamed_then_removed_is_removed() {
        let result = coal(
            PendingKind::Renamed {
                from: "old.rs".into(),
            },
            W::Removed,
        );
        assert!(matches!(result, PendingKind::Removed));
    }

    #[test]
    fn coalesce_transient_absorbs_all() {
        assert!(matches!(
            coal(PendingKind::Transient, W::Created),
            PendingKind::Transient
        ));
        assert!(matches!(
            coal(PendingKind::Transient, W::Modified),
            PendingKind::Transient
        ));
        assert!(matches!(
            coal(PendingKind::Transient, W::Removed),
            PendingKind::Transient
        ));
    }

    #[test]
    fn transient_entry_emits_nothing() {
        let pending = Pending {
            kind: PendingKind::Transient,
            timestamp: SystemTime::now(),
            last_seen: Instant::now(),
        };
        assert!(pending.into_event("foo.rs".into()).is_none());
    }
}
