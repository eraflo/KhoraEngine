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

//! **The editor's writes are indexed off the frame, and what needs the index
//! waits for it.** The project serves the engine's asset service and files
//! its reindexes with the engine's index hub. A save, a create or a rename
//! reindexes nothing on the frame: the service, and the browser, learn of
//! the file only once the reindex lands; what must use the new file — a
//! material assigned by its identity, a script revealed, a scene opened —
//! runs then. The engine's side of a frame is played by hand
//! ([`IndexingEngine`]), around the editor's own `land_reindex`, whose
//! actions that need the world are applied as `before_frame` applies them.

use std::path::Path;

use khora_sdk::khora_core::asset::{AssetUUID, Material, StandardMaterial};
use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;
use khora_sdk::{SerializationGoal, SerializationService};

use super::super::*;
use crate::hot_reload::{collect_asset_entries, land_reindex};
use crate::tests::gltf_fixture::{GltfProject, TRIANGLE_OBJ};
use crate::tests::project_index::IndexingEngine;
use crate::tests::scene_loads::SETTLE_FRAMES;

/// A project holding `files`, serving `engine`'s assets and filing its
/// reindexes with `engine`'s hub.
fn served(engine: &IndexingEngine, name: &str, files: &[(&str, &str)]) -> GltfProject {
    let project = GltfProject::with(name, files);
    engine.serve(&project.vfs);
    project
}

/// An editor over `project`, its browser listing what the project's index
/// holds now, at epoch 1.
fn editor_over(project: &GltfProject) -> Arc<Mutex<EditorState>> {
    let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
    Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().into_owned()),
        asset_entries: entries,
        asset_epoch: 1,
        ..EditorState::default()
    }))
}

/// Frames of the engine around the editor's `before_frame`: what was filed
/// is built and landed, then the editor reads the landing and applies the
/// actions that need the world.
fn settle(
    engine: &IndexingEngine,
    project: &GltfProject,
    state: &Arc<Mutex<EditorState>>,
    world: &mut GameWorld,
) {
    for _ in 0..SETTLE_FRAMES {
        engine.frame();
        let actions = land_reindex(&project.vfs, state);
        apply_after_index(world, actions);
    }
}

/// Whether the browser's entries list the file at `rel`.
fn listed(state: &Arc<Mutex<EditorState>>, rel: &str) -> bool {
    state
        .lock()
        .expect("the editor state")
        .asset_entries
        .iter()
        .any(|entry| entry.source_path == rel)
}

/// **A saved material is assigned once its index lands.** An entity wears
/// an inline material, which the author saves as a `.kmat`. The file is
/// written at once, but the index does not know it yet: the entity keeps
/// its inline material, so nothing asks the engine for an asset it cannot
/// find, and the service never records a failed decode of it. Once the
/// reindex lands, the entity references the saved material, and it decodes.
#[test]
fn a_saved_material_is_assigned_once_its_index_lands() {
    const STEEL: &str = "materials/Steel.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-assigned-once-indexed", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));

    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    let steel = project.uuid(STEEL);
    assert!(project.path(STEEL).exists(), "the material is written");
    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Inline { .. })
        ),
        "the entity keeps its inline material until the index lands"
    );
    assert!(
        !engine.indexes(&steel),
        "the save did not reindex on the frame"
    );
    assert!(
        !engine
            .shared
            .lock()
            .expect("the service")
            .has_failed(&steel),
        "no decode of the material failed"
    );

    // Read before the engine's frames: still in flight, nothing to apply.
    let early = land_reindex(&project.vfs, &state);
    assert!(early.is_empty(), "nothing landed yet: {early:?}");

    settle(&engine, &project, &state, &mut world);

    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Asset(uuid)) if *uuid == steel
        ),
        "the entity references the saved material once its index landed"
    );
    let mut service = engine.shared.lock().expect("the service");
    assert!(
        service.load::<Box<dyn Material>>(&steel).is_ok(),
        "the saved material decodes"
    );
    assert!(!service.has_failed(&steel), "no decode of it failed");
}

/// **A material assigned at a landing spares an entity changed since.** An
/// entity despawned before the reindex landed stays gone; one that wears
/// another material asset since keeps it.
#[test]
fn a_material_landing_spares_an_entity_despawned_or_reassigned_since() {
    let saved = AssetUUID::new_v5("materials/Steel.kmat");
    let chosen = AssetUUID::new_v5("materials/Gold.kmat");
    let mut world = GameWorld::new();
    let inline = || MaterialRef::inline(Box::new(StandardMaterial::default()));
    let gone = world.spawn((Transform::identity(), Name::new("Gone"), inline()));
    // By the landing, this one wears another material asset: the state a
    // reassignment since the save leaves it in. Spawned so, as
    // `GameWorld::add_component` does not replace a component already there.
    let reassigned = world.spawn((
        Transform::identity(),
        Name::new("Reassigned"),
        MaterialRef::Asset(chosen),
    ));
    world.despawn(gone);

    apply_after_index(
        &mut world,
        vec![
            crate::project_vfs::AfterIndex::AssignMaterial {
                entity: gone,
                uuid: saved,
                saved: String::new(),
                persistent: None,
                choice: 0,
                played: false,
            },
            crate::project_vfs::AfterIndex::AssignMaterial {
                entity: reassigned,
                uuid: saved,
                saved: String::new(),
                persistent: None,
                choice: 0,
                played: false,
            },
        ],
    );

    assert!(
        world.get_component::<MaterialRef>(gone).is_none(),
        "a despawned entity is not brought back"
    );
    assert!(
        matches!(
            world.get_component::<MaterialRef>(reassigned),
            Some(MaterialRef::Asset(uuid)) if *uuid == chosen
        ),
        "an entity that wears another material asset since keeps it"
    );
}

/// **A scene saved is listed once indexed.** The save writes the file, but
/// reindexes nothing on the frame: the service does not know it and the
/// browser does not list it. Once the reindex lands, both do, and the
/// browser is told to rescan.
#[test]
fn a_scene_saved_is_listed_once_indexed() {
    const LEVEL: &str = "scenes/level.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "scene-listed-once-indexed", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");

    let path = project.path(LEVEL);
    assert!(
        save_scene_dispatch(Some(&project.vfs), &world, &path.to_string_lossy()),
        "the scene is saved"
    );

    let level = project.uuid(LEVEL);
    assert!(path.exists(), "the scene is written");
    assert!(
        !engine.indexes(&level),
        "nothing was reindexed on the frame"
    );
    assert!(!listed(&state, LEVEL), "the browser does not list it yet");
    assert_eq!(engine.waiting(), 1, "the save filed a reindex");

    settle(&engine, &project, &state, &mut world);

    assert!(engine.indexes(&level), "the service indexes the scene");
    assert!(listed(&state, LEVEL), "the browser lists the scene");
    assert_ne!(
        state.lock().expect("the editor state").asset_epoch,
        1,
        "the browser is told to rescan"
    );
}

/// **A script created is revealed once indexed.** The browser asks for a
/// new script: it is written at once, but revealed only when the reindex
/// lands, when the browser's entries hold it.
#[test]
fn a_script_created_is_revealed_once_indexed() {
    const PLAYER: &str = "scripts/Player.erg";
    let engine = IndexingEngine::new();
    let project = served(&engine, "script-revealed-once-indexed", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    state
        .lock()
        .expect("the editor state")
        .pending_create_script = Some((PLAYER.to_owned(), "behavior Player { }".to_owned()));

    process_pending_asset_file_ops(Some(&project.vfs), &state);

    assert!(project.path(PLAYER).exists(), "the script is written");
    assert!(
        !engine.indexes(&project.uuid(PLAYER)),
        "nothing was reindexed on the frame"
    );
    assert_eq!(
        state.lock().expect("the editor state").pending_reveal_asset,
        None,
        "not revealed before the index holds it"
    );

    settle(&engine, &project, &state, &mut world);

    assert!(listed(&state, PLAYER), "the browser lists the script");
    assert_eq!(
        state
            .lock()
            .expect("the editor state")
            .pending_reveal_asset
            .as_deref(),
        Some(PLAYER),
        "revealed once the index holds it"
    );
}

/// **A renamed asset keeps its identity once reindexed.** Until the
/// reindex lands, the service still finds the asset at its old path; after
/// it, at its new path, under the same identity — never under the one its
/// new path would derive.
#[test]
fn a_renamed_asset_keeps_its_uuid_once_reindexed() {
    const CRATE: &str = "meshes/crate.obj";
    const BOX: &str = "meshes/box.obj";
    let engine = IndexingEngine::new();
    let project = served(
        &engine,
        "renamed-keeps-uuid-once-reindexed",
        &[(CRATE, TRIANGLE_OBJ)],
    );
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let uuid = project.uuid(CRATE);
    state.lock().expect("the editor state").pending_rename_asset =
        Some((CRATE.to_owned(), BOX.to_owned()));

    process_pending_asset_file_ops(Some(&project.vfs), &state);

    assert!(project.path(BOX).exists(), "the file is moved");
    assert_eq!(
        project.vfs.lock().expect("the project").rel_path_of(uuid),
        Some(CRATE.to_owned()),
        "until the landing, the service indexes the old path"
    );

    settle(&engine, &project, &state, &mut world);

    assert_eq!(
        project.vfs.lock().expect("the project").rel_path_of(uuid),
        Some(BOX.to_owned()),
        "after the landing, the new path, under the same identity"
    );
    assert_eq!(project.uuid(BOX), uuid, "the new path resolves to it");
    assert!(
        !engine.indexes(&AssetUUID::new_v5(BOX)),
        "not indexed under the identity its new path derives"
    );
    assert!(
        engine
            .shared
            .lock()
            .expect("the service")
            .load::<Mesh>(&uuid)
            .is_ok(),
        "the mesh decodes under its identity"
    );
}

/// **A prefab saved joins the catalog once indexed.** A script's `Spawn` is
/// checked against the project's prefab catalog; a prefab saved into the
/// project joins it when the reindex lands, not before.
#[test]
fn a_saved_prefab_joins_the_catalog_once_indexed() {
    const TURRET: &str = "prefabs/turret.kprefab";
    let engine = IndexingEngine::new();
    let project = served(&engine, "prefab-catalog-once-indexed", &[]);
    let catalog = khora_sdk::khora_data::scene::PrefabCatalog::new();
    project
        .vfs
        .lock()
        .expect("the project")
        .follow_prefab_catalog(catalog.clone());
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let turret = world.spawn((Transform::identity(), Name::new("Turret")));
    world
        .inner_world_mut()
        .mark_authored(turret)
        .expect("authored");
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_prefab_at = Some((turret, TURRET.to_owned()));

    process_pending_save_as_prefab(Some(&project.vfs), &world, &state);

    let prefab = project.uuid(TURRET);
    assert!(project.path(TURRET).exists(), "the prefab is written");
    assert!(
        !catalog.holds(prefab),
        "the catalog does not hold it before the index does"
    );
    assert!(
        !engine.indexes(&prefab),
        "nothing was reindexed on the frame"
    );

    settle(&engine, &project, &state, &mut world);

    assert!(catalog.holds(prefab), "the catalog holds the saved prefab");
}

/// A scene file of one authored entity, written under `rel` behind the
/// index's back: the project's index does not know it.
fn scene_behind_the_index(project: &GltfProject, rel: &str) {
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Hidden")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    let bytes = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes();
    let path = project.path(rel);
    std::fs::create_dir_all(path.parent().expect("the scene's folder")).expect("its folder");
    std::fs::write(&path, bytes).expect("the scene is written");
}

/// Whether `pending` names the file at `expected`.
fn names_the_file(pending: Option<&str>, expected: &Path) -> bool {
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    pending.is_some_and(|pending| {
        canonical(Path::new(pending)).is_some()
            && canonical(Path::new(pending)) == canonical(expected)
    })
}

/// **A scene missing from the index opens once indexed.** A scene written
/// behind the index's back is opened: the open files a reindex and nothing
/// else — no scene load, no reindex on the frame. Once the reindex lands
/// and the index holds the scene, it is queued to open.
#[test]
fn a_scene_missing_from_the_index_opens_once_indexed() {
    const HIDDEN: &str = "scenes/hidden.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "scene-opens-once-indexed", &[]);
    scene_behind_the_index(&project, HIDDEN);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);

    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &crate::tests::scene_loads::hub(&scenes),
        &project.path(HIDDEN),
    );

    assert!(
        !engine.indexes(&project.uuid(HIDDEN)),
        "nothing was reindexed on the frame"
    );
    assert_eq!(engine.waiting(), 1, "the open filed a reindex");
    {
        let state = state.lock().expect("the editor state");
        assert!(state.opening.is_empty(), "no scene load was filed");
        assert_eq!(state.pending_scene_load, None, "nothing is queued yet");
    }

    settle(&engine, &project, &state, &mut world);

    let pending = state
        .lock()
        .expect("the editor state")
        .pending_scene_load
        .clone();
    assert!(
        names_the_file(pending.as_deref(), &project.path(HIDDEN)),
        "the scene is queued to open once indexed: {pending:?}"
    );
    assert_eq!(engine.filed(), 1, "one reindex, no other");
}

/// **A scene still missing after its reindex is not indexed again.** A
/// scene that is nowhere is opened: one reindex is filed; when it lands
/// and the scene is still missing, nothing is opened and no second reindex
/// is filed.
#[test]
fn a_scene_still_missing_after_its_reindex_is_reported_once() {
    const NOWHERE: &str = "scenes/nowhere.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "scene-still-missing", &[]);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);

    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &crate::tests::scene_loads::hub(&scenes),
        &project.path(NOWHERE),
    );
    assert_eq!(engine.waiting(), 1, "the open filed a reindex");

    settle(&engine, &project, &state, &mut world);

    assert_eq!(engine.filed(), 1, "no second reindex was filed");
    let state = state.lock().expect("the editor state");
    assert_eq!(state.pending_scene_load, None, "nothing is queued to open");
    assert!(state.opening.is_empty(), "no scene load was filed");
}

/// A scene file of one authored entity named `name`, wearing an inline
/// material, written at `path`, outside the project's assets.
fn scene_of_one_inline_entity(path: &Path, name: &str) {
    let mut world = GameWorld::new();
    let root = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new(name),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    let bytes = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes();
    std::fs::write(path, bytes).expect("the scene is written");
}

/// **A material landing after the scene was replaced touches none of the new
/// scene's entities.** An entity's inline material is saved as a `.kmat`;
/// before its reindex lands, another scene replaces the world. The entity
/// the save named is gone: its id may come back attached to an entity of
/// the new scene, which must keep its own inline material.
#[test]
fn a_material_landing_after_the_scene_was_replaced_touches_no_new_entity() {
    const STEEL: &str = "materials/Steel.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-landing-after-replace", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let crate_entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((crate_entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);
    assert!(
        !engine.indexes(&project.uuid(STEEL)),
        "the premise: the assignment waits for the reindex"
    );

    // Another scene opens and lands before the reindex does.
    let other = project.root.join("other.kscene");
    scene_of_one_inline_entity(&other, "Barrel");
    super::open_and_land(&mut world, &state, &other);

    settle(&engine, &project, &state, &mut world);

    let steel = project.uuid(STEEL);
    let wearing_steel: Vec<_> = world
        .iter_entities()
        .filter(|entity| {
            matches!(
                world.get_component::<MaterialRef>(*entity),
                Some(MaterialRef::Asset(uuid)) if *uuid == steel
            )
        })
        .map(|entity| {
            world
                .get_component::<Name>(entity)
                .map(|name| name.as_str().to_owned())
        })
        .collect();
    assert!(
        wearing_steel.is_empty(),
        "an entity of the new scene was given the old scene's saved material: {wearing_steel:?}"
    );
}

/// **A new scene has the last word over a scene waiting for its index.** A
/// scene missing from the index is opened: it waits for a reindex. Before
/// that lands, the author asks for a new scene. The new scene stays: once
/// the reindex lands, the scene asked for before is not queued to open.
#[test]
fn a_new_scene_has_the_last_word_over_a_scene_waiting_for_its_index() {
    const HIDDEN: &str = "scenes/hidden.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "new-scene-over-waiting-open", &[]);
    scene_behind_the_index(&project, HIDDEN);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);

    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &crate::tests::scene_loads::hub(&scenes),
        &project.path(HIDDEN),
    );
    assert_eq!(
        engine.waiting(),
        1,
        "the premise: the open waits for a reindex"
    );
    apply_new_scene(&mut world, &state);

    settle(&engine, &project, &state, &mut world);

    assert_eq!(
        state.lock().expect("the editor state").pending_scene_load,
        None,
        "the scene asked for before the new scene is not opened over it"
    );
}

/// **A scene opened since has the last word over one waiting for its
/// index.** A scene missing from the index is opened and waits for a
/// reindex; before that lands, the author opens another scene, which lands.
/// Once the reindex lands, the scene asked for first is not queued to open
/// over the one asked for last.
#[test]
fn a_scene_opened_since_has_the_last_word_over_one_waiting_for_its_index() {
    const HIDDEN: &str = "scenes/hidden.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "later-open-over-waiting-open", &[]);
    scene_behind_the_index(&project, HIDDEN);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);

    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &crate::tests::scene_loads::hub(&scenes),
        &project.path(HIDDEN),
    );
    assert_eq!(
        engine.waiting(),
        1,
        "the premise: the open waits for a reindex"
    );
    let other = project.root.join("other.kscene");
    scene_of_one_inline_entity(&other, "Barrel");
    super::open_and_land(&mut world, &state, &other);

    settle(&engine, &project, &state, &mut world);

    assert_eq!(
        state.lock().expect("the editor state").pending_scene_load,
        None,
        "the scene asked for first is not opened over the one asked for last"
    );
}

/// **A scene opened once indexed loads from the path queued.** The path a
/// landing queues for a scene missing from the index is read back by the
/// next update as the open's own: it resolves inside the project, and the
/// scene lands in the world, its path the editor's current one.
#[test]
fn a_scene_queued_once_indexed_opens_from_the_path_queued() {
    const HIDDEN: &str = "scenes/hidden.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "scene-queued-opens", &[]);
    scene_behind_the_index(&project, HIDDEN);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);
    let loads = crate::tests::scene_loads::hub(&scenes);
    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &loads,
        &project.path(HIDDEN),
    );
    settle(&engine, &project, &state, &mut world);

    // The next update takes the path queued, as `EditorApp::update` does.
    let queued = state
        .lock()
        .expect("the editor state")
        .pending_scene_load
        .take()
        .expect("the scene is queued to open");
    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &loads,
        Path::new(&queued),
    );
    super::land_filed_loads(&scenes, &mut world, &state);

    let names: Vec<String> = world
        .iter_entities()
        .filter_map(|entity| {
            world
                .get_component::<Name>(entity)
                .map(|name| name.as_str().to_owned())
        })
        .collect();
    assert!(
        names.iter().any(|name| name == "Hidden"),
        "the scene queued once indexed is the one loaded: {names:?}"
    );
    assert_eq!(engine.filed(), 1, "its open filed no other reindex");
}

/// **The material saved last is the one the entity wears.** An entity's
/// inline material is saved as `Steel`, then, before that reindex lands,
/// saved again as `Iron` — the save accepts it, the entity being still
/// inline. Once both landed, the entity references `Iron`, the author's
/// last choice, not `Steel`.
#[test]
fn the_material_saved_last_is_the_one_the_entity_wears() {
    const STEEL: &str = "materials/Steel.kmat";
    const IRON: &str = "materials/Iron.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-saved-last-wins", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    for name in ["Steel", "Iron"] {
        state
            .lock()
            .expect("the editor state")
            .pending_save_as_material = Some((entity, name.to_owned()));
        process_pending_save_as_material(Some(&project.vfs), &mut world, &state);
    }
    assert!(
        project.path(STEEL).exists() && project.path(IRON).exists(),
        "the premise: both saves wrote their material"
    );

    settle(&engine, &project, &state, &mut world);

    let iron = project.uuid(IRON);
    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Asset(uuid)) if *uuid == iron
        ),
        "the entity wears the material saved last (Iron = {iron}, Steel = {}): {:?}",
        project.uuid(STEEL),
        world.get_component::<MaterialRef>(entity).map(|m| match m {
            MaterialRef::Asset(uuid) => uuid.to_string(),
            MaterialRef::Inline { .. } => "inline".to_owned(),
        })
    );
}

/// **An inline material changed since the save is not overwritten by it.**
/// An entity's inline material is saved as `Steel`; before that reindex
/// lands, a texture is dropped on the entity, which now wears a new inline
/// material made of it. The landing does not put the saved `Steel` over
/// that later change.
#[test]
fn an_inline_material_changed_since_the_save_is_kept_at_the_landing() {
    const RUST: &str = "textures/rust.png";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-changed-since-save", &[(RUST, "png")]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    // The texture is dropped on the entity before the reindex lands.
    let rust = project.uuid(RUST);
    state
        .lock()
        .expect("the editor state")
        .pending_assign_texture = Some((rust, entity));
    process_pending_assign_texture(Some(&project.vfs), &mut world, &state);
    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Inline { .. })
        ),
        "the premise: the drop gave the entity a new inline material"
    );

    settle(&engine, &project, &state, &mut world);

    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Inline { .. })
        ),
        "the texture dropped after the save is not overwritten by the saved material"
    );
}

/// **A material saved just before Play is still worn after Stop.** An
/// entity's inline material is saved as `Steel` while editing; Play is
/// pressed before the reindex lands, and the landing comes during play.
/// Stop brings back the scene as it was edited: the entity references the
/// material the author saved, not its inline one.
#[test]
fn a_material_saved_just_before_play_is_worn_after_stop() {
    const STEEL: &str = "materials/Steel.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-saved-before-play", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("authored");
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    apply_play(&mut world, &state);
    settle(&engine, &project, &state, &mut world);
    super::stop_and_land(&mut world, &state);
    // The `before_frame` that landed the restore then reads the project's
    // landing and applies what waited for it, in that order.
    let actions = land_reindex(&project.vfs, &state);
    apply_after_index(&mut world, actions);

    let steel = project.uuid(STEEL);
    let crate_material = world
        .iter_entities()
        .find(|entity| {
            world
                .get_component::<Name>(*entity)
                .is_some_and(|name| name.as_str() == "Crate")
        })
        .and_then(|entity| world.get_component::<MaterialRef>(entity))
        .map(|material| match material {
            MaterialRef::Asset(uuid) => Some(*uuid),
            MaterialRef::Inline { .. } => None,
        });
    assert_eq!(
        crate_material,
        Some(Some(steel)),
        "after Stop, the entity wears the material saved while editing"
    );
}

/// **A scene asked for on the frame a waiting open lands is the one
/// opened.** A scene missing from the index is opened and waits for a
/// reindex. On the frame that reindex lands, the shell — drawn before the
/// landing is read — has queued another scene the author double-clicked:
/// the landing does not put the scene asked for first over it.
#[test]
fn a_scene_asked_for_as_a_waiting_open_lands_is_not_overwritten() {
    const HIDDEN: &str = "scenes/hidden.kscene";
    let engine = IndexingEngine::new();
    let project = served(&engine, "open-asked-as-waiting-lands", &[]);
    scene_behind_the_index(&project, HIDDEN);
    let state = editor_over(&project);
    let scenes = crate::tests::scene_loads::runtime();
    let mut world = crate::tests::scene_loads::engine_world(&scenes);
    open_scene_dispatch(
        Some(&project.vfs),
        &mut world,
        &state,
        &crate::tests::scene_loads::hub(&scenes),
        &project.path(HIDDEN),
    );

    // The shell queues the scene double-clicked, then the landing is read.
    let chosen = project
        .root
        .join("other.kscene")
        .to_string_lossy()
        .into_owned();
    state.lock().expect("the editor state").pending_scene_load = Some(chosen.clone());
    settle(&engine, &project, &state, &mut world);

    assert_eq!(
        state.lock().expect("the editor state").pending_scene_load,
        Some(chosen),
        "the scene the author asked for last is the one queued"
    );
}

/// The entity named `name`, if the world holds one.
fn named(world: &GameWorld, name: &str) -> Option<EntityId> {
    world.iter_entities().find(|entity| {
        world
            .get_component::<Name>(*entity)
            .is_some_and(|found| found.as_str() == name)
    })
}

/// The material asset `entity` references: `Some(None)` while it wears an
/// inline material, `None` when it has no material.
fn worn(world: &GameWorld, entity: EntityId) -> Option<Option<AssetUUID>> {
    world
        .get_component::<MaterialRef>(entity)
        .map(|material| match material {
            MaterialRef::Asset(uuid) => Some(*uuid),
            MaterialRef::Inline { .. } => None,
        })
}

/// **A material landing spares the scene opened since, even where it holds
/// the same entity.** An authored entity's inline material is saved as a
/// `.kmat`; before its reindex lands, the author opens another scene — a copy
/// of this one, or this one reopened from disk — which holds an entity under
/// the same identity, wearing the same inline material. The save was made on
/// the scene the author left: the scene opened since keeps its inline
/// material, as it keeps its own choice of scene over an open waiting for the
/// index.
#[test]
fn a_material_landing_spares_the_same_entity_of_a_scene_opened_since() {
    const STEEL: &str = "materials/Steel.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-landing-after-reopen", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let crate_entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    world
        .inner_world_mut()
        .mark_authored(crate_entity)
        .expect("authored");
    // The scene as its file holds it: the one the author opens next.
    let copy = project.root.join("copy.kscene");
    let bytes = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene saves")
        .to_bytes();
    std::fs::write(&copy, bytes).expect("the scene is written");

    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((crate_entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);
    assert!(
        !engine.indexes(&project.uuid(STEEL)),
        "the premise: the assignment waits for the reindex"
    );

    super::open_and_land(&mut world, &state, &copy);
    settle(&engine, &project, &state, &mut world);

    let opened = named(&world, "Crate").expect("the scene opened holds the crate");
    assert_eq!(
        worn(&world, opened),
        Some(None),
        "the scene opened since keeps its inline material (Steel = {})",
        project.uuid(STEEL)
    );
}

/// **The material saved last is worn, across a Stop.** An authored entity's
/// inline material is saved as `Steel`; Play and Stop come before its reindex
/// lands, and the restored entity — the same identity under another id — is
/// saved again as `Iron`. Once both landed, the entity wears `Iron`, the
/// author's last choice.
#[test]
fn the_material_saved_last_is_worn_across_a_stop() {
    const STEEL: &str = "materials/Steel.kmat";
    const IRON: &str = "materials/Iron.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-saved-last-across-stop", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("authored");
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    apply_play(&mut world, &state);
    super::stop_and_land(&mut world, &state);
    let actions = land_reindex(&project.vfs, &state);
    apply_after_index(&mut world, actions);
    let restored = named(&world, "Crate").expect("the restore brings the crate back");
    assert_ne!(
        restored, entity,
        "the premise: the restore gave it another id"
    );

    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((restored, "Iron".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);
    settle(&engine, &project, &state, &mut world);

    assert_eq!(
        worn(&world, restored),
        Some(Some(project.uuid(IRON))),
        "the entity wears the material saved last (Iron = {}, Steel = {})",
        project.uuid(IRON),
        project.uuid(STEEL)
    );
}

/// **A material saved while editing is worn after Stop, whatever is saved
/// during play.** An authored entity's inline material is saved as `Steel`
/// while editing; Play is pressed before its reindex lands. During play the
/// entity's material is changed and saved as `Iron` — a change of the played
/// world, which Stop discards. After Stop, the edited entity wears `Steel`,
/// as it would had its reindex landed before Play.
#[test]
fn a_material_saved_while_editing_is_worn_after_a_save_during_play() {
    const STEEL: &str = "materials/Steel.kmat";
    let engine = IndexingEngine::new();
    let project = served(&engine, "material-saved-again-during-play", &[]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("authored");
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    apply_play(&mut world, &state);
    world.add_component(
        entity,
        MaterialRef::inline(Box::new(StandardMaterial {
            metallic: 1.0,
            ..StandardMaterial::default()
        })),
    );
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Iron".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);
    settle(&engine, &project, &state, &mut world);
    super::stop_and_land(&mut world, &state);
    let actions = land_reindex(&project.vfs, &state);
    apply_after_index(&mut world, actions);

    let restored = named(&world, "Crate").expect("the restore brings the crate back");
    assert_eq!(
        worn(&world, restored),
        Some(Some(project.uuid(STEEL))),
        "after Stop, the entity wears the material saved while editing"
    );
}

/// **A textured material saved just before Play is worn after Stop.** As
/// [`a_material_saved_just_before_play_is_worn_after_stop`], with a material
/// whose fields are not the defaults: a base color texture, colors and
/// factors no short decimal writes. The restore brings back the same
/// material, so the landing recognises it.
#[test]
fn a_textured_material_saved_just_before_play_is_worn_after_stop() {
    const STEEL: &str = "materials/Steel.kmat";
    const RUST: &str = "textures/rust.png";
    let engine = IndexingEngine::new();
    let project = served(&engine, "textured-material-before-play", &[(RUST, "png")]);
    let state = editor_over(&project);
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial {
            base_color: khora_sdk::khora_core::math::LinearRgba::new(0.1, 0.2, 0.3, 0.7),
            base_color_texture: Some(project.uuid(RUST)),
            metallic: 1.0 / 3.0,
            roughness: 0.123_456_79,
            ..StandardMaterial::default()
        })),
    ));
    world
        .inner_world_mut()
        .mark_authored(entity)
        .expect("authored");
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_material = Some((entity, "Steel".to_owned()));
    process_pending_save_as_material(Some(&project.vfs), &mut world, &state);

    apply_play(&mut world, &state);
    settle(&engine, &project, &state, &mut world);
    super::stop_and_land(&mut world, &state);
    let actions = land_reindex(&project.vfs, &state);
    apply_after_index(&mut world, actions);

    let restored = named(&world, "Crate").expect("the restore brings the crate back");
    assert_eq!(
        worn(&world, restored),
        Some(Some(project.uuid(STEEL))),
        "after Stop, the entity wears the textured material saved while editing"
    );
}
