// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Filesystem watcher for hot-reload.
//!
//! Wraps [`notify::RecommendedWatcher`] (cross-platform — inotify on Linux,
//! FSEvents on macOS, ReadDirectoryChangesW on Windows) and translates raw
//! events into [`AssetChangeEvent`]s with pre-computed UUIDs.
//!
//! Each consumer calls [`AssetWatcher::poll_for`] with its own name and reads
//! the same stream from its own position — the editor's reindex pump, the
//! shader pipeline, and the script runtime all watch one directory and must all
//! see what happened in it.
//!
//! # Threading
//!
//! `notify` v6 spawns its own internal backend thread for the OS-level event
//! source (this is unavoidable — that's how kernel APIs deliver events). We
//! never call `std::thread::spawn` from this crate, so the workspace
//! convention "no `std::thread::spawn` in user code" is respected. The
//! crossbeam channel is bounded only by the internal handler closure; the
//! consumer side is non-blocking via [`AssetWatcher::poll_for`].

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use khora_core::asset::AssetUUID;
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::index_builder::should_skip_file;

/// What kind of change happened to an asset on disk.
///
/// Renames are decomposed into `Removed` + `Created` to keep consumer code
/// simple — handle two events instead of carrying a `from`/`to` pair through
/// the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetChangeKind {
    /// A new asset file appeared.
    Created,
    /// An existing asset file's bytes (or attributes) changed.
    Modified,
    /// An asset file was deleted.
    Removed,
}

/// One filesystem change against an asset under the watched root.
#[derive(Debug, Clone)]
pub struct AssetChangeEvent {
    /// What happened.
    pub kind: AssetChangeKind,
    /// Path relative to the watched assets root, forward-slash separated for
    /// cross-platform UUID stability.
    pub rel_path: String,
    /// UUID derived from `rel_path` via [`AssetUUID::new_v5`]. May not yet
    /// (or no longer) exist in the [`crate::vfs::VirtualFileSystem`] — the
    /// consumer reconciles by reindex + invalidate as appropriate.
    pub uuid: AssetUUID,
}

/// Drains filesystem-change events under a project's `assets/` directory.
///
/// Holds `notify`'s `RecommendedWatcher` alive — drop the [`AssetWatcher`]
/// to stop watching.
pub struct AssetWatcher {
    // Kept alive for its Drop side-effect (releases the OS handle).
    _watcher: RecommendedWatcher,
    receiver: Receiver<AssetChangeEvent>,
    assets_root: PathBuf,
    backlog: Mutex<Backlog>,
}

impl AssetWatcher {
    /// Starts watching `assets_root` recursively. Future writes / creates /
    /// removes under that tree produce events read via [`Self::poll_for`].
    ///
    /// Returns an error if `notify` fails to construct a recommended watcher
    /// or to register the path (e.g. the path doesn't exist or the OS denies
    /// the watch). The editor passes a path it has just `create_dir_all`'d,
    /// so this should be reliable in practice.
    pub fn new(assets_root: impl Into<PathBuf>) -> Result<Self> {
        let assets_root = assets_root.into();
        let (tx, rx): (Sender<AssetChangeEvent>, Receiver<AssetChangeEvent>) =
            crossbeam_channel::unbounded();
        let root_for_handler = assets_root.clone();

        let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("notify error: {e}");
                    return;
                }
            };
            for path in &event.paths {
                if let Some(change) = translate_event(&event.kind, path, &root_for_handler) {
                    let _ = tx.send(change);
                }
            }
        })
        .context("Failed to create filesystem watcher")?;

        watcher
            .watch(&assets_root, RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch {}", assets_root.display()))?;

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
            assets_root,
            backlog: Mutex::new(Backlog::default()),
        })
    }

    /// Returns the watched assets root.
    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }

    /// Everything `subscriber` has not yet seen.
    ///
    /// **Named, because there is more than one reader.** The shader pipeline
    /// wants `.wgsl` changes and the script runtime wants `.erg` ones, and both
    /// watch the same directory. A single drained channel gives every event to
    /// whichever system runs first and nothing to the second — a hot-reload that
    /// silently never fires, which is worse than one that is absent. Each
    /// subscriber therefore reads the same stream from its own position.
    ///
    /// Coalesces repeated `Modified` events on the same path within one ingest —
    /// `notify` v6 fires several for a single save on Windows. The order of
    /// distinct events is preserved.
    ///
    /// A name seen for the first time starts at the oldest retained event, so a
    /// system registered a frame late still hears what happened.
    pub fn poll_for(&self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        let Ok(mut backlog) = self.backlog.lock() else {
            log::error!("the asset watcher's backlog is poisoned; {subscriber} hears nothing");
            return Vec::new();
        };

        self.ingest(&mut backlog);
        backlog.read(subscriber)
    }

    /// Moves whatever the watcher thread has produced into the backlog.
    fn ingest(&self, backlog: &mut Backlog) {
        let mut seen_modified: HashSet<String> = HashSet::new();
        while let Ok(event) = self.receiver.try_recv() {
            if matches!(event.kind, AssetChangeKind::Modified)
                && !seen_modified.insert(event.rel_path.clone())
            {
                // Already taken a Modified for this path in this ingest.
                continue;
            }
            backlog.events.push_back(event);
        }
    }
}

/// The shared event stream, and where each subscriber has read to.
#[derive(Default)]
struct Backlog {
    events: VecDeque<AssetChangeEvent>,
    /// The position of `events[0]` in the stream as a whole.
    ///
    /// Counted rather than reset, so a cursor stays meaningful after a trim.
    base: u64,
    cursors: HashMap<&'static str, u64>,
}

impl Backlog {
    /// Everything `subscriber` has not read, advancing its cursor past it.
    ///
    /// Separate from [`AssetWatcher::poll_for`] so the part with the arithmetic
    /// in it can be tested without a filesystem and a watcher thread — the
    /// bookkeeping is what has to be right, and it does not need either.
    fn read(&mut self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        let base = self.base;
        let end = base + self.events.len() as u64;
        let cursor = self.cursors.entry(subscriber).or_insert(base);
        let from = (*cursor).max(base);
        *cursor = end;

        let unread: Vec<AssetChangeEvent> = self
            .events
            .iter()
            .skip((from - base) as usize)
            .cloned()
            .collect();

        self.trim();
        unread
    }

    /// Keeps the backlog bounded, oldest first.
    ///
    /// **Not "drop what every subscriber has read".** The backlog cannot know
    /// who the subscribers are: the first reader is the only cursor that exists
    /// when it reads, so trimming to the minimum cursor would discard every
    /// event before the second reader ever asked — which is the exact bug this
    /// whole mechanism was written to fix, reintroduced one layer down.
    ///
    /// A window instead. Every subscriber sees everything as long as it polls
    /// within [`RETAINED`] events of the change, which a per-frame system does
    /// by several orders of magnitude, and a subscriber that stopped polling
    /// cannot grow this without bound.
    fn trim(&mut self) {
        while self.events.len() > RETAINED {
            self.events.pop_front();
            self.base += 1;
        }
    }
}

/// How many changes the backlog keeps for readers that have not caught up.
///
/// Far more than a frame produces — a project-wide reformat is hundreds — and
/// small enough that the memory is irrelevant. It is a backstop against a
/// subscriber that stops reading, not a tuning knob.
const RETAINED: usize = 4096;

/// Maps a raw `notify::EventKind` + absolute path to one of our
/// [`AssetChangeEvent`]s. Returns `None` if the path isn't a recognized
/// asset (filtered via `asset_type_for_extension`) or if the event kind is
/// uninteresting (Access, Other, Any).
fn translate_event(
    kind: &notify::EventKind,
    abs: &Path,
    assets_root: &Path,
) -> Option<AssetChangeEvent> {
    use notify::event::{ModifyKind, RenameMode};
    let our_kind = match kind {
        notify::EventKind::Create(_) => AssetChangeKind::Created,
        notify::EventKind::Remove(_) => AssetChangeKind::Removed,
        // ModifyKind::Name(...) = renames. notify reports them as paired
        // Remove/Create on Linux but as Modify(Name(...)) on macOS/Windows.
        // We simplify: rename = Removed-then-Created (or vice versa) by
        // treating Modify(Name) as Modified — the consumer's reindex pass
        // will pick up the new path on the subsequent Create event anyway.
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::From)) => AssetChangeKind::Removed,
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::To)) => AssetChangeKind::Created,
        notify::EventKind::Modify(_) => AssetChangeKind::Modified,
        // Access / Any / Other: not interesting for hot-reload.
        _ => return None,
    };

    // Drop OS scratch files / editor swap files. Anything else is a
    // legitimate asset under `assets/` and should fire a hot-reload
    // event — the VFS now tracks every extension so the previous
    // canonical-allowlist filter no longer makes sense here.
    let file_name = abs.file_name().and_then(|n| n.to_str())?;
    if should_skip_file(file_name) {
        return None;
    }

    let rel = abs.strip_prefix(assets_root).ok()?;
    let rel_str = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");

    let uuid = AssetUUID::new_v5(&rel_str);
    Some(AssetChangeEvent {
        kind: our_kind,
        rel_path: rel_str,
        uuid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, thread::sleep, time::Duration};
    use tempfile::tempdir;

    /// notify is timing-dependent on every backend; this test allows up to
    /// `MAX_WAIT` for the watcher's own backend thread to deliver an event.
    /// Marked `#[ignore]` so CI can opt-in — it's flaky on heavily-loaded
    /// runners.
    const MAX_WAIT: Duration = Duration::from_secs(2);

    #[test]
    #[ignore = "filesystem-watcher tests are timing-dependent; run manually"]
    fn watcher_emits_event_on_create() {
        let dir = tempdir().unwrap();
        let watcher = AssetWatcher::new(dir.path()).unwrap();

        // Give the watcher backend a moment to arm.
        sleep(Duration::from_millis(100));

        fs::create_dir_all(dir.path().join("textures")).unwrap();
        fs::write(dir.path().join("textures").join("foo.png"), b"PNG").unwrap();

        let mut events = Vec::new();
        let deadline = std::time::Instant::now() + MAX_WAIT;
        while std::time::Instant::now() < deadline {
            events.extend(watcher.poll_for("test"));
            if events.iter().any(|e| e.rel_path == "textures/foo.png") {
                break;
            }
            sleep(Duration::from_millis(50));
        }

        assert!(
            events.iter().any(|e| e.rel_path == "textures/foo.png"),
            "expected at least one event for textures/foo.png; got {:?}",
            events
        );
    }
}

#[cfg(test)]
mod backlog_tests {
    use super::*;

    fn change(path: &str) -> AssetChangeEvent {
        AssetChangeEvent {
            kind: AssetChangeKind::Modified,
            rel_path: path.to_owned(),
            uuid: AssetUUID::new_v5(path),
        }
    }

    fn with(paths: &[&str]) -> Backlog {
        Backlog {
            events: paths.iter().map(|path| change(path)).collect(),
            ..Backlog::default()
        }
    }

    fn paths(events: &[AssetChangeEvent]) -> Vec<&str> {
        events.iter().map(|e| e.rel_path.as_str()).collect()
    }

    /// **The bug this exists for.** One drained channel gave every event to
    /// whichever system ran first, so the shader pump ate the script pump's
    /// `.erg` changes and script hot-reload silently never fired.
    #[test]
    fn two_subscribers_each_see_everything() {
        let mut backlog = with(&["shaders/pbr.wgsl", "scripts/guard.erg"]);

        assert_eq!(
            paths(&backlog.read("shaders")),
            ["shaders/pbr.wgsl", "scripts/guard.erg"]
        );
        assert_eq!(
            paths(&backlog.read("scripts")),
            ["shaders/pbr.wgsl", "scripts/guard.erg"],
            "reading did not consume it for anybody else"
        );
    }

    /// And each sees it once. A hot-reload that re-fired every frame would
    /// recompile the same module forever.
    #[test]
    fn a_subscriber_does_not_see_the_same_event_twice() {
        let mut backlog = with(&["scripts/guard.erg"]);

        assert_eq!(backlog.read("scripts").len(), 1);
        assert!(backlog.read("scripts").is_empty());
    }

    /// **What made the first attempt at this wrong.** Trimming to the minimum
    /// cursor discards everything as soon as the *first* reader has read,
    /// because it is the only cursor that exists yet — reintroducing the very
    /// bug one layer down. A reader that has not asked yet still gets to.
    #[test]
    fn reading_once_does_not_discard_what_nobody_else_has_seen() {
        let mut backlog = with(&["a.erg", "b.erg"]);

        backlog.read("shaders");
        assert_eq!(backlog.events.len(), 2, "still there for the other reader");
        assert_eq!(paths(&backlog.read("scripts")), ["a.erg", "b.erg"]);
    }

    /// It stays bounded, so a subscriber that stops polling cannot grow it
    /// without end.
    #[test]
    fn the_backlog_is_bounded() {
        let mut backlog = Backlog::default();
        for index in 0..RETAINED + 10 {
            backlog.events.push_back(change(&format!("{index}.erg")));
        }
        backlog.trim();

        assert_eq!(backlog.events.len(), RETAINED);
        assert_eq!(backlog.base, 10, "the oldest ten went");
    }

    /// A cursor left behind by a trim reads from what remains rather than
    /// panicking on a position that no longer exists.
    #[test]
    fn a_cursor_the_trim_outran_starts_from_what_remains() {
        let mut backlog = with(&["a.erg"]);
        backlog.read("shaders");

        // A trim that overtook the cursor, as a long silence would produce.
        backlog.events.clear();
        backlog.base = 99;
        backlog.events.push_back(change("late.erg"));

        assert_eq!(paths(&backlog.read("shaders")), ["late.erg"]);
    }

    /// A subscriber that arrives late starts from what is still retained.
    #[test]
    fn a_late_subscriber_starts_from_what_remains() {
        let mut backlog = with(&["a.erg"]);
        backlog.read("shaders");
        backlog.events.push_back(change("c.erg"));

        assert_eq!(
            paths(&backlog.read("editor")),
            ["a.erg", "c.erg"],
            "it was not listening before, but what is retained is fair game"
        );
    }
}
