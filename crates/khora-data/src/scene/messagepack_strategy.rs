// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! MessagePack scene strategy — portable schema-less binary.
//!
//! Same shape as [`super::RecipeSerializationStrategy`] (a topo-sorted
//! [`super::SceneRecipe`]) but encoded with `rmp-serde` so any language
//! with a MessagePack reader can consume it. Slightly larger on disk
//! than bincode but the broad ecosystem support makes it the natural
//! "interop" format for asset pipeline tooling outside Rust.

use super::{DeserializationError, SerializationError, SerializationStrategy};
use crate::{
    ecs::World,
    scene::{registry::ComponentRegistration, SceneCommand, SceneRecipe},
};
use khora_core::{ecs::entity::EntityId, graph::topological_sort};
use std::collections::HashMap;

/// MessagePack-encoded `SceneRecipe`. Identifier `KH_MESSAGEPACK_V1`.
pub struct MessagePackSerializationStrategy;

impl Default for MessagePackSerializationStrategy {
    fn default() -> Self {
        Self
    }
}

impl MessagePackSerializationStrategy {
    /// Constructs a fresh `MessagePackSerializationStrategy`. The type
    /// has no state — the constructor exists for parity with the other
    /// strategies and `Default`.
    pub fn new() -> Self {
        Self
    }
}

impl SerializationStrategy for MessagePackSerializationStrategy {
    fn get_strategy_id(&self) -> &'static str {
        "KH_MESSAGEPACK_V1"
    }

    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        let mut commands = Vec::new();

        // Same topological-sort + walk as Recipe — MessagePack only
        // changes the wire format.
        let nodes: Vec<EntityId> = world.iter_entities().collect();
        let mut edges: Vec<(EntityId, EntityId)> = Vec::new();
        for &entity_id in &nodes {
            if let Some(parent) = world.get::<crate::ecs::Parent>(entity_id) {
                edges.push((parent.0, entity_id));
            }
        }
        let sorted_entities = topological_sort(nodes, edges).map_err(|_| {
            SerializationError::ProcessingFailed("Cycle detected in scene hierarchy.".to_string())
        })?;

        for entity_id in sorted_entities {
            commands.push(SceneCommand::Spawn { id: entity_id });
            for reg in inventory::iter::<ComponentRegistration> {
                if let Some(data) = (reg.serialize_recipe)(world, entity_id) {
                    commands.push(SceneCommand::AddComponent {
                        entity_id,
                        component_type: reg.type_name.to_string(),
                        component_data: data,
                    });
                }
            }
            if let Some(parent) = world.get::<crate::ecs::Parent>(entity_id) {
                commands.push(SceneCommand::SetParent {
                    child_id: entity_id,
                    parent_id: parent.0,
                });
            }
        }

        let scene_recipe = SceneRecipe { commands };
        rmp_serde::to_vec_named(&scene_recipe)
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        let recipe: SceneRecipe = rmp_serde::from_slice(data)
            .map_err(|e| DeserializationError::InvalidFormat(e.to_string()))?;

        let mut id_map = HashMap::<EntityId, EntityId>::new();

        for command in recipe.commands {
            match command {
                SceneCommand::Spawn { id } => {
                    let new_id = world.spawn(());
                    id_map.insert(id, new_id);
                }
                SceneCommand::AddComponent {
                    entity_id,
                    component_type,
                    component_data,
                } => {
                    let live_id = id_map.get(&entity_id).copied().ok_or_else(|| {
                        DeserializationError::WorldPopulationFailed(format!(
                            "Recipe AddComponent for unspawned entity {:?}",
                            entity_id
                        ))
                    })?;
                    let reg = inventory::iter::<ComponentRegistration>
                        .into_iter()
                        .find(|r| r.type_name == component_type.as_str())
                        .ok_or_else(|| {
                            DeserializationError::WorldPopulationFailed(format!(
                                "Unknown component type '{}' in recipe",
                                component_type
                            ))
                        })?;
                    (reg.deserialize_recipe)(world, live_id, &component_data).map_err(|e| {
                        DeserializationError::WorldPopulationFailed(format!(
                            "Failed to apply component '{}': {:?}",
                            component_type, e
                        ))
                    })?;
                }
                SceneCommand::SetParent {
                    child_id,
                    parent_id,
                } => {
                    let child = id_map.get(&child_id).copied().ok_or_else(|| {
                        DeserializationError::WorldPopulationFailed(format!(
                            "Recipe SetParent unknown child {:?}",
                            child_id
                        ))
                    })?;
                    let parent = id_map.get(&parent_id).copied().ok_or_else(|| {
                        DeserializationError::WorldPopulationFailed(format!(
                            "Recipe SetParent unknown parent {:?}",
                            parent_id
                        ))
                    })?;
                    let _ = world.add_component(child, crate::ecs::Parent(parent));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{GlobalTransform, Transform, World};
    use khora_core::math::Vec3;

    #[test]
    fn round_trip_preserves_transform() {
        let mut src = World::new();
        let t = Transform {
            translation: Vec3::new(7.0, 0.0, -3.0),
            ..Default::default()
        };
        src.spawn((t, GlobalTransform::identity()));

        let strat = MessagePackSerializationStrategy::new();
        let bytes = strat.serialize(&src).unwrap();
        assert!(!bytes.is_empty());

        let mut dst = World::new();
        strat.deserialize(&bytes, &mut dst).unwrap();

        let mut q = dst.query::<&Transform>();
        let got = q.next().expect("at least one entity restored");
        assert_eq!(*got, t);
    }
}
