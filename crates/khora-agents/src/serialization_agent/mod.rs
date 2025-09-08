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

//! The Intelligent Subsystem Agent responsible for managing scene serialization.
//!
//! This agent acts as the primary entry point for all serialization tasks. It holds a
//! registry of available [`SerializationStrategy`] `Lanes` and contains the SAA logic
//! to select the appropriate strategy based on a given [`SerializationGoal`].

use khora_core::scene::{SceneFile, SceneHeader, SerializationGoal};
use khora_data::ecs::World;
use khora_lanes::scene_lane::{
    ArchetypeSerializationLane, DefinitionSerializationLane, RecipeSerializationLane,
    SerializationStrategy,
};
use std::collections::HashMap;

/// An error that can occur within the `SerializationAgent`.
#[derive(Debug)]
pub enum AgentError {
    /// No suitable serialization strategy was found for the requested operation.
    StrategyNotFound,
    /// The scene file header is invalid or corrupted.
    InvalidHeader,
    /// A general processing error occurred during serialization or deserialization.
    ProcessingError(String),
}

/// The ISA responsible for the entire scene serialization process.
pub struct SerializationAgent {
    /// A registry of all available serialization strategies, keyed by their unique ID.
    strategies: HashMap<String, Box<dyn SerializationStrategy>>,
}

impl SerializationAgent {
    /// Creates a new `SerializationAgent` and registers all built-in strategies.
    pub fn new() -> Self {
        let mut strategies: HashMap<String, Box<dyn SerializationStrategy>> = HashMap::new();

        // Register the built-in strategies.
        let definition_lane = DefinitionSerializationLane::new();
        strategies.insert(
            definition_lane.get_strategy_id().to_string(),
            Box::new(definition_lane),
        );

        let recipe_lane = RecipeSerializationLane::new();
        strategies.insert(
            recipe_lane.get_strategy_id().to_string(),
            Box::new(recipe_lane),
        );

        let archetype_lane = ArchetypeSerializationLane::new();
        strategies.insert(
            archetype_lane.get_strategy_id().to_string(),
            Box::new(archetype_lane),
        );

        Self { strategies }
    }

    /// Saves the current state of the `World` based on a high-level goal.
    pub fn save_world(
        &self,
        world: &World,
        goal: SerializationGoal,
    ) -> Result<SceneFile, AgentError> {
        let strategy_id = match goal {
            SerializationGoal::HumanReadableDebug | SerializationGoal::LongTermStability => {
                "KH_DEFINITION_RON_V1"
            }
            SerializationGoal::SmallestFileSize | SerializationGoal::EditorInterchange => {
                "KH_RECIPE_V1"
            }
            SerializationGoal::FastestLoad => "KH_ARCHETYPE_V1",
        };

        let strategy = self
            .strategies
            .get(strategy_id)
            .ok_or(AgentError::StrategyNotFound)?;

        let payload = strategy
            .serialize(world)
            .map_err(|e| AgentError::ProcessingError(e.to_string()))?;

        let strategy_id_str = strategy.get_strategy_id();
        let mut strategy_id_bytes = [0u8; 32];
        strategy_id_bytes[..strategy_id_str.len()].copy_from_slice(strategy_id_str.as_bytes());

        let header = SceneHeader {
            magic_bytes: khora_core::scene::HEADER_MAGIC_BYTES,
            format_version: 1,
            strategy_id: strategy_id_bytes,
            payload_length: payload.len() as u64,
        };

        Ok(SceneFile { header, payload })
    }

    /// Populates a `World` from a `SceneFile`.
    pub fn load_world(&self, file: &SceneFile, world: &mut World) -> Result<(), AgentError> {
        // Convert the strategy_id bytes back to a string slice.
        let strategy_id = str::from_utf8(&file.header.strategy_id)
            .map_err(|_| AgentError::InvalidHeader)? // Nouvelle erreur
            .trim_end_matches('\0');

        let strategy = self
            .strategies
            .get(strategy_id)
            .ok_or(AgentError::StrategyNotFound)?;

        strategy
            .deserialize(&file.payload, world)
            .map_err(|e| AgentError::ProcessingError(e.to_string()))
    }
}

impl Default for SerializationAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::Vec3;
    use khora_data::ecs::{GlobalTransform, Parent, Transform, Without, World};
    use khora_lanes::scene_lane::transform_propagation_system;

    #[test]
    fn test_serialization_round_trip() {
        // --- 1. ARRANGE ---
        // Create the "source" world and populate it.
        let mut source_world = World::new();

        let root_transform = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            ..Default::default()
        };
        let root_id = source_world.spawn((root_transform, GlobalTransform::identity()));

        let child_transform = Transform {
            translation: Vec3::new(0.0, 5.0, 0.0),
            ..Default::default()
        };
        let child_id = source_world.spawn((
            child_transform,
            GlobalTransform::identity(),
            Parent(root_id),
        ));

        // Run transform propagation to have a complete, valid state.
        transform_propagation_system(&mut source_world);
        let expected_child_global = source_world.get::<GlobalTransform>(child_id).unwrap().0;

        // --- 2. ACT ---
        // Create an agent and perform the save/load cycle.
        let agent = SerializationAgent::new();
        let scene_file = agent
            .save_world(&source_world, SerializationGoal::LongTermStability)
            .unwrap();

        // Create the "destination" world.
        let mut dest_world = World::new();
        agent.load_world(&scene_file, &mut dest_world).unwrap();

        // Run propagation on the new world.
        transform_propagation_system(&mut dest_world);

        // --- 3. ASSERT ---
        // We can't rely on EntityIds, so we verify the structure and data.

        // Find the new root (entity with Transform but no Parent).
        let mut root_query = dest_world.query::<(&Transform, Without<Parent>)>();
        let (new_root_transform, _) = root_query.next().expect("Should be one root entity");
        assert_eq!(*new_root_transform, root_transform);

        // Find the new child entity and verify its data.
        let mut child_query = dest_world.query::<(&Transform, &Parent, &GlobalTransform)>();
        let (new_child_transform, _new_parent, new_child_global) =
            child_query.next().expect("Should be one child entity");

        // Verify the local transform is correct.
        assert_eq!(*new_child_transform, child_transform);
        // We can't check the parent ID directly, but the propagation result is the true test.
        assert_eq!(new_child_global.0, expected_child_global);
    }

    #[test]
    fn test_recipe_serialization_round_trip() {
        // --- 1. ARRANGE ---
        let mut source_world = World::new();

        let root_transform = Transform {
            translation: Vec3::new(25.0, 0.0, 0.0),
            ..Default::default()
        };
        source_world.spawn((root_transform, GlobalTransform::identity()));

        // --- 2. ACT ---
        let agent = SerializationAgent::new();
        // We ask for the recipe strategy explicitly via the goal.
        let scene_file = agent
            .save_world(&source_world, SerializationGoal::EditorInterchange)
            .unwrap();

        let mut dest_world = World::new();
        agent.load_world(&scene_file, &mut dest_world).unwrap();

        // --- 3. ASSERT ---
        // We verify that the header has the correct strategy ID.
        assert_eq!(
            str::from_utf8(&scene_file.header.strategy_id)
                .unwrap()
                .trim_end_matches('\0'),
            "KH_RECIPE_V1"
        );

        // We verify that the data is correct.
        let mut root_query = dest_world.query::<(&Transform, Without<Parent>)>();
        let (new_root_transform, _) = root_query.next().expect("Should be one root entity");
        assert_eq!(*new_root_transform, root_transform);
    }

    #[test]
    fn test_archetype_serialization_round_trip() {
        // --- 1. ARRANGE ---
        // Create the source world and populate it with a simple hierarchy.
        let mut source_world = World::new();

        let root_transform = Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            ..Default::default()
        };
        let root_id = source_world.spawn((root_transform, GlobalTransform::identity()));

        let child_transform = Transform {
            translation: Vec3::new(0.0, 5.0, 0.0),
            ..Default::default()
        };
        let child_id = source_world.spawn((
            child_transform,
            GlobalTransform::identity(),
            Parent(root_id),
        ));

        // Run the transform system to ensure the source world is in a valid, fully computed state.
        transform_propagation_system(&mut source_world);
        let expected_child_global = source_world.get::<GlobalTransform>(child_id).unwrap().0;

        // --- 2. ACT ---
        // Create an agent and serialize the world using the `FastestLoad` goal,
        // which should trigger the `ArchetypeSerializationLane`.
        let agent = SerializationAgent::new();
        let scene_file = agent
            .save_world(&source_world, SerializationGoal::FastestLoad)
            .unwrap();

        // Archetype deserialization completely replaces the world's state.
        // We create a new empty world to load the data into.
        let mut dest_world = World::new();
        agent.load_world(&scene_file, &mut dest_world).unwrap();

        // --- 3. ASSERT ---
        // Verify that the correct strategy was written to the file header.
        assert_eq!(
            str::from_utf8(&scene_file.header.strategy_id)
                .unwrap()
                .trim_end_matches('\0'),
            "KH_ARCHETYPE_V1"
        );

        // A key guarantee of the archetype strategy is that it preserves EntityIds,
        // as it's a direct memory snapshot. We can therefore use the original IDs to
        // query the destination world.
        let new_child_global = dest_world
            .get::<GlobalTransform>(child_id)
            .expect("Child entity should exist with the same ID")
            .0;

        // The simplest and most robust check is to see if the child's final GlobalTransform
        // is correct. This implicitly validates the entire hierarchy and data.
        // Note: No need to run transform_propagation_system on dest_world, as the
        // raw memory of the `GlobalTransform` components was also copied.
        assert_eq!(new_child_global, expected_child_global);

        // As a sanity check, also verify the root entity was restored correctly.
        assert!(
            dest_world.get::<Transform>(root_id).is_some(),
            "Root entity should exist with the same ID"
        );
    }
}
