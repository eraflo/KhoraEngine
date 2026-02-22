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

use crate::scene_lane::strategies::{
    DeserializationError, SerializationError, SerializationStrategy,
};
use bincode::{config, Decode};
use khora_core::lane::Lane;
use khora_core::{ecs::entity::EntityId, graph::topological_sort};
use khora_data::{
    ecs::{Component, Parent, SerializableParent, SerializableTransform, Transform, World},
    scene::{SceneCommand, SceneRecipe},
};
use std::{any, collections::HashMap};

// --- Deserialization Registry ---

// A function pointer that takes bytes, deserializes them into a stable representation,
// converts that into a live component, and adds it to the world.
type ComponentDeserializer =
    Box<dyn Fn(&mut World, EntityId, &[u8]) -> Result<(), DeserializationError> + Send + Sync>;

/// A registry to map component type names to their deserialization functions.
///
/// This provides a form of reflection, allowing the `Lane` to deserialize different
/// component types dynamically based on a string name.
#[derive(Default)]
struct DeserializerRegistry {
    map: HashMap<String, ComponentDeserializer>,
}

impl DeserializerRegistry {
    /// Creates a new registry and registers all supported component types.
    fn new() -> Self {
        let mut registry = Self::default();

        // For each component, we register how to convert its `Serializable` form
        // back into the live `Component` form.
        registry.register::<Transform, SerializableTransform>(|st| Transform {
            translation: st.translation,
            rotation: st.rotation,
            scale: st.scale,
        });
        registry.register::<Parent, SerializableParent>(|sp| Parent(sp.0));

        registry
    }

    /// Registers a component type for deserialization.
    ///
    /// # Type Parameters
    /// * `C`: The live component type (e.g., `khora_data::ecs::Transform`).
    /// * `S`: The stable, serializable representation (e.g., `SerializableTransform`).
    ///
    /// # Arguments
    /// * `from_serializable`: A function that converts the stable representation into the live one.
    fn register<C, S>(&mut self, from_serializable: fn(S) -> C)
    where
        C: Component,
        S: Decode<()> + 'static,
    {
        let type_name = any::type_name::<C>().to_string();
        self.map.insert(
            type_name,
            Box::new(move |world, entity_id, data| {
                let (serializable_component, _): (S, _) =
                    bincode::decode_from_slice_with_context(data, config::standard(), ())
                        .map_err(|e| DeserializationError::InvalidFormat(e.to_string()))?;
                let live_component = from_serializable(serializable_component);
                world.add_component(entity_id, live_component).ok();
                Ok(())
            }),
        );
    }

    /// Retrieves the deserializer function for a given component type name.
    fn get(&self, type_name: &str) -> Option<&ComponentDeserializer> {
        self.map.get(type_name)
    }
}

// --- The Lane ---

/// A serialization strategy that uses a sequence of commands (`SceneRecipe`).
///
/// This lane is designed for flexibility. The command-based format is a good
/// foundation for editor tools, scene patching, and potential streaming in the future.
pub struct RecipeSerializationLane {
    deserializers: DeserializerRegistry,
}

impl Default for RecipeSerializationLane {
    fn default() -> Self {
        Self {
            deserializers: DeserializerRegistry::new(),
        }
    }
}

impl RecipeSerializationLane {
    /// Creates a new instance of the `RecipeSerializationLane`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl khora_core::lane::Lane for RecipeSerializationLane {
    fn strategy_name(&self) -> &'static str {
        "KH_RECIPE_V1"
    }

    fn lane_kind(&self) -> khora_core::lane::LaneKind {
        khora_core::lane::LaneKind::Scene
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl SerializationStrategy for RecipeSerializationLane {
    fn get_strategy_id(&self) -> &'static str {
        self.strategy_name()
    }

    // --- SERIALIZATION ---
    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        let mut commands = Vec::new();

        // 1. Collect nodes (all living entities) and edges (Parent->Child relationships).
        let nodes: Vec<EntityId> = world.iter_entities().collect();
        let mut edges: Vec<(EntityId, EntityId)> = Vec::new();
        for &entity_id in &nodes {
            if let Some(parent) = world.get::<Parent>(entity_id) {
                // The edge goes from the parent (source) to the child (destination).
                edges.push((parent.0, entity_id));
            }
        }

        // 2. Call our generic topological sort utility from khora-core.
        let sorted_entities = topological_sort(nodes, edges).map_err(|_| {
            SerializationError::ProcessingFailed("Cycle detected in scene hierarchy.".to_string())
        })?;

        // 3. Generate the commands in the guaranteed topological order.
        for entity_id in sorted_entities {
            // Spawn command
            commands.push(SceneCommand::Spawn { id: entity_id });

            // AddComponent commands for each serializable component.
            if let Some(transform) = world.get::<Transform>(entity_id) {
                // First, convert the live component to its stable, serializable form.
                let serializable = SerializableTransform {
                    translation: transform.translation,
                    rotation: transform.rotation,
                    scale: transform.scale,
                };
                commands.push(SceneCommand::AddComponent {
                    entity_id,
                    component_type: any::type_name::<Transform>().to_string(),
                    // Then, serialize the stable form.
                    component_data: bincode::encode_to_vec(serializable, config::standard())
                        .unwrap(),
                });
            }
            if let Some(parent) = world.get::<Parent>(entity_id) {
                let serializable = SerializableParent(parent.0);
                commands.push(SceneCommand::AddComponent {
                    entity_id,
                    component_type: any::type_name::<Parent>().to_string(),
                    component_data: bincode::encode_to_vec(serializable, config::standard())
                        .unwrap(),
                });
            }
        }

        let scene_recipe = SceneRecipe { commands };
        bincode::encode_to_vec(&scene_recipe, config::standard())
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    // --- DESERIALIZATION ---
    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        let (recipe, _): (SceneRecipe, _) = bincode::decode_from_slice(data, config::standard())
            .map_err(|e| DeserializationError::InvalidFormat(e.to_string()))?;

        let mut id_map = HashMap::<EntityId, EntityId>::new();

        // Execute each command sequentially.
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
                    // Use the id_map to find the entity's ID in the new world.
                    if let Some(new_id) = id_map.get(&entity_id) {
                        // Use the registry to find the correct deserialization function.
                        if let Some(deserializer) = self.deserializers.get(&component_type) {
                            deserializer(world, *new_id, &component_data)?;
                        }
                    }
                }
                SceneCommand::SetParent { .. } => {
                    // This variant is not used in our current serialization implementation.
                }
            }
        }

        Ok(())
    }
}
