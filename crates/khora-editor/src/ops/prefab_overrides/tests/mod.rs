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

//! The inspector's prefab overrides: the JSON paths that differ, an
//! inspected instance member against its prefab, and the edits that revert
//! an override.

mod inspection;
mod json_paths;
mod reverts;

use std::collections::HashMap;

use khora_sdk::editor_ui::{ComponentOverride, EditorState, PropertyEdit};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_data::ecs::World;
use khora_sdk::khora_data::scene::{capture_subtree, instantiate_prefab, SceneRecord};
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::Vec3;

use super::*;

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

/// A turret prefab: a lit root and two children, a barrel raised one unit
/// and a sight one unit forward — by their ids in the prefab.
struct Turret {
    record: SceneRecord,
    barrel: PersistentId,
    sight: PersistentId,
}

fn turret() -> Turret {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Turret"), Light::point()));
    let barrel = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Barrel"),
    ));
    let sight = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.0, 1.0)),
        Name::new("Sight"),
    ));
    world.set_parent(barrel, Some(root));
    world.set_parent(sight, Some(root));
    world.mark_authored(root).expect("authored");
    let barrel = world.mark_authored(barrel).expect("authored");
    let sight = world.mark_authored(sight).expect("authored");
    Turret {
        record: capture_subtree(&world, root).expect("the turret captures"),
        barrel,
        sight,
    }
}

/// A turret instance placed in an editor world.
struct Placed {
    world: GameWorld,
    library: Library,
    prefab: AssetUUID,
    root: EntityId,
    barrel: EntityId,
    sight: EntityId,
}

fn placed() -> Placed {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(prefab, turret.record)]));
    let mut world = GameWorld::new();
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the turret instantiates");
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let member = |inner: PersistentId| {
        world
            .inner_world()
            .entity_with_id(PersistentId::within(instance, inner))
            .expect("the member is known by its derived id")
    };
    let barrel = member(turret.barrel);
    let sight = member(turret.sight);
    Placed {
        world,
        library,
        prefab,
        root,
        barrel,
        sight,
    }
}

/// A turret instance placed in a scene that already holds other entities:
/// its members sit at other indices than the prefab's expansion gives them.
fn placed_after_others() -> Placed {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(prefab, turret.record)]));
    let mut world = GameWorld::new();
    for index in 0..3 {
        world.spawn((Transform::identity(), Name::new(format!("Rock {index}"))));
    }
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the turret instantiates");
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let member = |inner: PersistentId| {
        world
            .inner_world()
            .entity_with_id(PersistentId::within(instance, inner))
            .expect("the member is known by its derived id")
    };
    let barrel = member(turret.barrel);
    let sight = member(turret.sight);
    Placed {
        world,
        library,
        prefab,
        root,
        barrel,
        sight,
    }
}

/// A scripted turret placed in an editor world: its root's script aims where
/// `aim` says in the world the prefab is made in — given the root and the
/// barrel — and gives a `range` of 5.
struct Scripted {
    world: GameWorld,
    library: Library,
    prefab: AssetUUID,
    root: EntityId,
    barrel: EntityId,
}

fn scripted(aim: impl FnOnce(&mut World, EntityId, EntityId) -> EntityId) -> Scripted {
    use khora_sdk::khora_core::script::ScriptValue;
    use khora_sdk::khora_data::ecs::Script;

    let mut made = World::new();
    let root = made.spawn((Transform::identity(), Name::new("Turret")));
    let barrel = made.spawn((Transform::identity(), Name::new("Barrel")));
    made.set_parent(barrel, Some(root));
    let target = aim(&mut made, root, barrel);
    made.add_component(
        root,
        Script::new("ai/turret.erg", "Turret")
            .with_field("aim", ScriptValue::Entity(target))
            .with_field("range", ScriptValue::Float(5.0)),
    )
    .expect("a script attaches");
    made.mark_authored(root).expect("authored");
    let barrel_id = made.mark_authored(barrel).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(
        prefab,
        capture_subtree(&made, root).expect("the turret captures"),
    )]));
    let mut world = GameWorld::new();
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the turret instantiates");
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let barrel = world
        .inner_world()
        .entity_with_id(PersistentId::within(instance, barrel_id))
        .expect("the barrel is there");
    Scripted {
        world,
        library,
        prefab,
        root,
        barrel,
    }
}

/// The `range` the script of `entity` gives.
fn script_range(world: &GameWorld, entity: EntityId) -> Option<f32> {
    use khora_sdk::khora_core::script::ScriptValue;
    use khora_sdk::khora_data::ecs::Script;

    world
        .get_component::<Script>(entity)?
        .fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("range", ScriptValue::Float(range)) => Some(*range),
            _ => None,
        })
}

impl Placed {
    /// The inspected view of `entity` against its prefab.
    fn inspect(&self, entity: EntityId) -> InspectedPrefab {
        inspect_prefab(&self.world, entity, &self.library)
            .unwrap_or_else(|| panic!("{entity:?} belongs to no instance"))
    }

    fn translation_mut(&mut self, entity: EntityId) -> &mut Vec3 {
        &mut self
            .world
            .get_component_mut::<Transform>(entity)
            .expect("placed")
            .translation
    }
}

/// The live JSON of `type_name` on `entity`, as the inspector reads it.
fn component_json(world: &GameWorld, entity: EntityId, type_name: &str) -> Option<Value> {
    inventory::iter::<khora_sdk::ComponentRegistration>
        .into_iter()
        .find(|reg| reg.type_name == type_name)
        .and_then(|reg| (reg.to_json)(world.inner_world(), entity))
}

/// The prefab's value of `type_name` in `inspected`, if the prefab gives it.
fn prefab_value<'a>(inspected: &'a InspectedPrefab, type_name: &str) -> Option<&'a Value> {
    inspected
        .prefab_json
        .iter()
        .find(|(name, _)| name == type_name)
        .map(|(_, value)| value)
}

/// `steps` as a path.
fn path(steps: &[&str]) -> Vec<String> {
    steps.iter().map(|step| (*step).to_owned()).collect()
}

/// `paths`, sorted — the order paths are reported in is not part of the
/// contract.
fn sorted(mut paths: Vec<Vec<String>>) -> Vec<Vec<String>> {
    paths.sort();
    paths
}

/// Applies `edits` as the inspector's next frame does.
fn apply(world: &mut GameWorld, edits: Vec<PropertyEdit>) {
    let mut state = EditorState::default();
    for edit in edits {
        state.push_edit(edit);
    }
    crate::ops::apply_edits(world, &mut state);
}
