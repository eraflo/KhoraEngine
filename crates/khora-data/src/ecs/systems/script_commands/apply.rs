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

//! Performing one command.
//!
//! Every failure here is a script's mistake, never the engine's, so none of them
//! panics and none of them stops the batch. A behavior that writes to a
//! despawned entity gets one named diagnostic and the rest of the game carries
//! on — `RULES.md` §4 forbids `unwrap` on fallible work, and a faulty script is
//! exactly the fallible input this boundary exists to absorb.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ComponentName, ScriptValue, WorldCommand};

use crate::ecs::{Transform, World};
use crate::scene::registry::registration_of;

use super::json::{merge, to_json};

/// Why a command could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The entity is gone — despawned earlier in this same batch, or in an
    /// earlier frame while the script kept the handle.
    NoSuchEntity(EntityId),
    /// The entity has no `Transform` to move, turn or resize.
    NoTransform(EntityId),
    /// The component is not on the entity, so there is nothing to write to.
    NotAttached {
        /// The entity.
        entity: EntityId,
        /// The component that is missing.
        component: String,
    },
    /// The component is already on the entity.
    AlreadyAttached {
        /// The entity.
        entity: EntityId,
        /// The component that is already there.
        component: String,
    },
    /// No component type goes by that name.
    UnknownComponent(String),
    /// The parent is already below the child, so the edge would close a loop.
    WouldCycle {
        /// The child being re-parented.
        entity: EntityId,
        /// The proposed parent.
        parent: EntityId,
    },
    /// The component exists but refused the value.
    Rejected {
        /// The component.
        component: String,
        /// What went wrong, from the component's own mirror.
        reason: String,
    },
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchEntity(entity) => {
                write!(
                    f,
                    "entity {}v{} no longer exists",
                    entity.index, entity.generation
                )
            }
            Self::NoTransform(entity) => write!(
                f,
                "entity {}v{} has no Transform to move",
                entity.index, entity.generation
            ),
            Self::NotAttached { entity, component } => write!(
                f,
                "entity {}v{} has no {component} — add it before writing to it",
                entity.index, entity.generation
            ),
            Self::AlreadyAttached { entity, component } => write!(
                f,
                "entity {}v{} already has a {component}",
                entity.index, entity.generation
            ),
            Self::UnknownComponent(name) => {
                write!(f, "no component type is named {name}")
            }
            Self::WouldCycle { entity, parent } => write!(
                f,
                "entity {}v{} cannot be parented to {}v{} — it is already above it",
                entity.index, entity.generation, parent.index, parent.generation
            ),
            Self::Rejected { component, reason } => {
                write!(f, "{component} refused the value: {reason}")
            }
        }
    }
}

/// Applies one command to the world.
pub(super) fn apply(world: &mut World, command: &WorldCommand) -> Result<(), ApplyError> {
    match command {
        WorldCommand::SetTranslation { entity, value } => {
            transform_mut(world, *entity).map(|t| t.translation = *value)
        }
        WorldCommand::Translate { entity, delta } => {
            transform_mut(world, *entity).map(|t| t.translation = t.translation + *delta)
        }
        WorldCommand::SetRotation { entity, value } => {
            transform_mut(world, *entity).map(|t| t.rotation = *value)
        }
        WorldCommand::SetScale { entity, value } => {
            transform_mut(world, *entity).map(|t| t.scale = *value)
        }
        WorldCommand::SetParent { entity, parent } => set_parent(world, *entity, *parent),
        WorldCommand::Spawn {
            position,
            rotation,
            components,
        } => spawn(world, *position, *rotation, components),
        WorldCommand::Despawn { entity } => {
            // Recursive, so a guard holding a weapon as a child does not leave
            // the weapon floating where it died.
            match world.despawn_subtree(*entity) {
                0 => Err(ApplyError::NoSuchEntity(*entity)),
                _ => Ok(()),
            }
        }
        WorldCommand::SetComponent {
            entity,
            component,
            value,
        } => set_component(world, *entity, component.as_str(), value),
        WorldCommand::AddComponent {
            entity,
            component,
            value,
        } => add_component(world, *entity, component.as_str(), value),
        WorldCommand::RemoveComponent { entity, component } => {
            remove_component(world, *entity, component.as_str())
        }
    }
}

/// The local `Transform` — the authored one, not the derived `GlobalTransform`.
///
/// `transform_propagation` recomputes the global from this, one phase per frame,
/// which is the same latency a physics writeback already has. Writing the global
/// directly would be overwritten on the next propagation anyway.
fn transform_mut(world: &mut World, entity: EntityId) -> Result<&mut Transform, ApplyError> {
    if !world.contains(entity) {
        return Err(ApplyError::NoSuchEntity(entity));
    }
    world
        .get_mut::<Transform>(entity)
        .ok_or(ApplyError::NoTransform(entity))
}

fn set_parent(
    world: &mut World,
    entity: EntityId,
    parent: Option<EntityId>,
) -> Result<(), ApplyError> {
    if !world.contains(entity) {
        return Err(ApplyError::NoSuchEntity(entity));
    }
    if let Some(parent) = parent {
        if !world.contains(parent) {
            return Err(ApplyError::NoSuchEntity(parent));
        }
    }
    // The hierarchy invariant lives on `World` — both halves of the edge are
    // written there, whether the caller is a script, the editor or a scene load.
    if world.set_parent(entity, parent) {
        Ok(())
    } else {
        Err(ApplyError::WouldCycle {
            entity,
            parent: parent.unwrap_or(entity),
        })
    }
}

fn spawn(
    world: &mut World,
    position: Vec3,
    rotation: Quaternion,
    components: &[(ComponentName, ScriptValue)],
) -> Result<(), ApplyError> {
    let entity = world.spawn(Transform::new(position, rotation, Vec3::ONE));

    // A component that fails to attach leaves a half-built entity, so the spawn
    // is rolled back rather than left in the scene: a projectile with no damage
    // is harder to notice than one that never appeared.
    for (component, value) in components {
        if let Err(error) = add_component(world, entity, component.as_str(), value) {
            world.despawn(entity);
            return Err(error);
        }
    }
    Ok(())
}

fn set_component(
    world: &mut World,
    entity: EntityId,
    component: &str,
    value: &ScriptValue,
) -> Result<(), ApplyError> {
    let registration = lookup(world, entity, component)?;

    // Read what is there, merge the write onto it, put it back. The merge is
    // what makes a one-field write mean "change this field" rather than "reset
    // every field the script did not mention".
    let current = (registration.to_json)(world, entity).ok_or_else(|| ApplyError::NotAttached {
        entity,
        component: component.to_owned(),
    })?;
    let patch = encode(component, value)?;

    (registration.from_json)(world, entity, &merge(current, patch)).map_err(|reason| {
        ApplyError::Rejected {
            component: component.to_owned(),
            reason,
        }
    })
}

fn add_component(
    world: &mut World,
    entity: EntityId,
    component: &str,
    value: &ScriptValue,
) -> Result<(), ApplyError> {
    let registration = lookup(world, entity, component)?;
    if (registration.to_json)(world, entity).is_some() {
        return Err(ApplyError::AlreadyAttached {
            entity,
            component: component.to_owned(),
        });
    }

    // Start from the component's own default so a script only has to name the
    // fields it cares about, then apply its value as a patch over that.
    (registration.create_default)(world, entity).map_err(|reason| ApplyError::Rejected {
        component: component.to_owned(),
        reason,
    })?;

    if matches!(value, ScriptValue::Unit) {
        return Ok(());
    }
    set_component(world, entity, component, value)
}

fn remove_component(
    world: &mut World,
    entity: EntityId,
    component: &str,
) -> Result<(), ApplyError> {
    let registration = lookup(world, entity, component)?;
    (registration.remove)(world, entity).map_err(|reason| ApplyError::Rejected {
        component: component.to_owned(),
        reason,
    })
}

fn lookup(
    world: &World,
    entity: EntityId,
    component: &str,
) -> Result<&'static crate::scene::registry::ComponentRegistration, ApplyError> {
    if !world.contains(entity) {
        return Err(ApplyError::NoSuchEntity(entity));
    }
    registration_of(component).ok_or_else(|| ApplyError::UnknownComponent(component.to_owned()))
}

fn encode(component: &str, value: &ScriptValue) -> Result<serde_json::Value, ApplyError> {
    to_json(value).map_err(|reason| ApplyError::Rejected {
        component: component.to_owned(),
        reason,
    })
}
