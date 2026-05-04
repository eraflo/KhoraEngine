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

//! Implements the "Recipe" serialization strategy for scenes.
//!
//! Uses `inventory`-based component registration for open serialization.
//! All component types that derive `Component` are automatically handled.

use super::{DeserializationError, SerializationError, SerializationStrategy};
use crate::{
    ecs::World,
    scene::{registry::ComponentRegistration, SceneCommand, SceneRecipe},
};
use bincode::config;
use khora_core::{ecs::entity::EntityId, graph::topological_sort};
use std::collections::HashMap;

/// A serialization strategy that uses a sequence of commands (`SceneRecipe`).
///
/// This strategy iterates all registered components via `inventory` to
/// serialize every component on every entity. On deserialization, it
/// looks up the matching registration by type name and decodes.
pub struct RecipeSerializationStrategy;

impl Default for RecipeSerializationStrategy {
    fn default() -> Self {
        Self
    }
}

impl RecipeSerializationStrategy {
    /// Creates a new instance of the `RecipeSerializationStrategy`.
    pub fn new() -> Self {
        Self
    }
}

impl SerializationStrategy for RecipeSerializationStrategy {
    fn get_strategy_id(&self) -> &'static str {
        "KH_RECIPE_V1"
    }

    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        let mut commands = Vec::new();

        // 1. Collect nodes and edges for topological sort.
        let nodes: Vec<EntityId> = world.iter_entities().collect();
        let mut edges: Vec<(EntityId, EntityId)> = Vec::new();

        // Direct Parent access for topological sort (Parent is always registered).
        for &entity_id in &nodes {
            if let Some(parent) = world.get::<crate::ecs::Parent>(entity_id) {
                edges.push((parent.0, entity_id));
            }
        }

        // 2. Topological sort.
        let sorted_entities = topological_sort(nodes, edges).map_err(|_| {
            SerializationError::ProcessingFailed("Cycle detected in scene hierarchy.".to_string())
        })?;

        // 3. For each entity, iterate ALL component registrations and emit commands.
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

            // Emit SetParent command for hierarchy reconstruction.
            if let Some(parent) = world.get::<crate::ecs::Parent>(entity_id) {
                commands.push(SceneCommand::SetParent {
                    child_id: entity_id,
                    parent_id: parent.0,
                });
            }
        }

        let scene_recipe = SceneRecipe { commands };
        bincode::encode_to_vec(&scene_recipe, config::standard())
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        let (recipe, _): (SceneRecipe, _) = bincode::decode_from_slice(data, config::standard())
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
                    if let Some(new_id) = id_map.get(&entity_id) {
                        // Look up the registration by type_name.
                        for reg in inventory::iter::<ComponentRegistration> {
                            if reg.type_name == component_type {
                                if let Err(e) =
                                    (reg.deserialize_recipe)(world, *new_id, &component_data)
                                {
                                    log::warn!("Failed to deserialize {}: {}", component_type, e);
                                }
                                break;
                            }
                        }
                    }
                }
                SceneCommand::SetParent {
                    child_id,
                    parent_id,
                } => {
                    if let (Some(&new_child), Some(&new_parent)) =
                        (id_map.get(&child_id), id_map.get(&parent_id))
                    {
                        world
                            .add_component(new_child, crate::ecs::Parent(new_parent))
                            .ok();
                    }
                }
            }
        }

        Ok(())
    }
}
