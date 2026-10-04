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

//! The prefab commands against a project on disk: spawning a prefab from
//! its file, and saving part of the scene as a prefab.

use std::collections::HashMap;
use std::path::PathBuf;

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_data::ecs::{PrefabInstance, World};
use khora_sdk::khora_data::scene::{
    capture_subtree, instantiate_prefab, read_scene_file, write_scene_file, CompactEncoding,
    PrefabSource, SceneRecord,
};
use khora_sdk::{MetricsRegistry, SceneFile};

use super::super::*;

const TURRET: &str = "prefabs/turret.kprefab";
const TOWER: &str = "prefabs/tower.kprefab";

/// Prefabs held in memory, by asset id.
struct Library(HashMap<AssetUUID, SceneRecord>);

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?} in the library"))
    }
}

/// A turret prefab — a root and its barrel — and the barrel's id in it.
fn turret_prefab() -> (SceneRecord, PersistentId) {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Turret")));
    let barrel = world.spawn((Transform::identity(), Name::new("Barrel")));
    world.set_parent(barrel, Some(root));
    world.mark_authored(root).expect("authored");
    let barrel = world.mark_authored(barrel).expect("authored");
    (
        capture_subtree(&world, root).expect("the turret captures"),
        barrel,
    )
}

/// A project on disk holding the turret prefab as `TURRET`, removed when the
/// test ends.
struct Project {
    root: PathBuf,
    vfs: Arc<Mutex<ProjectVfs>>,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn project(name: &str, turret: &SceneRecord) -> Project {
    let root = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("an old project is removed");
    }
    let prefabs = root.join("assets").join("prefabs");
    std::fs::create_dir_all(&prefabs).expect("the prefab folder");
    let file = write_scene_file(turret, &CompactEncoding).expect("the prefab writes");
    std::fs::write(prefabs.join("turret.kprefab"), file.to_bytes()).expect("the prefab is written");
    let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    Project {
        root,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

impl Project {
    fn uuid(&self, rel: &str) -> AssetUUID {
        self.vfs.lock().expect("the project").resolve_uuid(rel)
    }
}

/// The entities of `world` that are a prefab instance's root, with the
/// prefab each links to.
fn instance_roots(world: &GameWorld) -> Vec<(EntityId, AssetUUID)> {
    world
        .iter_entities()
        .filter_map(|e| {
            world
                .get_component::<PrefabInstance>(e)
                .map(|link| (e, link.prefab))
        })
        .collect()
}

/// Spawning a prefab from the project makes a linked instance: one root
/// carrying the link to the prefab's file, its barrel under the id derived
/// from that root — so an edit of the file reaches it on the next load.
#[test]
fn spawning_a_prefab_links_the_instance() {
    let (turret, barrel) = turret_prefab();
    let project = project("prefab-spawn", &turret);
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TURRET.to_owned(), None));

    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);

    let roots = instance_roots(&world);
    assert_eq!(roots.len(), 1, "one instance root: {roots:?}");
    let (root, prefab) = roots[0];
    assert_eq!(prefab, project.uuid(TURRET));
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let barrel = world
        .inner_world()
        .entity_with_id(PersistentId::within(instance, barrel))
        .expect("the barrel is known by the id derived from the root");
    assert_eq!(
        world.get_component::<Parent>(barrel).map(|p| p.0),
        Some(root)
    );
    assert_eq!(world.iter_entities().count(), 2);
}

/// Saving a subtree that holds an instance as a prefab writes the instance
/// as a link to its prefab — no row of its members — and spawning that new
/// prefab brings the nested instance back linked.
#[test]
fn saving_a_subtree_as_a_prefab_keeps_its_instances_linked() {
    let (turret, barrel) = turret_prefab();
    let project = project("prefab-save", &turret);
    let turret_id = project.uuid(TURRET);
    let library = Library(HashMap::from([(turret_id, turret)]));
    let mut world = GameWorld::new();
    let tower = world.spawn((Transform::identity(), Name::new("Tower")));
    world
        .inner_world_mut()
        .mark_authored(tower)
        .expect("authored");
    let nested = instantiate_prefab(world.inner_world_mut(), turret_id, &library)
        .expect("the turret instantiates");
    world.set_parent(nested, Some(tower));
    let nested = world
        .inner_world()
        .persistent_id(nested)
        .expect("the turret has an id");
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_save_as_prefab_at = Some((tower, TOWER.to_owned()));

    process_pending_save_as_prefab(Some(&project.vfs), &world, &state);

    let bytes = std::fs::read(
        project
            .root
            .join("assets")
            .join("prefabs")
            .join("tower.kprefab"),
    )
    .expect("the tower prefab is written");
    let record = read_scene_file(&SceneFile::from_bytes(&bytes).expect("a scene file"))
        .expect("the tower prefab reads");
    assert_eq!(record.instances.len(), 1, "the turret is kept as a link");
    assert_eq!(record.instances[0].prefab, turret_id);
    assert_eq!(record.instances[0].root, nested);
    assert!(
        !record
            .pages
            .iter()
            .any(|page| page.rows.contains(&PersistentId::within(nested, barrel))),
        "the tower prefab holds a row of the turret's barrel"
    );

    let mut spawned = GameWorld::new();
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TOWER.to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut spawned, &state);
    let mut links: Vec<AssetUUID> = instance_roots(&spawned)
        .into_iter()
        .map(|(_, prefab)| prefab)
        .collect();
    links.sort();
    let mut expected = vec![project.uuid(TOWER), turret_id];
    expected.sort();
    assert_eq!(
        links, expected,
        "the tower and its nested turret are both linked"
    );
}

/// Saving an instance of a prefab over that prefab's own file — dropping a
/// spawned turret back where `turret.kprefab` lives — must not leave a
/// prefab that links to itself: the file still holds a turret, and spawning
/// it still brings the root and its barrel.
#[test]
fn saving_an_instance_over_its_own_prefab_keeps_the_prefab_whole() {
    let (turret, _) = turret_prefab();
    let project = project("prefab-overwrite", &turret);
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TURRET.to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
    let (root, _) = instance_roots(&world)[0];

    state
        .lock()
        .expect("the editor state")
        .pending_save_as_prefab_at = Some((root, TURRET.to_owned()));
    process_pending_save_as_prefab(Some(&project.vfs), &world, &state);

    let mut spawned = GameWorld::new();
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TURRET.to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut spawned, &state);
    assert_eq!(
        spawned.iter_entities().count(),
        2,
        "the turret prefab no longer spawns its root and barrel"
    );
}

/// Dropping a spawned `Turret` instance into the folder its prefab lives in
/// names the file after the entity — `Turret.kprefab` — which, where file
/// names ignore case, is the very `turret.kprefab` it came from. The prefab
/// written over it must still hold a turret: spawning it still brings the
/// root and its barrel.
#[test]
fn saving_an_instance_over_its_prefab_under_another_case_keeps_the_prefab_whole() {
    let (turret, _) = turret_prefab();
    let project = project("prefab-overwrite-case", &turret);
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TURRET.to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
    let (root, _) = instance_roots(&world)[0];

    state
        .lock()
        .expect("the editor state")
        .pending_save_as_prefab_at = Some((root, "prefabs/Turret.kprefab".to_owned()));
    process_pending_save_as_prefab(Some(&project.vfs), &world, &state);

    let mut spawned = GameWorld::new();
    state.lock().expect("the editor state").pending_prefab_spawn = Some((TURRET.to_owned(), None));
    process_pending_prefab_spawn(Some(&project.vfs), &mut spawned, &state);
    assert_eq!(
        spawned.iter_entities().count(),
        2,
        "the turret prefab no longer spawns its root and barrel"
    );
}
