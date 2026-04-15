//! GAR-259: Filtered filesystem watcher backed by [`notify`].
//!
//! Watches a directory tree and emits [`WatchEvent`]s **only** for paths that:
//!
//! 1. Are relative to (and inside) the configured root — path traversal is blocked.
//! 2. Are **not** excluded by `.gitignore` / `.garraignore` rules (same as [`Scanner`]).
//! 3. Match at least one include [`GlobPattern`] (when any are configured).
//! 4. Are not suppressed by an explicit exclude [`GlobPattern`].
//!
//! Debouncing is intentionally absent here (that is GAR-260). Consumers may
//! receive duplicate or rapid-fire events during saves; they should coalesce
//! as needed.
//!
//! # Example
//!
//! ```no_run
//! use garraia_glob::watcher::{WatcherBuilder, WatchEventKind};
//! use garraia_glob::pattern::GlobConfig;
//!
//! let watcher = WatcherBuilder::new("./src", GlobConfig::default())
//!     .include("**/*.rs").unwrap()
//!     .start()
//!     .unwrap();
//!
//! while let Some(event) = watcher.recv() {
//!     match event.kind {
//!         WatchEventKind::Modified => println!("changed: {}", event.path),
//!         WatchEventKind::Created  => println!("created: {}", event.path),
//!         WatchEventKind::Removed  => println!("removed: {}", event.path),
//!         WatchEventKind::Renamed { from } => println!("renamed: {} -> {}", from, event.path),
//!     }
//! }
//! ```
//!
//! [`Scanner`]: crate::scanner::Scanner

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{
    ignore::IgnoreFile,
    pattern::{GlobConfig, GlobPattern},
    GlobError, Result,
};

// ── Public types ──────────────────────────────────────────────────────────────

/// The kind of filesystem change that occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEventKind {
    /// A new file or directory appeared.
    Created,
    /// An existing file's content was modified.
    Modified,
    /// A file or directory was deleted.
    Removed,
    /// A file or directory was renamed. `from` holds the **original** relative path.
    Renamed { from: String },
}

/// A filtered, normalized filesystem event emitted by [`ActiveWatcher`].
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// Relative path from the watcher root, normalized (`\` → `/`).
    pub path: String,
    /// Nature of the change.
    pub kind: WatchEventKind,
    /// Wall-clock time the event was received (best-effort).
    pub timestamp: SystemTime,
}

/// A running watcher. Dropping this value stops all filesystem monitoring.
///
/// Consume events via [`recv`](ActiveWatcher::recv) or
/// [`try_recv`](ActiveWatcher::try_recv).
pub struct ActiveWatcher {
    /// Filtered event channel. All events have already passed through the
    /// ignore/include/exclude rules configured on [`WatcherBuilder`].
    pub receiver: mpsc::Receiver<WatchEvent>,
    // Keeps the notify watcher alive; dropping it unregisters the OS watch.
    _watcher: RecommendedWatcher,
}

impl ActiveWatcher {
    /// Block until the next filtered event arrives, or return `None` if the
    /// channel has been disconnected (watcher dropped or error).
    pub fn recv(&self) -> Option<WatchEvent> {
        self.receiver.recv().ok()
    }

    /// Non-blocking: return the next event if one is immediately available.
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.receiver.try_recv().ok()
    }

    /// Decompose this watcher into a raw event receiver and a [`WatcherGuard`].
    ///
    /// The `WatcherGuard` **must** be kept alive for OS events to continue
    /// flowing into the receiver. Typically used by [`crate::debouncer::Debouncer`]
    /// to move the receiver to a background thread while keeping the guard alive
    /// in the `Debouncer` struct.
    pub fn split(self) -> (mpsc::Receiver<WatchEvent>, WatcherGuard) {
        (
            self.receiver,
            WatcherGuard {
                _watcher: self._watcher,
            },
        )
    }
}

/// Keeps the underlying notify watcher alive after [`ActiveWatcher::split`].
///
/// Dropping this stops OS-level filesystem monitoring.
pub struct WatcherGuard {
    _watcher: RecommendedWatcher,
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for a filtered filesystem watcher.
///
/// Mirror of [`Scanner`](crate::scanner::Scanner) — same include / exclude /
/// ignore configuration API.
pub struct WatcherBuilder {
    root: PathBuf,
    include: Vec<GlobPattern>,
    exclude: Vec<GlobPattern>,
    use_gitignore: bool,
    use_garraignore: bool,
    config: GlobConfig,
}

impl WatcherBuilder {
    /// Create a new builder rooted at `root` with the given glob config.
    pub fn new(root: impl AsRef<Path>, config: GlobConfig) -> Self {
        WatcherBuilder {
            root: root.as_ref().to_path_buf(),
            include: Vec::new(),
            exclude: Vec::new(),
            use_gitignore: true,
            use_garraignore: true,
            config,
        }
    }

    /// Add an include pattern. Only paths matching at least one include are
    /// emitted. If **no** include patterns are added, all paths pass (subject
    /// to excludes and ignore files).
    pub fn include(mut self, pattern: &str) -> Result<Self> {
        self.include.push(GlobPattern::new(pattern, &self.config)?);
        Ok(self)
    }

    /// Add an exclude pattern. Paths matching any exclude are suppressed.
    pub fn exclude(mut self, pattern: &str) -> Result<Self> {
        self.exclude.push(GlobPattern::new(pattern, &self.config)?);
        Ok(self)
    }

    /// Whether to load and respect `.gitignore` files found in the root.
    /// Default: `true`.
    pub fn use_gitignore(mut self, val: bool) -> Self {
        self.use_gitignore = val;
        self
    }

    /// Whether to load and respect `.garraignore` files found in the root.
    /// Default: `true`.
    pub fn use_garraignore(mut self, val: bool) -> Self {
        self.use_garraignore = val;
        self
    }

    /// Start the watcher and return an [`ActiveWatcher`].
    ///
    /// Loads root-level ignore files immediately (same strategy as [`Scanner`]).
    /// Subdirectory ignore files are **not** dynamically loaded after start —
    /// only root-level `.gitignore`/`.garraignore` are respected.
    pub fn start(self) -> Result<ActiveWatcher> {
        if !self.root.exists() {
            return Err(GlobError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("watch root does not exist: {}", self.root.display()),
            )));
        }

        // Load root-level ignore files once at startup — same strategy as Scanner.
        let mut root_ignores: Vec<IgnoreFile> = Vec::new();
        if self.use_gitignore {
            if let Ok(ig) = IgnoreFile::from_path(self.root.join(".gitignore")) {
                root_ignores.push(ig);
            }
        }
        if self.use_garraignore {
            if let Ok(ig) = IgnoreFile::from_garraignore_path(self.root.join(".garraignore")) {
                root_ignores.push(ig);
            }
        }

        // Move all filter state into the closure (owned, no Arc needed).
        let root = self.root.clone();
        let include = self.include;
        let exclude = self.exclude;
        let ignores = root_ignores;

        let (tx, rx) = mpsc::channel::<WatchEvent>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(err = %e, "garraia_glob::watcher: notify error");
                    return;
                }
            };

            let timestamp = SystemTime::now();

            // ── Rename (batched) — paths[0]=from, paths[1]=to ──────────────
            if is_rename_both(&event.kind) && event.paths.len() >= 2 {
                let from_rel = match relative(&event.paths[0], &root) {
                    Some(r) => r,
                    None => return,
                };
                let to_rel = match relative(&event.paths[1], &root) {
                    Some(r) => r,
                    None => return,
                };
                if passes_filters(&to_rel, &ignores, &include, &exclude) {
                    let _ = tx.send(WatchEvent {
                        path: to_rel,
                        kind: WatchEventKind::Renamed { from: from_rel },
                        timestamp,
                    });
                }
                return;
            }

            // ── All other events — one kind, one or more paths ─────────────
            let kind = match classify(&event.kind) {
                Some(k) => k,
                None => return, // access-only, metadata-only, unknown — skip
            };

            for abs in &event.paths {
                let rel = match relative(abs, &root) {
                    Some(r) => r,
                    None => {
                        tracing::debug!(
                            path = %abs.display(),
                            "garraia_glob::watcher: path outside root, skipping"
                        );
                        continue;
                    }
                };

                if !passes_filters(&rel, &ignores, &include, &exclude) {
                    continue;
                }

                tracing::debug!(path = %rel, kind = ?kind, "garraia_glob::watcher: event");
                let _ = tx.send(WatchEvent {
                    path: rel,
                    kind: kind.clone(),
                    timestamp,
                });
            }
        })
        .map_err(|e| {
            GlobError::Io(std::io::Error::other(format!(
                "garraia_glob::watcher: init failed: {e}"
            )))
        })?;

        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| {
                GlobError::Io(std::io::Error::other(format!(
                    "garraia_glob::watcher: watch failed: {e}"
                )))
            })?;

        Ok(ActiveWatcher {
            receiver: rx,
            _watcher: watcher,
        })
    }
}

// ── private helpers ───────────────────────────────────────────────────────────

/// Compute a normalized relative path from `root`. Returns `None` if the path
/// is outside `root` (path traversal guard) or equal to root (empty rel).
fn relative(abs: &Path, root: &Path) -> Option<String> {
    abs.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|s| !s.is_empty())
}

/// Returns `true` when `kind` represents a batched rename (from + to in one event).
fn is_rename_both(kind: &EventKind) -> bool {
    use notify::event::{ModifyKind, RenameMode};
    matches!(kind, EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
}

/// Map a `notify::EventKind` to a [`WatchEventKind`]. Returns `None` for events
/// that should be silently discarded (access-only, metadata-only, etc.).
fn classify(kind: &EventKind) -> Option<WatchEventKind> {
    use notify::event::{ModifyKind, RenameMode};
    match kind {
        EventKind::Create(_) => Some(WatchEventKind::Created),
        // Data change
        EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Any)
        | EventKind::Modify(ModifyKind::Other) => Some(WatchEventKind::Modified),
        // Rename (separate from/to events — not batched)
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some(WatchEventKind::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => Some(WatchEventKind::Created),
        // Any other rename that isn't `Both` — treat as modify
        EventKind::Modify(ModifyKind::Name(_)) => Some(WatchEventKind::Modified),
        // Permission/timestamp changes — not interesting
        EventKind::Modify(ModifyKind::Metadata(_)) => None,
        EventKind::Remove(_) => Some(WatchEventKind::Removed),
        // Access events, unknown kinds — skip
        _ => None,
    }
}

/// Returns `true` if `rel` passes all ignore, include, and exclude filters.
fn passes_filters(
    rel: &str,
    ignores: &[IgnoreFile],
    include: &[GlobPattern],
    exclude: &[GlobPattern],
) -> bool {
    // Ignore rules take highest priority.
    if ignores.iter().any(|ig| ig.is_ignored(rel)) {
        tracing::debug!(path = %rel, "garraia_glob::watcher: suppressed by ignore rule");
        return false;
    }
    // Explicit excludes.
    if exclude.iter().any(|p| p.matches(rel)) {
        tracing::debug!(path = %rel, "garraia_glob::watcher: suppressed by exclude pattern");
        return false;
    }
    // Include filter (if any patterns are configured, path must match at least one).
    if !include.is_empty() && !include.iter().any(|p| p.matches(rel)) {
        tracing::debug!(path = %rel, "garraia_glob::watcher: not matched by include pattern");
        return false;
    }
    true
}
