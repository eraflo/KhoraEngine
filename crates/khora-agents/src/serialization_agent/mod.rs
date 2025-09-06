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

use khora_core::scene::{
    format::{SceneFile, SceneHeader},
    serialization::SerializationGoal,
};
use khora_data::ecs::world::World;
use khora_lanes::scene_lane::strategies::{DeserializationError, SerializationError, SerializationStrategy};
use std::collections::HashMap;

/// An error that can occur within the `SerializationAgent`.
#[derive(Debug)]
pub enum AgentError {
    /// The requested operation is not yet implemented.
    NotImplemented,
    /// No strategy could be found for the given goal or file format.
    StrategyNotFound,
}

/// The ISA responsible for the entire scene serialization process.
#[derive(Default)]
pub struct SerializationAgent {
    /// A registry of all available serialization strategies, keyed by their unique ID.
    strategies: HashMap<String, Box<dyn SerializationStrategy>>,
}

impl SerializationAgent {
    /// Creates a new, empty `SerializationAgent`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Saves the current state of the `World` based on a high-level goal.
    ///
    /// This is a placeholder implementation. The actual logic will be built in Phase 4.
    pub fn save_world(
        &self,
        _world: &World,
        _goal: SerializationGoal,
    ) -> Result<SceneFile, AgentError> {
        // In the future, this will:
        // 1. Select a strategy based on the goal.
        // 2. Call `strategy.serialize(world)`.
        // 3. Construct and return a `SceneFile`.
        Err(AgentError::NotImplemented)
    }

    /// Populates a `World` from a `SceneFile`.
    ///
    /// This is a placeholder implementation.
    pub fn load_world(&self, _file: &SceneFile, _world: &mut World) -> Result<(), AgentError> {
        // In the future, this will:
        // 1. Read the strategy_id from the file header.
        // 2. Look up the strategy in the registry.
        // 3. Call `strategy.deserialize(file.payload, world)`.
        Err(AgentError::NotImplemented)
    }
}