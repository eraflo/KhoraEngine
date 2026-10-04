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

//! An instance's references written back into its prefab: to a member, to
//! the scene, to an entity of the author's — the prefab names only what it
//! holds.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Light, Name, Script, Transform};
use khora_data::scene::{apply_to_prefab, capture_subtree, instantiate_prefab, InstanceOf};

use super::prefab_apply::{component, placed};
use super::prefab_sample::*;
use super::*;

/// The entity the `aim` field of `entity`'s script names, if it names one.
fn aim(world: &World, entity: EntityId) -> Option<EntityId> {
    let script = world.get::<Script>(entity)?;
    script
        .fields
        .iter()
        .find(|(name, _)| name == "aim")
        .and_then(|(_, value)| match value {
            ScriptValue::Entity(target) => Some(*target),
            _ => None,
        })
}

/// `entity`'s script, aiming at `target`.
fn aim_at(world: &mut World, entity: EntityId, target: EntityId) {
    let script = world.get_mut::<Script>(entity).expect("scripted");
    *script = script
        .clone()
        .with_field("aim", ScriptValue::Entity(target));
}

/// A member aiming at another member is applied as the prefab aiming at its
/// own entity: the barrel, by its id in the prefab.
#[test]
fn applying_a_reference_to_a_member_names_the_prefabs_own_entity() {
    let mut placed = placed();
    let barrel = placed.member(placed.turret.barrel);
    aim_at(&mut placed.world, placed.root, barrel);

    let written = placed.apply(component(placed.root, "Script"));

    let prefab = open(&written, &placed.library);
    assert_eq!(
        aim(&prefab, entity(&prefab, placed.turret.root)),
        Some(entity(&prefab, placed.turret.barrel))
    );
}

/// A member aiming at an entity of the scene is applied as aiming at
/// nothing of the prefab: the prefab cannot hold the scene's entity.
#[test]
fn applying_a_reference_to_a_scene_entity_names_none_of_the_prefab() {
    let mut placed = placed();
    let lone = placed
        .world
        .spawn((Transform::identity(), Name::new("Lone")));
    placed.world.mark_authored(lone).expect("authored");
    aim_at(&mut placed.world, placed.root, lone);

    let written = placed.apply(component(placed.root, "Script"));

    let prefab = open(&written, &placed.library);
    let target = aim(&prefab, entity(&prefab, placed.turret.root));
    assert!(
        target.is_none_or(|target| prefab.persistent_id(target).is_none()),
        "the prefab aims at one of its own: {target:?}"
    );
}

/// The prefab made from a turret the author built in the scene keeps that
/// turret's identities, and the turret stays in the scene. The author hangs
/// the scene's barrel under the instance and aims the instance at it: the
/// barrel is the author's entity, not the instance's, so the prefab aims at
/// nothing of its own — an id the scene and the prefab share is still the
/// scene's entity.
#[test]
fn applying_a_reference_to_an_author_entity_sharing_a_prefab_id_names_none_of_the_prefab() {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Turret"), Light::point()));
    let barrel = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Barrel"),
    ));
    world.set_parent(barrel, Some(root));
    world
        .add_component(root, Script::new("ai/turret.erg", "Turret"))
        .expect("a script attaches");
    let root_id = world.mark_authored(root).expect("authored");
    let barrel_id = world.mark_authored(barrel).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library::default().with(
        prefab,
        capture_subtree(&world, root).expect("the turret captures"),
    );
    let instance =
        instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    world.set_parent(barrel, Some(instance));
    aim_at(&mut world, instance, barrel);

    let written = apply_to_prefab(
        &world,
        &InstanceOf {
            root: instance,
            prefab,
        },
        &component(instance, "Script"),
        &library,
    )
    .unwrap_or_else(|e| panic!("the apply is refused: {e}"));

    let opened = open(&written, &library);
    assert_ne!(
        aim(&opened, entity(&opened, root_id)),
        Some(entity(&opened, barrel_id)),
        "the author's barrel became the prefab's own"
    );
}
