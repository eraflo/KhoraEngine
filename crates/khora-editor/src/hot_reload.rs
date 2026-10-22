// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project hot-reload pump.
//!
//! Drains pending filesystem-change events from the project watcher,
//! invalidates the `AssetService` cache for modified UUIDs, and reindexes
//! when files are added or removed. Run once per frame, before the agents
//! see the world, so a coherent VFS is in scope for the rest of the tick.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::EditorState;
use khora_sdk::{AssetChangeKind, AssetSource};

use crate::project_vfs::ProjectVfs;

/// Drain the project's filesystem watcher and apply the appropriate
/// invalidation / reindex actions. The asset browser cache is rebuilt
/// whenever a full reindex was queued.
pub fn pump(pvfs_mutex: &Arc<Mutex<ProjectVfs>>, editor_state: &Arc<Mutex<EditorState>>) {
    let Ok(mut pvfs) = pvfs_mutex.lock() else {
        return;
    };
    let events = pvfs.poll_changes();
    if events.is_empty() {
        return;
    }

    // Coalesce Modified by path (one save fires a flurry); Created/Removed
    // trigger a full reindex.
    let mut needs_reindex = false;
    let mut modified: HashSet<String> = HashSet::new();
    for e in events {
        match e.kind {
            AssetChangeKind::Modified => {
                modified.insert(e.rel_path);
            }
            AssetChangeKind::Created | AssetChangeKind::Removed => {
                // A file replaced in place (removed, then created) must not
                // keep serving what was decoded from its former bytes.
                modified.insert(e.rel_path.clone());
                log::info!(
                    "Hot reload: {:?} '{}' — full reindex queued",
                    e.kind,
                    e.rel_path
                );
                needs_reindex = true;
            }
        }
    }

    // Resolve the UUID through the identity registry (not the event's raw
    // path-derived value) so a modified *renamed* asset invalidates the right
    // cache entry.
    for rel in &modified {
        let uuid = pvfs.resolve_uuid(rel);
        // A file that can hold parts may have gained or lost some (a
        // re-exported glTF with another node count, or one whole again after
        // a broken save): its parts are listed again.
        needs_reindex |= pvfs.asset_service.may_hold_sub_assets(&uuid);
        let dropped = pvfs.asset_service.invalidate(&uuid);
        log::info!(
            "Hot reload: Modified '{}' (cache dropped: {})",
            rel,
            dropped
        );
    }

    if !needs_reindex {
        return;
    }
    if let Err(e) = pvfs.rebuild_index() {
        log::error!("Hot reload: failed to rebuild index: {:#}", e);
        return;
    }
    let entries = collect_asset_entries(&pvfs);
    let dirs = pvfs.list_dirs();
    if let Ok(mut state) = editor_state.lock() {
        state.asset_entries = entries;
        state.asset_dirs = dirs;
        state.asset_epoch = state.asset_epoch.wrapping_add(1);
    }
}

/// Build an `AssetEntry` cache from the current VFS contents. Used by the
/// initial project open and when the user browses to a different folder
/// at runtime.
pub fn collect_asset_entries(pvfs: &ProjectVfs) -> Vec<AssetEntry> {
    pvfs.asset_service
        .index()
        .iter_all()
        .map(|m| {
            // Forward slashes whatever the platform: the browser's folders,
            // the identity registry and the project VFS all name an asset so.
            let rel_str = m.source_path.to_string_lossy().replace('\\', "/");
            // A part of a file is named by its label inside the file; the
            // file is named by its file name.
            let (name, container) = match m.variants.get("default") {
                Some(AssetSource::Sub { container, label }) => (label.clone(), Some(*container)),
                _ => (
                    m.source_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| rel_str.clone()),
                    None,
                ),
            };
            AssetEntry {
                name,
                asset_type: m.asset_type_name.clone(),
                source_path: rel_str,
                uuid: m.uuid,
                container,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_sdk::khora_core::asset::AssetUUID;
    use khora_sdk::{AssetIdRegistry, MetricsRegistry};

    use super::collect_asset_entries;
    use crate::project_vfs::ProjectVfs;

    /// **Each entry carries the identity the index gave its asset** — the one
    /// the registry froze for a renamed file, the path's own for the others:
    /// what a reference to the asset stores.
    #[test]
    fn each_asset_entry_carries_the_identity_the_index_resolved() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-asset-entry-uuid-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let prefabs = root.join("assets").join("prefabs");
        std::fs::create_dir_all(&prefabs)?;
        std::fs::write(prefabs.join("bullet.kprefab"), b"prefab bytes")?;
        std::fs::write(prefabs.join("rocket.kprefab"), b"prefab bytes")?;

        // `rocket` was renamed from `missile`: it keeps the identity it had.
        let frozen = AssetUUID::new_v5("prefabs/missile.kprefab");
        let mut registry = AssetIdRegistry::load(&root);
        registry.freeze("prefabs/rocket.kprefab", frozen);
        registry.save()?;

        let entries = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .map(|pvfs| collect_asset_entries(&pvfs));
        let _ = std::fs::remove_dir_all(&root);
        let entries = entries?;

        let uuid_of = |path: &str| {
            entries
                .iter()
                .find(|entry| entry.source_path.replace('\\', "/") == path)
                .map(|entry| entry.uuid)
        };
        assert_eq!(
            uuid_of("prefabs/bullet.kprefab"),
            Some(AssetUUID::new_v5("prefabs/bullet.kprefab")),
            "an asset never renamed is named by its path: {entries:?}"
        );
        assert_eq!(
            uuid_of("prefabs/rocket.kprefab"),
            Some(frozen),
            "a renamed asset keeps the identity the registry froze: {entries:?}"
        );
        Ok(())
    }

    const SHIP: &str = "models/ship.gltf";
    const CRATE: &str = "models/crate.obj";

    /// **Each part of a file is listed as an entry of its own, marked with
    /// its file**: named by its label, typed by its kind, found at its
    /// file's path. The file and a mesh file with no parts are marked with
    /// none.
    #[test]
    fn each_part_of_a_file_is_an_entry_marked_with_its_file() {
        use crate::gltf_fixture::{GltfProject, SHIP_GLTF, TRIANGLE_OBJ};

        let project =
            GltfProject::with("entries-parts", &[(SHIP, SHIP_GLTF), (CRATE, TRIANGLE_OBJ)]);
        let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
        let ship = project.uuid(SHIP);
        let of = |uuid: AssetUUID| {
            entries
                .iter()
                .find(|entry| entry.uuid == uuid)
                .unwrap_or_else(|| panic!("no entry holds {uuid:?}: {entries:?}"))
        };

        let file = of(ship);
        assert_eq!(file.container, None, "the file is no part: {file:?}");
        assert_eq!(file.name, "ship.gltf");
        let obj = of(project.uuid(CRATE));
        assert_eq!(obj.container, None, "the OBJ is no part: {obj:?}");

        for (label, kind) in [
            ("scene", "prefab"),
            ("scene0", "prefab"),
            ("mesh0/prim0", "mesh"),
            ("mesh1/prim0", "mesh"),
            ("material0", "material"),
            ("material1", "material"),
            ("texture0", "texture"),
        ] {
            let part = of(AssetUUID::sub_asset(ship, label));
            assert_eq!(part.container, Some(ship), "{label} is a part of the file");
            assert_eq!(part.name, label, "a part is named by its label");
            assert_eq!(part.asset_type, kind, "{label}'s kind");
            assert_eq!(
                part.source_path, SHIP,
                "{label} is found at its file's path"
            );
        }
        let parts = entries
            .iter()
            .filter(|entry| entry.container == Some(ship))
            .count();
        assert_eq!(parts, 7, "every part of the file, once: {entries:?}");
        assert!(
            entries
                .iter()
                .all(|entry| entry.container != Some(project.uuid(CRATE))),
            "the OBJ has no parts"
        );
    }

    /// **A glTF saved over with another node lists its new parts once the
    /// pump has run**: its list of parts changed with its contents, though
    /// the file was only modified — not created or removed.
    #[test]
    fn a_modified_gltf_lists_its_new_parts_after_the_pump() {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        use khora_sdk::EditorState;

        use super::pump;
        use crate::gltf_fixture::{GltfProject, ONE_NODE_GLTF, SHIP_GLTF};

        let project = GltfProject::with("hot-reload-parts", &[(SHIP, ONE_NODE_GLTF)]);
        let ship = project.uuid(SHIP);
        let mast = AssetUUID::sub_asset(ship, "mesh1/prim0");
        let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
        assert!(
            !entries.iter().any(|entry| entry.uuid == mast),
            "the premise: one node, no second mesh: {entries:?}"
        );
        let state = Arc::new(Mutex::new(EditorState {
            project_folder: Some(project.root.to_string_lossy().into_owned()),
            asset_entries: entries,
            asset_epoch: 1,
            ..EditorState::default()
        }));
        // The watcher was armed when the project opened; give it a moment.
        std::thread::sleep(Duration::from_millis(150));

        std::fs::write(project.path(SHIP), SHIP_GLTF).expect("the file is saved over");
        let listed = |state: &Arc<Mutex<EditorState>>| {
            state
                .lock()
                .expect("the editor state")
                .asset_entries
                .iter()
                .any(|entry| entry.uuid == mast)
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !listed(&state) {
            pump(&project.vfs, &state);
            std::thread::sleep(Duration::from_millis(50));
        }

        let state_now = state.lock().expect("the editor state");
        assert!(
            state_now
                .asset_entries
                .iter()
                .any(|entry| entry.uuid == mast),
            "the second node's mesh is listed: {:?}",
            state_now
                .asset_entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_ne!(state_now.asset_epoch, 1, "the browser is told to rescan");
    }

    /// **A glTF that could not be read when it was indexed lists its parts
    /// once it is saved whole**: an exporter caught mid-write leaves a file
    /// the index lists alone, without parts; the save that completes it is
    /// a modification, and its parts must appear all the same.
    #[test]
    fn a_gltf_saved_whole_after_a_broken_write_lists_its_parts_after_the_pump() {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        use khora_sdk::EditorState;

        use super::pump;
        use crate::gltf_fixture::{GltfProject, SHIP_GLTF};

        let half_written = &SHIP_GLTF[..SHIP_GLTF.len() / 2];
        let project = GltfProject::with("hot-reload-broken-then-whole", &[(SHIP, half_written)]);
        let ship = project.uuid(SHIP);
        let scene = AssetUUID::sub_asset(ship, "scene");
        let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
        assert!(
            entries.iter().any(|entry| entry.uuid == ship),
            "the premise: the broken file is listed: {entries:?}"
        );
        assert!(
            !entries.iter().any(|entry| entry.container == Some(ship)),
            "the premise: the broken file has no parts: {entries:?}"
        );
        let state = Arc::new(Mutex::new(EditorState {
            project_folder: Some(project.root.to_string_lossy().into_owned()),
            asset_entries: entries,
            asset_epoch: 1,
            ..EditorState::default()
        }));
        // The watcher was armed when the project opened; give it a moment.
        std::thread::sleep(Duration::from_millis(150));

        std::fs::write(project.path(SHIP), SHIP_GLTF).expect("the file is saved whole");
        let listed = |state: &Arc<Mutex<EditorState>>| {
            state
                .lock()
                .expect("the editor state")
                .asset_entries
                .iter()
                .any(|entry| entry.uuid == scene)
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && !listed(&state) {
            pump(&project.vfs, &state);
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            listed(&state),
            "the glTF's scene is listed once the file is whole: {:?}",
            state
                .lock()
                .expect("the editor state")
                .asset_entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>()
        );
    }
}
