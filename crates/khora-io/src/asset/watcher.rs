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
//! convention "no `std::thread::spawn` in user code" is respected. That thread
//! writes straight into a bounded [`Channel`], and the consumer side is
//! non-blocking via [`AssetWatcher::poll_for`].

use anyhow::{Context, Result};
use khora_core::asset::AssetUUID;
use khora_core::event::{Channel, Supersedes, WhenFull};
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

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
    assets_root: PathBuf,
    /// What `notify` has reported, waiting to be read.
    ///
    /// The watcher used to hold a `crossbeam` channel in front of a hand-rolled
    /// backlog: a bounded window fed by an **unbounded** queue, which grew
    /// forever if the backlog's lock was ever poisoned — `poll_for` returned
    /// before draining it. One bounded channel has no in-front.
    changes: Channel<AssetChangeEvent>,
}

/// How many changes are kept for readers that have not caught up.
///
/// Far more than a frame produces — a project-wide reformat is hundreds — and
/// small enough that the memory is irrelevant. A backstop against a subscriber
/// that stops reading, not a tuning knob.
const RETAINED: usize = 4096;

impl Supersedes for AssetChangeEvent {
    // **Deliberately not coalescing**, even though repeated `Modified` on one
    // path is exactly what a single save produces on Windows. Coalescing in the
    // channel is a scan of everything queued, run on `notify`'s own thread; a
    // branch switch reporting thousands of *distinct* paths would make that
    // quadratic to discover that nothing matches. The burst from one save
    // arrives consecutively, so the constructor drops an immediate repeat in
    // `O(1)` instead, and consumers that care de-duplicate what they read —
    // which both hot-reload pumps and the editor already do.
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
        let changes: Channel<AssetChangeEvent> = Channel::bounded(RETAINED, WhenFull::DropOldest);

        let root_for_handler = assets_root.clone();
        let sink = changes.clone();
        // One save fires several `Modified` for the same path. Remembering only
        // the previous one collapses that burst — they arrive consecutively —
        // without scanning what is already queued.
        let mut last: Option<AssetChangeEvent> = None;

        let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("notify error: {e}");
                    return;
                }
            };
            for path in &event.paths {
                let Some(change) = translate_event(&event.kind, path, &root_for_handler) else {
                    continue;
                };
                if is_immediate_repeat(last.as_ref(), &change) {
                    continue;
                }
                last = Some(change.clone());
                sink.send(change);
            }
        })
        .context("Failed to create filesystem watcher")?;

        watcher
            .watch(&assets_root, RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch {}", assets_root.display()))?;

        Ok(Self {
            _watcher: watcher,
            assets_root,
            changes,
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
    /// A name seen for the first time starts at the oldest retained change, so
    /// a system registered a frame late still hears what happened.
    pub fn poll_for(&self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        self.changes.read_for(subscriber)
    }

    /// How many changes were dropped because nobody read them in time.
    ///
    /// Readable rather than silent: a subscriber that stopped polling makes the
    /// window slide past it, and this is what says so.
    pub fn dropped(&self) -> u64 {
        self.changes.dropped()
    }
}

/// Whether `next` says nothing the event just before it did not.
///
/// One save fires several `Modified` for the same path — `notify` v6 does this
/// on Windows in particular — and they arrive consecutively, so remembering one
/// event collapses the burst without scanning what is already queued.
///
/// Only `Modified` collapses. A `Created` or a `Removed` is a fact about the
/// tree's shape that a consumer reindexes on, and two of them in a row mean two
/// different things.
fn is_immediate_repeat(previous: Option<&AssetChangeEvent>, next: &AssetChangeEvent) -> bool {
    previous.is_some_and(|previous| {
        previous.kind == AssetChangeKind::Modified
            && next.kind == AssetChangeKind::Modified
            && previous.rel_path == next.rel_path
    })
}

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
mod repeat_tests {
    use super::*;

    fn event(kind: AssetChangeKind, path: &str) -> AssetChangeEvent {
        AssetChangeEvent {
            kind,
            rel_path: path.to_owned(),
            uuid: AssetUUID::new_v5(path),
        }
    }

    #[test]
    fn a_save_that_fires_twice_is_reported_once() {
        let first = event(AssetChangeKind::Modified, "scripts/guard.erg");
        let again = event(AssetChangeKind::Modified, "scripts/guard.erg");

        assert!(is_immediate_repeat(Some(&first), &again));
    }

    #[test]
    fn a_different_path_is_not_a_repeat() {
        let first = event(AssetChangeKind::Modified, "scripts/guard.erg");
        let other = event(AssetChangeKind::Modified, "scripts/chest.erg");

        assert!(!is_immediate_repeat(Some(&first), &other));
    }

    /// Two `Created` in a row are two files appearing, and a consumer reindexes
    /// on each — collapsing them would hide one.
    #[test]
    fn only_modified_collapses() {
        let first = event(AssetChangeKind::Created, "scripts/guard.erg");
        let again = event(AssetChangeKind::Created, "scripts/guard.erg");

        assert!(!is_immediate_repeat(Some(&first), &again));
    }

    /// A save right after the file appeared is a real second fact: the
    /// consumer that reindexed on the create still has to reload the contents.
    #[test]
    fn a_save_after_a_create_survives() {
        let created = event(AssetChangeKind::Created, "scripts/guard.erg");
        let saved = event(AssetChangeKind::Modified, "scripts/guard.erg");

        assert!(!is_immediate_repeat(Some(&created), &saved));
    }

    #[test]
    fn the_first_event_is_never_a_repeat() {
        let first = event(AssetChangeKind::Modified, "scripts/guard.erg");

        assert!(!is_immediate_repeat(None, &first));
    }
}
