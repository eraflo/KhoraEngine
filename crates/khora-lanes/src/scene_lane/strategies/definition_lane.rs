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

//! A serialization strategy that uses a stable, intermediate representation (`SceneDefinition`).

use crate::scene_lane::strategies::{
    DeserializationError, SerializationError, SerializationStrategy,
};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::Lane;
use khora_data::{
    ecs::{GlobalTransform, Parent, SerializableParent, SerializableTransform, Transform, World},
    scene::{ComponentDefinition, EntityDefinition, SceneDefinition},
};
use std::collections::HashMap;

/// A serialization strategy that uses a stable, intermediate representation (`SceneDefinition`).
///
/// This lane is designed for long-term stability and human-readability. It translates
/// the live world state into a decoupled format, ensuring that changes to the internal
/// component structures do not break scene files.
#[derive(Default)]
pub struct DefinitionSerializationLane;

impl DefinitionSerializationLane {
    /// Creates a new instance of the DefinitionSerializationLane.
    pub fn new() -> Self {
        Self
    }
}

impl khora_core::lane::Lane for DefinitionSerializationLane {
    fn strategy_name(&self) -> &'static str {
        "KH_DEFINITION_RON_V1"
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

impl SerializationStrategy for DefinitionSerializationLane {
    /// Returns a unique identifier for this serialization strategy.
    fn get_strategy_id(&self) -> &'static str {
        self.strategy_name()
    }

    /// Serializes the given world into a byte vector using the stable intermediate representation.
    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        let mut entity_defs = Vec::new();

        // Iterate over all entities that have at least one component.
        for entity_id in world.iter_entities() {
            let mut component_defs = Vec::new();

            // Translate `Transform` component if it exists.
            if let Some(transform) = world.get::<Transform>(entity_id) {
                let serializable_transform = SerializableTransform {
                    translation: transform.translation,
                    rotation: transform.rotation,
                    scale: transform.scale,
                };
                component_defs.push(ComponentDefinition::Transform(serializable_transform));
            }

            // Translate `Parent` component if it exists.
            if let Some(parent) = world.get::<Parent>(entity_id) {
                component_defs.push(ComponentDefinition::Parent(SerializableParent(parent.0)));
            }

            // Only include entities that have components we care about.
            if !component_defs.is_empty() {
                entity_defs.push(EntityDefinition {
                    id: entity_id,
                    components: component_defs,
                });
            }
        }

        let scene_definition = SceneDefinition {
            entities: entity_defs,
        };

        // Use RON for human-readable output.
        let pretty_config = ron::ser::PrettyConfig::default().indentor("  ".to_string());
        ron::ser::to_string_pretty(&scene_definition, pretty_config)
            .map(|s| s.into_bytes())
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    /// Deserializes the given byte slice into the provided world using the stable intermediate representation.
    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        let scene_def: SceneDefinition = ron::de::from_bytes(data)
            .map_err(|e| DeserializationError::InvalidFormat(e.to_string()))?;

        let mut id_map = HashMap::<EntityId, EntityId>::new();

        // First Pass: Spawn all entities and create an ID map.
        // This ensures all entities exist before we try to establish parent-child relationships.
        for entity_def in &scene_def.entities {
            // We use `spawn(())` to create an entity with no components initially.
            let new_id = world.spawn(());
            id_map.insert(entity_def.id, new_id);
        }

        // Second Pass: Add components to the newly created entities.
        for entity_def in &scene_def.entities {
            let new_id = id_map[&entity_def.id]; // We can unwrap, we know the ID exists.

            for component_def in &entity_def.components {
                match component_def {
                    ComponentDefinition::Transform(st) => {
                        let transform = Transform {
                            translation: st.translation,
                            rotation: st.rotation,
                            scale: st.scale,
                        };
                        world.add_component(new_id, transform).ok();
                        world
                            .add_component(new_id, GlobalTransform::identity())
                            .ok();
                    }
                    ComponentDefinition::Parent(sp) => {
                        // Use the map to find the *new* ID of the parent entity.
                        if let Some(new_parent_id) = id_map.get(&sp.0) {
                            world.add_component(new_id, Parent(*new_parent_id)).ok();
                        } else {
                            // This would be an error: the file references a parent that wasn't defined.
                            // We'll ignore it for now.
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
