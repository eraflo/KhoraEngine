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

//! The commands that bring an asset into the world: a prefab instance at the
//! id a turn reserved, and a material on an entity.

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ComponentName, ScriptValue};

use crate::ecs::{MaterialRef, Transform, World};
use crate::scene::{instantiate_prefab_at, PrefabSource};

use super::apply::{add_component, key_of, set_component, ApplyError};

/// Brings `prefab` in as a linked instance whose root lands at `entity`,
/// stands at `placed.0`, turned by `placed.1` or as the prefab turns it, each
/// of `components` merged onto the root's or added where it has none. Any
/// failure leaves nothing of the instance.
pub(super) fn spawn_prefab(
    world: &mut World,
    prefabs: Option<&dyn PrefabSource>,
    entity: EntityId,
    prefab: AssetUUID,
    placed: (Vec3, Option<Quaternion>),
    components: &[(ComponentName, ScriptValue)],
) -> Result<(), ApplyError> {
    let refused = |reason: String| ApplyError::Prefab { prefab, reason };
    let prefabs = prefabs.ok_or_else(|| refused("this world reads no prefabs".to_owned()))?;
    let made = instantiate_prefab_at(world, entity, prefab, prefabs)
        .map_err(|failure| refused(failure.message))?;
    // Undone whole: every entity the prefab brought, whether or not it sits
    // under the root.
    let undo = |world: &mut World| {
        for entity in &made {
            world.despawn(*entity);
        }
    };

    let (position, rotation) = placed;
    match world.get_mut::<Transform>(entity) {
        Some(transform) => {
            transform.translation = position;
            if let Some(rotation) = rotation {
                transform.rotation = rotation;
            }
        }
        None => {
            let rotation = rotation.unwrap_or(Quaternion::IDENTITY);
            if world
                .add_component(entity, Transform::new(position, rotation, Vec3::ONE))
                .is_err()
            {
                undo(world);
                return Err(ApplyError::NoTransform(entity));
            }
        }
    }

    // A component the call names merges onto the prefab's, as a `Set` does;
    // one the prefab lacks is added. A refusal undoes the whole instance: a
    // bullet missing what its spawner gave it is harder to notice than one
    // that never appeared.
    for (component, value) in components {
        let written = key_of(world, entity, component.as_str()).and_then(|key| {
            if world.row(entity, key).is_some() {
                set_component(world, entity, component.as_str(), value)
            } else {
                add_component(world, entity, component.as_str(), value)
            }
        });
        if let Err(error) = written {
            undo(world);
            return Err(error);
        }
    }
    Ok(())
}

/// Gives `entity` the material `material`, replacing the one it has: the asset
/// resolver loads it at the next frame's extraction.
pub(super) fn set_material(
    world: &mut World,
    entity: EntityId,
    material: AssetUUID,
) -> Result<(), ApplyError> {
    if !world.contains(entity) {
        return Err(ApplyError::NoSuchEntity(entity));
    }
    if let Some(current) = world.get_mut::<MaterialRef>(entity) {
        *current = MaterialRef::Asset(material);
        return Ok(());
    }
    world
        .add_component(entity, MaterialRef::Asset(material))
        .map(|_| ())
        .map_err(|error| ApplyError::Rejected {
            component: "MaterialRef".to_owned(),
            reason: format!("{error:?}"),
        })
}
