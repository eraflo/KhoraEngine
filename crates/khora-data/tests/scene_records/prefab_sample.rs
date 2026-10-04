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

//! The prefabs the prefab tests build on: a library held in memory, a turret
//! prefab, a tower prefab holding a turret, and the moves an author makes.

use std::collections::HashMap;

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Light, Name, Script, Transform};
use khora_data::scene::{
    capture_subtree, collapse, expand, instantiate_prefab, PrefabSource, SaveError,
};

use super::*;

/// Prefabs held in memory, by asset id — what a project's asset service is
/// to a running editor.
#[derive(Debug, Clone, Default)]
pub(super) struct Library(HashMap<AssetUUID, SceneRecord>);

impl Library {
    /// The library, with `record` as the prefab known as `id`.
    pub(super) fn with(mut self, id: AssetUUID, record: SceneRecord) -> Self {
        self.0.insert(id, record);
        self
    }
}

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id:?} in the library"))
    }
}

/// A turret prefab: its root (placed, named, lit, with a script aiming at
/// the sight) and two children, a barrel and a sight — by their ids in the
/// prefab.
pub(super) struct Turret {
    pub(super) record: SceneRecord,
    pub(super) root: PersistentId,
    pub(super) barrel: PersistentId,
    pub(super) sight: PersistentId,
}

pub(super) fn turret() -> Turret {
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
    world
        .add_component(
            root,
            Script::new("ai/turret.erg", "Turret").with_field("aim", ScriptValue::Entity(sight)),
        )
        .expect("a script attaches");
    for entity in [root, barrel, sight] {
        world.mark_authored(entity).expect("authored");
    }
    let record = capture_subtree(&world, root).expect("the turret captures");
    Turret {
        record,
        root: id(&world, root),
        barrel: id(&world, barrel),
        sight: id(&world, sight),
    }
}

/// A tower prefab: a root holding an instance of the turret prefab, raised
/// on the tower — an override the tower makes on its nested turret.
pub(super) struct Tower {
    /// The tower's record, as a prefab file holds it: the turret a link.
    pub(super) record: SceneRecord,
    /// The nested turret's root, by its id in the tower.
    pub(super) turret_root: PersistentId,
}

pub(super) fn tower(turret_prefab: AssetUUID, library: &Library) -> Tower {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Tower")));
    world.mark_authored(root).expect("authored");
    let turret = instantiate_prefab(&mut world, turret_prefab, library)
        .unwrap_or_else(|e| panic!("the turret does not instantiate: {e}"));
    world.set_parent(turret, Some(root));
    world
        .get_mut::<Transform>(turret)
        .expect("placed")
        .translation = Vec3::new(0.0, 5.0, 0.0);
    let record = collapse(
        &capture_subtree(&world, root).expect("the tower captures"),
        library,
    )
    .unwrap_or_else(|e| panic!("the tower does not collapse: {e}"));
    Tower {
        record,
        turret_root: id(&world, turret),
    }
}

/// `prefab` — a record holding no instance — as its author leaves it after
/// `edit`: opened with its own ids, edited, captured again from its root.
pub(super) fn edited<T>(
    prefab: &SceneRecord,
    edit: impl FnOnce(&mut World) -> T,
) -> (SceneRecord, T) {
    let mut world = World::new();
    apply(&mut world, prefab, Identity::Keep).expect("the prefab opens");
    let made = edit(&mut world);
    let root = entity(&world, prefab.entities[0]);
    let record = capture_subtree(&world, root).expect("the prefab captures");
    (record, made)
}

/// `world` written down as a scene: captured, its instances collapsed.
pub(super) fn save(world: &World, prefabs: &dyn PrefabSource) -> SceneRecord {
    let captured = capture_world(world).expect("the scene captures");
    collapse(&captured, prefabs)
        .unwrap_or_else(|e: SaveError| panic!("the scene does not collapse: {e}"))
}

/// `scene` expanded from `prefabs` and opened in a world of its own, every
/// identity kept.
pub(super) fn open(scene: &SceneRecord, prefabs: &dyn PrefabSource) -> World {
    let expanded =
        expand(scene, prefabs).unwrap_or_else(|e| panic!("the scene does not expand: {e}"));
    assert!(
        expanded.instances.is_empty(),
        "an expanded record still links to prefabs: {:?}",
        expanded.instances
    );
    let mut world = World::new();
    apply(&mut world, &expanded, Identity::Keep)
        .unwrap_or_else(|e| panic!("the expanded scene does not load: {e}"));
    world
}

pub(super) fn id(world: &World, entity: EntityId) -> PersistentId {
    world
        .persistent_id(entity)
        .expect("a live entity has an id")
}

/// The entity `world` knows by `id`.
pub(super) fn entity(world: &World, id: PersistentId) -> EntityId {
    world
        .entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the world is {id:?}"))
}

pub(super) fn translation(world: &World, entity: EntityId) -> Vec3 {
    world
        .get::<Transform>(entity)
        .expect("the entity is placed")
        .translation
}

pub(super) fn name(world: &World, entity: EntityId) -> String {
    world
        .get::<Name>(entity)
        .expect("the entity is named")
        .as_str()
        .to_owned()
}

/// The entity `world` parents `entity` to, if any.
pub(super) fn parent(world: &World, entity: EntityId) -> Option<EntityId> {
    world.get::<khora_data::ecs::Parent>(entity).map(|p| p.0)
}

/// Whether any page of `record` holds a row for `id`.
pub(super) fn has_row(record: &SceneRecord, id: PersistentId) -> bool {
    record.pages.iter().any(|page| page.rows.contains(&id))
}
