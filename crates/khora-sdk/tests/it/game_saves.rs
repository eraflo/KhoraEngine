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

//! A game saved and loaded through the `GameWorld` façade alone.

use khora_sdk::prelude::ecs::{Name, Transform};
use khora_sdk::prelude::math::Vec3;
use khora_sdk::prelude::AssetUUID;
use khora_sdk::{GameWorld, SceneFile, SerializationGoal, SerializationService};

/// A level as it ships — two authored entities — its scene file, and the
/// identity the game knows it by.
fn level() -> (GameWorld, SceneFile, AssetUUID) {
    let mut world = GameWorld::new();
    for (name, x) in [("Hero", 0.0), ("Door", 5.0)] {
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(x, 0.0, 0.0)),
            Name::new(name),
        ));
        world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("a level's entities are authored");
    }
    let scene = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the level saves");
    (world, scene, AssetUUID::new_v5("levels/one.kscene"))
}

fn named(world: &GameWorld, name: &str) -> Option<Transform> {
    world
        .iter_entities()
        .find(|entity| {
            world
                .get_component::<Name>(*entity)
                .is_some_and(|n| n.as_str() == name)
        })
        .and_then(|entity| world.get_transform(entity).copied())
}

/// **The façade a game saves through.** What the run changed and spawned
/// comes back in a fresh `GameWorld`; what it left alone comes from the
/// level.
#[test]
fn a_game_world_saves_and_loads_a_game() {
    let (mut world, scene, scene_id) = level();
    let hero = world
        .iter_entities()
        .find(|entity| {
            world
                .get_component::<Name>(*entity)
                .is_some_and(|n| n.as_str() == "Hero")
        })
        .expect("the hero");
    world.update_transform(hero, |t| t.translation = Vec3::new(2.0, 0.0, 1.0));
    world.spawn((
        Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)),
        Name::new("Loot"),
    ));

    let save = world
        .save_game(scene_id, &scene, SerializationGoal::EditorInterchange)
        .expect("the game saves");
    let save = SceneFile::from_bytes(&save.to_bytes()).expect("the save parses");

    let mut loaded = GameWorld::new();
    loaded.load_game(&save, &scene).expect("the game loads");

    assert_eq!(loaded.iter_entities().count(), 3);
    assert_eq!(
        named(&loaded, "Hero").map(|t| t.translation),
        Some(Vec3::new(2.0, 0.0, 1.0))
    );
    assert_eq!(
        named(&loaded, "Door").map(|t| t.translation),
        Some(Vec3::new(5.0, 0.0, 0.0))
    );
    assert_eq!(
        named(&loaded, "Loot").map(|t| t.translation),
        Some(Vec3::new(9.0, 0.0, 0.0))
    );
}

/// A save that cannot be loaded leaves the `GameWorld` exactly as it was.
#[test]
fn a_game_world_refuses_a_damaged_save_whole() {
    let (world, scene, scene_id) = level();
    let mut save = world
        .save_game(scene_id, &scene, SerializationGoal::EditorInterchange)
        .expect("the game saves");
    save.payload.truncate(save.payload.len() / 2);
    save.header.payload_length = save.payload.len() as u64;

    let (mut target, _, _) = level();
    let before = target.iter_entities().count();
    assert!(target.load_game(&save, &scene).is_err());
    assert_eq!(target.iter_entities().count(), before);
    assert_eq!(
        named(&target, "Hero").map(|t| t.translation),
        Some(Vec3::ZERO)
    );
}

/// The prefabs a level links to, held in memory by asset id — what the
/// shipped game's asset service is to `run_default`.
struct Prefabs(std::collections::HashMap<AssetUUID, khora_sdk::khora_data::scene::SceneRecord>);

impl khora_sdk::khora_data::scene::PrefabSource for Prefabs {
    fn prefab(&self, id: AssetUUID) -> Result<khora_sdk::khora_data::scene::SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?}"))
    }
}

/// A level holding a prefab instance — saved with its prefabs, as the editor
/// writes it, and loaded the way `run_default` loads it — saves and loads a
/// game through the `GameWorld` façade like any other level.
#[test]
fn a_game_world_saves_a_game_over_a_level_holding_a_prefab_instance() {
    use khora_sdk::khora_data::scene::{capture_subtree, instantiate_prefab, read_scene_file};

    let mut crate_world = GameWorld::new();
    let lid = crate_world.spawn((Transform::identity(), Name::new("Crate")));
    crate_world
        .inner_world_mut()
        .mark_authored(lid)
        .expect("a prefab's entities are authored");
    let prefab_id = AssetUUID::new_v5("prefabs/crate.kprefab");
    let record = capture_subtree(crate_world.inner_world(), lid).expect("the crate captures");
    let prefabs = std::sync::Arc::new(Prefabs([(prefab_id, record)].into_iter().collect()));

    let mut authoring = GameWorld::new();
    instantiate_prefab(authoring.inner_world_mut(), prefab_id, prefabs.as_ref())
        .expect("the crate instantiates");
    let service = SerializationService::with_prefabs(prefabs.clone());
    let scene = service
        .save_world(
            authoring.inner_world(),
            SerializationGoal::EditorInterchange,
        )
        .expect("the level saves");
    assert_eq!(
        read_scene_file(&scene).expect("reads").instances.len(),
        1,
        "the level links to its prefab"
    );
    let scene_id = AssetUUID::new_v5("levels/crates.kscene");

    let mut world = GameWorld::new();
    world.set_prefabs(prefabs.clone());
    service
        .load_world(&scene, world.inner_world_mut())
        .expect("the level loads, as run_default loads it");
    let crate_entity = world.iter_entities().next().expect("the crate");
    world.update_transform(crate_entity, |t| t.translation = Vec3::new(4.0, 0.0, 0.0));

    let save = world
        .save_game(scene_id, &scene, SerializationGoal::EditorInterchange)
        .expect("the game saves");
    let mut loaded = GameWorld::new();
    loaded.set_prefabs(prefabs.clone());
    loaded.load_game(&save, &scene).expect("the game loads");
    assert_eq!(
        named(&loaded, "Crate").map(|t| t.translation),
        Some(Vec3::new(4.0, 0.0, 0.0))
    );
}
