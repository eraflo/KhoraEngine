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
//! The editor's frame loop calls [`AssetWatcher::poll`] each frame to drain
//! pending events and feed them into `AssetService::invalidate` /
//! `reindex` — see the editor's `before_agents` hot-reload pump.
//!
//! # Threading
//!
//! `notify` v6 spawns its own internal backend thread for the OS-level event
//! source (this is unavoidable — that's how kernel APIs deliver events). We
//! never call `std::thread::spawn` from this crate, so the workspace
//! convention "no `std::thread::spawn` in user code" is respected. The
//! crossbeam channel is bounded only by the internal handler closure; the
//! consumer side is non-blocking via [`AssetWatcher::poll`].

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use khora_core::asset::AssetUUID;
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
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
}

impl AssetWatcher {
    /// Starts watching `assets_root` recursively. Future writes / creates /
    /// removes under that tree produce events drained via [`Self::poll`].
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
        })
    }

    /// Returns the watched assets root.
    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }

    /// Drains all pending events without blocking.
    ///
    /// Coalesces repeated `Modified` events on the same path within the same
    /// drain — `notify` v6 fires multiple Modified for one save on Windows.
    /// The order of distinct events is preserved for events on different
    /// paths.
    pub fn poll(&self) -> Vec<AssetChangeEvent> {
        let mut out: Vec<AssetChangeEvent> = Vec::new();
        let mut seen_modified: HashSet<String> = HashSet::new();
        while let Ok(event) = self.receiver.try_recv() {
            if matches!(event.kind, AssetChangeKind::Modified)
                && !seen_modified.insert(event.rel_path.clone())
            {
                // Already emitted a Modified for this path in this drain.
                continue;
            }
            out.push(event);
        }
        out
    }
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
            events.extend(watcher.poll());
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
