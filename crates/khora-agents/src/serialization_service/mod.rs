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

//! Scene serialization service — on-demand, not an Agent.
//!
//! This service replaces the former `SerializationAgent`. It provides
//! `save_world()` and `load_world()` APIs backed by a strategy registry.
//! No GORNA negotiation — the serialization strategy is chosen by the caller.

use khora_core::scene::{SceneFile, SceneHeader, SerializationGoal};
use khora_data::ecs::World;
use khora_lanes::scene_lane::{
    ArchetypeSerializationLane, DefinitionSerializationLane, RecipeSerializationLane,
    SerializationStrategy,
};
use std::collections::HashMap;

/// An error that can occur within the `SerializationService`.
#[derive(Debug)]
pub enum SerializationError {
    /// No suitable serialization strategy was found.
    StrategyNotFound,
    /// The scene file header is invalid or corrupted.
    InvalidHeader,
    /// A general processing error occurred.
    ProcessingError(String),
}

/// The serialization service.
///
/// Provides on-demand scene save/load through a strategy pattern.
/// Registered in `ServiceRegistry` and accessed by game code via `AppContext`.
pub struct SerializationService {
    strategies: HashMap<String, Box<dyn SerializationStrategy>>,
}

impl SerializationService {
    /// Creates a new service and registers all built-in strategies.
    pub fn new() -> Self {
        let mut strategies: HashMap<String, Box<dyn SerializationStrategy>> = HashMap::new();

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
    ) -> Result<SceneFile, SerializationError> {
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
            .ok_or(SerializationError::StrategyNotFound)?;

        let payload = strategy
            .serialize(world)
            .map_err(|e| SerializationError::ProcessingError(e.to_string()))?;

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
    pub fn load_world(
        &self,
        file: &SceneFile,
        world: &mut World,
    ) -> Result<(), SerializationError> {
        let strategy_id = str::from_utf8(&file.header.strategy_id)
            .map_err(|_| SerializationError::InvalidHeader)?
            .trim_end_matches('\0');

        let strategy = self
            .strategies
            .get(strategy_id)
            .ok_or(SerializationError::StrategyNotFound)?;

        strategy
            .deserialize(&file.payload, world)
            .map_err(|e| SerializationError::ProcessingError(e.to_string()))
    }
}

impl Default for SerializationService {
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

        transform_propagation_system(&mut source_world);
        let expected_child_global = source_world.get::<GlobalTransform>(child_id).unwrap().0;

        let service = SerializationService::new();
        let scene_file = service
            .save_world(&source_world, SerializationGoal::LongTermStability)
            .unwrap();

        let mut dest_world = World::new();
        service.load_world(&scene_file, &mut dest_world).unwrap();

        transform_propagation_system(&mut dest_world);

        let mut root_query = dest_world.query::<(&Transform, Without<Parent>)>();
        let (new_root_transform, _) = root_query.next().expect("Should be one root entity");
        assert_eq!(*new_root_transform, root_transform);

        let mut child_query = dest_world.query::<(&Transform, &Parent, &GlobalTransform)>();
        let (new_child_transform, _new_parent, new_child_global) =
            child_query.next().expect("Should be one child entity");

        assert_eq!(*new_child_transform, child_transform);
        assert_eq!(new_child_global.0, expected_child_global);
    }

    #[test]
    fn test_recipe_serialization_round_trip() {
        let mut source_world = World::new();

        let root_transform = Transform {
            translation: Vec3::new(25.0, 0.0, 0.0),
            ..Default::default()
        };
        source_world.spawn((root_transform, GlobalTransform::identity()));

        let service = SerializationService::new();
        let scene_file = service
            .save_world(&source_world, SerializationGoal::EditorInterchange)
            .unwrap();

        let mut dest_world = World::new();
        service.load_world(&scene_file, &mut dest_world).unwrap();

        assert_eq!(
            str::from_utf8(&scene_file.header.strategy_id)
                .unwrap()
                .trim_end_matches('\0'),
            "KH_RECIPE_V1"
        );

        let mut root_query = dest_world.query::<(&Transform, Without<Parent>)>();
        let (new_root_transform, _) = root_query.next().expect("Should be one root entity");
        assert_eq!(*new_root_transform, root_transform);
    }

    #[test]
    fn test_archetype_serialization_round_trip() {
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

        transform_propagation_system(&mut source_world);
        let expected_child_global = source_world.get::<GlobalTransform>(child_id).unwrap().0;

        let service = SerializationService::new();
        let scene_file = service
            .save_world(&source_world, SerializationGoal::FastestLoad)
            .unwrap();

        let mut dest_world = World::new();
        service.load_world(&scene_file, &mut dest_world).unwrap();

        assert_eq!(
            str::from_utf8(&scene_file.header.strategy_id)
                .unwrap()
                .trim_end_matches('\0'),
            "KH_ARCHETYPE_V1"
        );

        let new_child_global = dest_world
            .get::<GlobalTransform>(child_id)
            .expect("Child entity should exist with the same ID")
            .0;

        assert_eq!(new_child_global, expected_child_global);
        assert!(
            dest_world.get::<Transform>(root_id).is_some(),
            "Root entity should exist with the same ID"
        );
    }
}
