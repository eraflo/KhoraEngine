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

use crate::project_vfs::{AfterIndex, ProjectVfs};

/// Drain the project's filesystem watcher and apply the appropriate
/// invalidation / reindex actions. A full reindex is filed with the
/// engine's index hub the project holds ([`ProjectVfs::reindex`]), its
/// ticket mirrored in the editor state, for [`land_reindex`] to read; with
/// no hub, it is done on the frame and the asset browser cache rebuilt at
/// once.
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
        needs_reindex |= pvfs.assets().may_hold_sub_assets(&uuid);
        let dropped = pvfs.assets().invalidate(&uuid);
        log::info!(
            "Hot reload: Modified '{}' (cache dropped: {})",
            rel,
            dropped
        );
    }

    if !needs_reindex {
        return;
    }
    if let Err(e) = pvfs.reindex() {
        log::error!("Hot reload: failed to rebuild index: {:#}", e);
        return;
    }
    if let Some(ticket) = pvfs.reindexing() {
        // Off the frame: the browser is refreshed when it lands.
        if let Ok(mut state) = editor_state.lock() {
            state.reindexing = Some(ticket);
        }
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

/// Reads the outcome of the project's reindex in flight, once it landed
/// ([`ProjectVfs::land`]): the asset browser cache is rebuilt, the actions
/// that waited on the editor state run, and a failure is reported. Returns
/// the actions that need the world ([`AfterIndex::AssignMaterial`]), for
/// the caller to apply. Nothing while it is in flight, or when none was
/// filed.
pub fn land_reindex(
    pvfs_mutex: &Arc<Mutex<ProjectVfs>>,
    editor_state: &Arc<Mutex<EditorState>>,
) -> Vec<AfterIndex> {
    let Ok(mut pvfs) = pvfs_mutex.lock() else {
        return Vec::new();
    };
    let (editing, playing, choice, asked) =
        editor_state
            .lock()
            .map_or((false, false, 0, true), |state| {
                (
                    state.play_mode == khora_sdk::PlayMode::Editing && state.restoring.is_none(),
                    state.play_mode != khora_sdk::PlayMode::Editing,
                    state.scene_choices,
                    state.pending_scene_load.is_some(),
                )
            });
    let landed = pvfs.land();
    let in_flight = pvfs.reindexing();
    let Some(landed) = landed else {
        // What waited for the edited scene to come back is assigned now.
        return if editing {
            assignable(pvfs.release(), &mut pvfs, editing, playing, choice)
        } else {
            Vec::new()
        };
    };
    if let Err(why) = &landed.outcome {
        log::error!("Failed to rebuild the project's index: {why}");
    }
    let entries = collect_asset_entries(&pvfs);
    let dirs = pvfs.list_dirs();
    let mut with_world = Vec::new();
    let mut reveal = None;
    let mut open = None;
    for action in landed.actions {
        match action {
            AfterIndex::Reveal(rel) => reveal = Some(rel),
            AfterIndex::OpenScene {
                rel,
                choice: chosen,
            } => {
                // Another scene chosen since, or asked for now, has the last
                // word.
                if chosen != choice || asked {
                    continue;
                }
                let uuid = pvfs.resolve_uuid(&rel);
                if pvfs.assets().reader().index().get_metadata(&uuid).is_some() {
                    open = Some(pvfs.assets_root.join(&rel).to_string_lossy().into_owned());
                } else {
                    log::error!("The project holds no scene '{rel}'");
                }
            }
            action @ AfterIndex::AssignMaterial { .. } => with_world.push(action),
        }
    }
    if editing {
        let mut released = pvfs.release();
        released.append(&mut with_world);
        with_world = released;
    }
    let with_world = assignable(with_world, &mut pvfs, editing, playing, choice);
    drop(pvfs);
    if let Ok(mut state) = editor_state.lock() {
        state.reindexing = in_flight;
        state.asset_entries = entries;
        state.asset_dirs = dirs;
        state.asset_epoch = state.asset_epoch.wrapping_add(1);
        if reveal.is_some() {
            state.pending_reveal_asset = reveal;
        }
        if open.is_some() {
            state.pending_scene_load = open;
        }
    }
    with_world
}

/// Of the material assignments `actions` that landed, those to apply now.
///
/// One for a scene chosen before the open one is dropped. One saved while the
/// scene was played is for the played scene: applied while it plays, dropped
/// once it stopped. One saved while editing is for the edited scene: applied
/// while editing, kept in `pvfs` while the scene is played or restored, else
/// the restore at Stop would undo it.
fn assignable(
    actions: Vec<AfterIndex>,
    pvfs: &mut ProjectVfs,
    editing: bool,
    playing: bool,
    choice: u64,
) -> Vec<AfterIndex> {
    let mut now = Vec::new();
    let mut kept = Vec::new();
    for action in actions {
        let AfterIndex::AssignMaterial {
            choice: chosen,
            played,
            ..
        } = &action
        else {
            continue;
        };
        if *chosen != choice {
            continue;
        }
        match (*played, editing) {
            (true, _) if playing => now.push(action),
            (true, _) => {}
            (false, true) => now.push(action),
            (false, false) => kept.push(action),
        }
    }
    pvfs.hold(kept);
    now
}

/// Build an `AssetEntry` cache from the current VFS contents. Used by the
/// initial project open and when the user browses to a different folder
/// at runtime.
pub fn collect_asset_entries(pvfs: &ProjectVfs) -> Vec<AssetEntry> {
    pvfs.assets()
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
        use crate::tests::gltf_fixture::{GltfProject, SHIP_GLTF, TRIANGLE_OBJ};

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
        use crate::tests::gltf_fixture::{GltfProject, ONE_NODE_GLTF, SHIP_GLTF};

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
        use crate::tests::gltf_fixture::{GltfProject, SHIP_GLTF};

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

    /// The engine's side of one frame, for a project index: the `loading`
    /// family indexes what was filed, each result staged under its lease and
    /// received by the reserve, as the next frame start receives it; then
    /// the boundary's `asset_intake` and `asset_index_intake` take it in.
    fn engine_frame(runtime: &khora_sdk::Runtime) {
        use khora_sdk::khora_core::asset::{AssetIndexing, IndexRequest};
        use khora_sdk::khora_core::event::Channel;
        use khora_sdk::khora_core::lane::{OutputDeck, Reserve, Staging};
        use khora_sdk::khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, World};

        let requests = runtime
            .resources
            .get::<Channel<IndexRequest>>()
            .cloned()
            .expect("the runtime holds the index requests");
        let indexing = runtime
            .resources
            .get::<Arc<dyn AssetIndexing>>()
            .cloned()
            .expect("the runtime holds the indexer");
        let reserve = runtime
            .resources
            .get::<Reserve>()
            .cloned()
            .expect("the runtime holds the reserve");
        let mut staging = Staging::new();
        for request in requests.drain() {
            let (indexed, lease) = indexing.index_leased(&request);
            match lease {
                Some(lease) => staging.stage_while(vec![indexed], lease),
                None => staging.stage(vec![indexed]),
            }
        }
        reserve.receive(staging);

        let mut world = World::new();
        for name in ["asset_intake", "asset_index_intake"] {
            let system = khora_sdk::inventory::iter::<DataSystemRegistration>
                .into_iter()
                .find(|system| system.name == name)
                .unwrap_or_else(|| panic!("`{name}` is not registered"));
            (system.run)(
                &mut world,
                runtime,
                &mut OutputDeck::new(),
                SystemContext {
                    since: Tick(0),
                    resumed: false,
                    scope: None,
                },
            );
        }
    }

    /// **A file created is indexed off the frame.** The project serves the
    /// engine's asset service, and the engine holds its index hub. A file is
    /// created: the pump files a reindex with the hub and keeps its ticket,
    /// without reindexing on the frame — the service does not know the file,
    /// the browser's entries and epoch are unchanged, the project's
    /// generation has not moved. Once the engine's frames index it and land
    /// it, the editor reads the ticket's outcome: the entries list the file,
    /// the epoch tells the browser to rescan, and the project's generation
    /// moved.
    #[test]
    fn a_file_created_is_indexed_off_the_frame() {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        use khora_sdk::khora_core::lane::Reserve;
        use khora_sdk::{install_indexing, EditorState, IndexHub, Runtime};

        use super::{land_reindex, pump};
        use crate::tests::gltf_fixture::{GltfProject, TRIANGLE_OBJ};

        const CRATE: &str = "meshes/crate.obj";
        const SHIP: &str = "meshes/ship.obj";

        let project = GltfProject::with("index-off-the-frame", &[(CRATE, TRIANGLE_OBJ)]);
        // The engine: the asset service the project moves into, the reserve
        // the background stages into, and the index hub.
        let shared = Arc::new(Mutex::new(khora_sdk::AssetService::empty()));
        project
            .vfs
            .lock()
            .expect("the project")
            .share_assets(&shared);
        let mut runtime = Runtime::new();
        runtime.resources.insert(Reserve::new());
        runtime.services.insert(Arc::clone(&shared));
        install_indexing(&mut runtime);
        let hub = runtime
            .resources
            .get::<Arc<IndexHub>>()
            .cloned()
            .expect("`install_indexing` installs the hub");
        // The project files its reindexes with the engine's hub, as
        // `before_agents` arranges.
        project
            .vfs
            .lock()
            .expect("the project")
            .index_with(Arc::clone(&hub));

        let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
        let state = Arc::new(Mutex::new(EditorState {
            project_folder: Some(project.root.to_string_lossy().into_owned()),
            asset_entries: entries,
            asset_epoch: 1,
            ..EditorState::default()
        }));
        let generation = project.vfs.lock().expect("the project").generation();
        // The watcher was armed when the project opened; give it a moment.
        std::thread::sleep(Duration::from_millis(150));

        std::fs::write(project.path(SHIP), TRIANGLE_OBJ).expect("the file is created");
        let added = project.uuid(SHIP);
        let filed =
            |state: &Arc<Mutex<EditorState>>| state.lock().expect("the editor state").reindexing;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && filed(&state).is_none() {
            pump(&project.vfs, &state);
            std::thread::sleep(Duration::from_millis(50));
        }

        let ticket = filed(&state).expect("the pump filed a reindex and kept its ticket");
        assert!(hub.is_pending(ticket), "the reindex is in flight");
        {
            let state_now = state.lock().expect("the editor state");
            assert_eq!(state_now.asset_epoch, 1, "the browser is not told yet");
            assert!(
                !state_now
                    .asset_entries
                    .iter()
                    .any(|entry| entry.uuid == added),
                "the entries are not refreshed on the frame"
            );
        }
        assert!(
            shared
                .lock()
                .expect("the service")
                .index()
                .get_metadata(&added)
                .is_none(),
            "the pump did not reindex on the frame"
        );
        assert_eq!(
            project.vfs.lock().expect("the project").generation(),
            generation,
            "nothing was reindexed yet"
        );

        // Read before the engine's frames: still in flight, nothing changes.
        land_reindex(&project.vfs, &state);
        assert_eq!(filed(&state), Some(ticket), "still waiting for its outcome");

        for _ in 0..crate::tests::scene_loads::SETTLE_FRAMES {
            engine_frame(&runtime);
            land_reindex(&project.vfs, &state);
            if filed(&state).is_none() {
                break;
            }
        }

        let state_now = state.lock().expect("the editor state");
        assert!(
            state_now.reindexing.is_none(),
            "the ticket's outcome was read"
        );
        assert!(
            state_now
                .asset_entries
                .iter()
                .any(|entry| entry.uuid == added),
            "the entries list the new file: {:?}",
            state_now
                .asset_entries
                .iter()
                .map(|entry| entry.source_path.as_str())
                .collect::<Vec<_>>()
        );
        assert_ne!(state_now.asset_epoch, 1, "the browser is told to rescan");
        drop(state_now);
        assert!(
            project.vfs.lock().expect("the project").generation() > generation,
            "the project's generation moved"
        );
    }

    /// **A reindex filed for a project closed since lands nothing in the one
    /// opened.** The engine serves project A; a file is created in A and the
    /// pump files a reindex of A. Before it lands, the user opens project B,
    /// whose service moves into the engine's (`share_assets`, as
    /// `before_agents` does on the frame a project opens). When A's reindex
    /// lands, the engine's service must still index B's files and none of
    /// A's, and the browser must list B's files, not A's.
    #[test]
    fn a_reindex_filed_for_a_project_closed_since_lands_nothing_in_the_next() {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        use khora_sdk::khora_core::lane::Reserve;
        use khora_sdk::{install_indexing, EditorState, IndexHub, Runtime};

        use super::{land_reindex, pump};
        use crate::tests::gltf_fixture::{GltfProject, TRIANGLE_OBJ};

        const CRATE: &str = "meshes/crate.obj";
        const SHIP: &str = "meshes/ship.obj";
        const BARREL: &str = "meshes/barrel.obj";

        let a = GltfProject::with("reindex-closed-a", &[(CRATE, TRIANGLE_OBJ)]);
        let b = GltfProject::with("reindex-closed-b", &[(BARREL, TRIANGLE_OBJ)]);
        let shared = Arc::new(Mutex::new(khora_sdk::AssetService::empty()));
        a.vfs.lock().expect("project A").share_assets(&shared);
        let mut runtime = Runtime::new();
        runtime.resources.insert(Reserve::new());
        runtime.services.insert(Arc::clone(&shared));
        install_indexing(&mut runtime);
        let hub = runtime
            .resources
            .get::<Arc<IndexHub>>()
            .cloned()
            .expect("`install_indexing` installs the hub");
        // A files its reindexes with the engine's hub, as `before_agents`
        // arranges for the open project.
        a.vfs
            .lock()
            .expect("project A")
            .index_with(Arc::clone(&hub));

        let state = Arc::new(Mutex::new(EditorState {
            project_folder: Some(a.root.to_string_lossy().into_owned()),
            asset_entries: collect_asset_entries(&a.vfs.lock().expect("project A")),
            asset_epoch: 1,
            ..EditorState::default()
        }));
        std::thread::sleep(Duration::from_millis(150));

        // A file is created in A: the pump files a reindex of A.
        std::fs::write(a.path(SHIP), TRIANGLE_OBJ).expect("the file is created");
        let filed =
            |state: &Arc<Mutex<EditorState>>| state.lock().expect("the editor state").reindexing;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && filed(&state).is_none() {
            pump(&a.vfs, &state);
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            filed(&state).is_some(),
            "the premise: the pump filed a reindex of A"
        );

        // The user opens B before it lands: B's service moves into the
        // engine's, and B is the project the editor reads from now on.
        {
            let mut b_vfs = b.vfs.lock().expect("project B");
            b_vfs.share_assets(&shared);
            b_vfs.index_with(Arc::clone(&hub));
        }
        if let Ok(mut state) = state.lock() {
            state.project_folder = Some(b.root.to_string_lossy().into_owned());
            state.asset_entries = collect_asset_entries(&b.vfs.lock().expect("project B"));
        }

        for _ in 0..crate::tests::scene_loads::SETTLE_FRAMES {
            engine_frame(&runtime);
            land_reindex(&b.vfs, &state);
            if filed(&state).is_none() {
                break;
            }
        }

        let (crate_a, ship_a, barrel_b) = (a.uuid(CRATE), a.uuid(SHIP), b.uuid(BARREL));
        {
            let service = shared.lock().expect("the service");
            let index = service.index();
            assert!(
                index.get_metadata(&barrel_b).is_some(),
                "the engine's service still indexes B's file"
            );
            assert!(
                index.get_metadata(&crate_a).is_none() && index.get_metadata(&ship_a).is_none(),
                "the engine's service, now B's, indexes none of A's files"
            );
        }
        let listed: Vec<String> = state
            .lock()
            .expect("the editor state")
            .asset_entries
            .iter()
            .map(|entry| entry.source_path.clone())
            .collect();
        assert!(
            listed.iter().any(|path| path == BARREL)
                && !listed.iter().any(|path| path == CRATE || path == SHIP),
            "the browser lists B's files, not A's: {listed:?}"
        );
    }
}
