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

//! Implements the "Archetype" serialization strategy for scenes.

use crate::scene_lane::strategies::{
    DeserializationError, SerializationError, SerializationStrategy,
};
use khora_core::lane::Lane;
use khora_data::ecs::World;

// --- The Lane ---

/// A serialization lane that uses the "Archetype" strategy.
#[derive(Default)]
pub struct ArchetypeSerializationLane;

impl ArchetypeSerializationLane {
    /// Creates a new `ArchetypeSerializationLane`.
    pub fn new() -> Self {
        Self
    }
}

impl khora_core::lane::Lane for ArchetypeSerializationLane {
    fn strategy_name(&self) -> &'static str {
        "KH_ARCHETYPE_V1"
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

impl SerializationStrategy for ArchetypeSerializationLane {
    fn get_strategy_id(&self) -> &'static str {
        self.strategy_name()
    }

    /// Serializes the world by calling its internal, unsafe `serialize_archetype` method.
    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        world
            .serialize_archetype()
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    /// Deserializes the world by calling its internal, unsafe `deserialize_archetype` method.
    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        world
            .deserialize_archetype(data)
            .map_err(|e| DeserializationError::WorldPopulationFailed(e.to_string()))
    }
}
