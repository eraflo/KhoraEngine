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

//! The file system watcher behind hot reload: one root's changes as [`AssetChangeEvent`]s.
//!
//! `notify` delivers the OS events on its own thread, into a bounded [`Channel`]
//! each subscriber reads without blocking.

use anyhow::{Context, Result};
use khora_core::asset::asset_key;
use khora_core::event::{Channel, WhenFull};
use khora_core::sync::LockExt;
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::index_builder::should_skip_file;
use super::{AssetChangeEvent, AssetChangeKind, EventBudget};

/// How many changes are kept for readers that have not caught up.
///
/// A backstop against a subscriber that stops reading, not a tuning knob: a
/// project-wide reformat is hundreds.
const RETAINED: usize = 4096;

/// How many change events a watched root lets through a second.
const EVENTS_PER_SECOND: u32 = 512;

/// A watch of the file changes under one assets root; dropping it stops the watch.
pub struct AssetWatcher {
    // Kept alive for its Drop side-effect (releases the OS handle).
    _watcher: RecommendedWatcher,
    assets_root: PathBuf,
    /// What `notify` has reported, waiting to be read. Bounded, with no queue
    /// in front of it that could grow unread.
    changes: Channel<AssetChangeEvent>,
    /// The root as events name it.
    root: Arc<Path>,
    /// How many events the root lets through; the rescan a dropping burst
    /// calls for is delivered by the next poll after it.
    budget: Arc<Mutex<EventBudget>>,
}

impl AssetWatcher {
    /// Starts watching `assets_root` recursively, its changes read by [`Self::poll_for`].
    ///
    /// # Errors
    /// When `notify` cannot build a watcher or watch the path: it does not
    /// exist, or the OS denies the watch.
    pub fn new(assets_root: impl Into<PathBuf>) -> Result<Self> {
        let assets_root = assets_root.into();
        let changes: Channel<AssetChangeEvent> = Channel::bounded(RETAINED, WhenFull::DropOldest);

        let root_for_handler: Arc<Path> = Arc::from(assets_root.as_path());
        // The root resolved: some systems report a change under the folder's
        // real path rather than the name it was watched by (macOS gives
        // `/private/var/…` for a watch on `/var/…`), and such a change is
        // still under this root.
        let resolved_root: Option<PathBuf> = assets_root
            .canonicalize()
            .ok()
            .filter(|resolved| resolved != &assets_root);
        let sink = changes.clone();
        let budget = Arc::new(Mutex::new(EventBudget::new(EVENTS_PER_SECOND)));
        let budget_for_handler = Arc::clone(&budget);
        let root = Arc::clone(&root_for_handler);
        // One save fires several `Modified` in a row, the file unchanged between
        // them: remembering the previous event and its file stamp collapses the
        // burst. Another save, however soon, leaves another stamp.
        let mut last: Option<(AssetChangeEvent, Option<crate::fs::FileStamp>)> = None;

        let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("notify error: {e}");
                    return;
                }
            };
            for path in &event.paths {
                let Some(change) = translate_event(
                    &event.kind,
                    path,
                    &root_for_handler,
                    resolved_root.as_deref(),
                ) else {
                    continue;
                };
                let stamp = crate::fs::FileStamp::of(path);
                let echo = last
                    .as_ref()
                    .filter(|(_, before)| stamp.is_some() && *before == stamp)
                    .map(|(previous, _)| previous);
                if is_immediate_repeat(echo, &change) {
                    continue;
                }
                last = Some((change.clone(), stamp));
                let admitted = budget_for_handler.lock_recovered().admit(Instant::now());
                if admitted {
                    sink.send(change);
                }
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
            root,
            budget,
        })
    }

    /// Returns the watched assets root.
    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }

    /// Returns every change `subscriber` has not yet seen.
    ///
    /// Each subscriber reads the whole stream from its own position, so one
    /// reader never takes another's changes. A new name starts at the oldest
    /// retained change, so a system registered a frame late still hears it.
    pub fn poll_for(&self, subscriber: &'static str) -> Vec<AssetChangeEvent> {
        let rescan = self.budget.lock_recovered().take_rescan(Instant::now());
        if rescan {
            log::warn!(
                "Too many changes at once under {}: everything under it is read again",
                self.assets_root.display()
            );
            self.changes.send(AssetChangeEvent {
                kind: AssetChangeKind::Rescan,
                root: Arc::clone(&self.root),
                rel_path: String::new(),
            });
        }
        self.changes.read_for(subscriber)
    }

    /// Returns how many changes were dropped because a subscriber did not read them in time.
    pub fn dropped(&self) -> u64 {
        self.changes.dropped()
    }
}

/// Returns whether `next` says nothing the event just before it did not.
///
/// Only `Modified` collapses: one save fires several in a row. Two `Created`
/// or `Removed` in a row are two facts a consumer reindexes on.
fn is_immediate_repeat(previous: Option<&AssetChangeEvent>, next: &AssetChangeEvent) -> bool {
    previous.is_some_and(|previous| {
        previous.kind == AssetChangeKind::Modified
            && next.kind == AssetChangeKind::Modified
            && previous.rel_path == next.rel_path
    })
}

/// Returns the change a `notify` event says of the absolute path `abs`, if any.
///
/// `None` for an access or other uninteresting kind, a skipped scratch file, or
/// a path under neither the root's name nor its resolved path.
fn translate_event(
    kind: &notify::EventKind,
    abs: &Path,
    assets_root: &Arc<Path>,
    resolved_root: Option<&Path>,
) -> Option<AssetChangeEvent> {
    use notify::event::{ModifyKind, RenameMode};
    let our_kind = match kind {
        notify::EventKind::Create(_) => AssetChangeKind::Created,
        notify::EventKind::Remove(_) => AssetChangeKind::Removed,
        // A rename: where the file was is removed, where it went is created.
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::From)) => AssetChangeKind::Removed,
        notify::EventKind::Modify(ModifyKind::Name(RenameMode::To)) => AssetChangeKind::Created,
        // A rename without its side (macOS `Any`, Windows `Both`) is read from
        // the disk: the path still there is where the file went.
        notify::EventKind::Modify(ModifyKind::Name(_)) => {
            if abs.exists() {
                AssetChangeKind::Created
            } else {
                AssetChangeKind::Removed
            }
        }
        notify::EventKind::Modify(_) => AssetChangeKind::Modified,
        // Access / Any / Other: not interesting for hot-reload.
        _ => return None,
    };

    // OS scratch and editor swap files are skipped; the VFS tracks every
    // other extension.
    let file_name = abs.file_name().and_then(|n| n.to_str())?;
    if should_skip_file(file_name) {
        return None;
    }

    let rel = abs
        .strip_prefix(assets_root)
        .ok()
        .or_else(|| resolved_root.and_then(|resolved| abs.strip_prefix(resolved).ok()))?;
    // What a folder moved aside holds is under another name for a while.
    let in_temporary = rel.parent().is_some_and(|folder| {
        (folder.components())
            .any(|part| crate::fs::is_temporary(&part.as_os_str().to_string_lossy()))
    });
    if in_temporary {
        return None;
    }
    Some(AssetChangeEvent {
        kind: our_kind,
        root: Arc::clone(assets_root),
        rel_path: asset_key(rel),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, thread::sleep, time::Duration};
    use tempfile::tempdir;

    /// How long the watcher's backend thread has to deliver an event.
    ///
    /// `notify` is timing-dependent, so the test using it is ignored by default.
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

    mod repeat_tests {
        use super::super::*;

        fn event(kind: AssetChangeKind, path: &str) -> AssetChangeEvent {
            AssetChangeEvent {
                kind,
                root: Arc::from(Path::new("assets")),
                rel_path: path.to_owned(),
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

        /// A change under the root's real path (macOS `/private/var/…`) keeps the root's name.
        #[test]
        fn a_change_reported_under_the_real_path_is_under_the_root() {
            let named: Arc<Path> = Arc::from(Path::new("/var/project/assets"));
            let real = Path::new("/private/var/project/assets");
            let change = translate_event(
                &notify::EventKind::Create(notify::event::CreateKind::File),
                &real.join("scripts").join("guard.erg"),
                &named,
                Some(real),
            )
            .expect("a change under the root");

            assert_eq!(change.rel_path, "scripts/guard.erg");
            assert_eq!(change.root(), &*named);
        }

        /// A rename without its side is read from the disk: the path that exists is where it went.
        #[test]
        fn a_rename_without_its_side_is_read_from_the_disk() {
            use notify::event::{ModifyKind, RenameMode};
            let dir = tempfile::tempdir().expect("a temp dir");
            let root: Arc<Path> = Arc::from(dir.path());
            std::fs::write(dir.path().join("there.erg"), "behavior B {}").expect("writes");
            let rename = notify::EventKind::Modify(ModifyKind::Name(RenameMode::Any));

            let arrived = translate_event(&rename, &dir.path().join("there.erg"), &root, None)
                .expect("a change under the root");
            let left = translate_event(&rename, &dir.path().join("gone.erg"), &root, None)
                .expect("a change under the root");

            assert_eq!(arrived.kind, AssetChangeKind::Created);
            assert_eq!(left.kind, AssetChangeKind::Removed);
        }

        /// Under neither name, a path is not a change of this root.
        #[test]
        fn a_path_under_neither_name_is_not_a_change() {
            let named: Arc<Path> = Arc::from(Path::new("/var/project/assets"));
            let change = translate_event(
                &notify::EventKind::Create(notify::event::CreateKind::File),
                Path::new("/elsewhere/guard.erg"),
                &named,
                Some(Path::new("/private/var/project/assets")),
            );

            assert!(change.is_none());
        }

        /// A file in a folder moved aside under a temporary name is not a change: it is under another name for a while.
        #[test]
        fn a_change_inside_a_folder_moved_aside_is_not_a_change() {
            let root: Arc<Path> = Arc::from(Path::new("/var/project/assets"));
            let create = notify::EventKind::Create(notify::event::CreateKind::File);
            let aside = root.join("Tex.12.3.aside.tmp").join("a.png");

            assert!(translate_event(&create, &aside, &root, None).is_none());
            assert!(
                translate_event(&create, &root.join("Tex").join("a.png"), &root, None).is_some()
            );
        }

        #[test]
        fn the_first_event_is_never_a_repeat() {
            let first = event(AssetChangeKind::Modified, "scripts/guard.erg");

            assert!(!is_immediate_repeat(None, &first));
        }
    }
}
