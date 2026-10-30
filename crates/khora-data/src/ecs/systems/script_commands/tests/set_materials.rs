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

//! `e.SetMaterial(m)` at the frame boundary: the entity's `MaterialRef`
//! becomes a reference to the material asset — replaced if it had one, added
//! if not — and the asset resolver loads it at the next `PreExtract`.

use khora_core::asset::{AssetUUID, StandardMaterial};
use khora_core::lane::OutputDeck;
use khora_core::script::{CommandBuffer, WorldCommand};
use khora_core::Runtime;

use super::super::apply_script_commands;
use crate::ecs::{MaterialRef, Transform, World};

/// One frame of the `script_commands` system, its deck holding `commands`.
fn frame(world: &mut World, commands: Vec<WorldCommand>) {
    let mut deck = OutputDeck::new();
    deck.slot::<CommandBuffer>().extend(commands);
    apply_script_commands(
        world,
        &Runtime::default(),
        &mut deck,
        crate::ecs::SystemContext {
            since: crate::ecs::Tick(0),
        },
    );
}

/// The asset `entity`'s material references, if it references one.
fn material_asset(world: &World, entity: khora_core::ecs::entity::EntityId) -> Option<AssetUUID> {
    match world.get::<MaterialRef>(entity) {
        Some(MaterialRef::Asset(uuid)) => Some(*uuid),
        _ => None,
    }
}

/// **A material replaces the entity's**: an inline material becomes a
/// reference to the asset.
#[test]
fn setting_a_material_replaces_the_entitys_material() {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::identity(),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    let stone = AssetUUID::new();

    frame(
        &mut world,
        vec![WorldCommand::SetMaterial {
            entity,
            material: stone,
        }],
    );

    assert_eq!(material_asset(&world, entity), Some(stone));
}

/// A second material replaces the first.
#[test]
fn setting_a_material_again_replaces_the_asset() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), MaterialRef::Asset(AssetUUID::new())));
    let stone = AssetUUID::new();

    frame(
        &mut world,
        vec![WorldCommand::SetMaterial {
            entity,
            material: stone,
        }],
    );

    assert_eq!(material_asset(&world, entity), Some(stone));
}

/// **An entity with no material is given one.**
#[test]
fn setting_a_material_adds_one_when_the_entity_has_none() {
    let mut world = World::new();
    let entity = world.spawn(Transform::identity());
    let stone = AssetUUID::new();

    frame(
        &mut world,
        vec![WorldCommand::SetMaterial {
            entity,
            material: stone,
        }],
    );

    assert_eq!(material_asset(&world, entity), Some(stone));
}

/// A material for an entity that is gone changes nothing, and the batch goes
/// on: the next command applies.
#[test]
fn setting_a_material_on_a_despawned_entity_changes_nothing() {
    let mut world = World::new();
    let gone = world.spawn(Transform::identity());
    let kept = world.spawn(Transform::identity());
    world.despawn(gone);
    let stone = AssetUUID::new();

    frame(
        &mut world,
        vec![
            WorldCommand::SetMaterial {
                entity: gone,
                material: stone,
            },
            WorldCommand::SetMaterial {
                entity: kept,
                material: stone,
            },
        ],
    );

    assert_eq!(world.entity_count(), 1);
    assert_eq!(material_asset(&world, kept), Some(stone));
}

/// **A material set on a prefab spawn's root, in the same batch, lands on
/// the instance's root** — the spawn applies first.
#[test]
fn setting_a_material_on_a_prefab_spawns_root_in_the_same_batch() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use khora_core::math::Vec3;

    use crate::ecs::Name;
    use crate::scene::{capture_subtree, PrefabLibrary, PrefabSource, SceneRecord};

    struct One(HashMap<AssetUUID, SceneRecord>);
    impl PrefabSource for One {
        fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
            self.0.get(&id).cloned().ok_or_else(|| "none".to_owned())
        }
    }

    let prefab = AssetUUID::new();
    let mut source = World::new();
    let root_in_prefab = source.spawn((Transform::identity(), Name("Crate".to_owned())));
    source.mark_authored(root_in_prefab).expect("authored");
    let record = capture_subtree(&source, root_in_prefab).expect("captures");
    let mut runtime = Runtime::default();
    runtime
        .resources
        .insert(PrefabLibrary(Arc::new(One(HashMap::from([(
            prefab, record,
        )])))));
    let mut world = World::new();
    let root = world.reserver().reserve();
    let stone = AssetUUID::new();

    let mut deck = OutputDeck::new();
    deck.slot::<CommandBuffer>().extend(vec![
        WorldCommand::SpawnPrefab {
            entity: root,
            prefab,
            position: Vec3::ZERO,
            rotation: None,
            components: Vec::new(),
        },
        WorldCommand::SetMaterial {
            entity: root,
            material: stone,
        },
    ]);
    apply_script_commands(
        &mut world,
        &runtime,
        &mut deck,
        crate::ecs::SystemContext {
            since: crate::ecs::Tick(0),
        },
    );

    assert!(world.contains(root));
    assert_eq!(material_asset(&world, root), Some(stone));
}
