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

//! Defines the data format for the "Recipe" serialization strategy.
//!
//! This format represents a scene as an executable sequence of commands, which provides
//! great flexibility for tools, streaming, and scene patching.

use bincode::{Decode, Encode};
use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

/// The root container for a scene recipe. It's simply a list of commands.
#[derive(Debug, Serialize, Deserialize, Encode, Decode)]
pub struct SceneRecipe {
    /// The ordered list of commands to execute to reconstruct the scene.
    pub commands: Vec<SceneCommand>,
}

/// A single, atomic operation required to construct a scene.
#[derive(Debug, Serialize, Deserialize, Encode, Decode)]
pub enum SceneCommand {
    /// Spawns a new, empty entity with a specific ID from the original scene.
    Spawn {
        /// The ID to assign to the newly spawned entity.
        id: EntityId,
    },
    /// Adds a component to a specified entity.
    AddComponent {
        /// The ID of the entity to which the component should be added.
        entity_id: EntityId,
        /// The full type name of the component (e.g., "khora_data::ecs::components::Transform").
        /// This will be used for reflection during deserialization.
        component_type: String,
        /// The component data, serialized into a compact binary format using bincode.
        component_data: Vec<u8>,
    },
    /// Establishes a parent-child relationship between two entities.
    SetParent {
        /// The ID of the child entity.
        child_id: EntityId,
        /// The ID of the parent entity.
        parent_id: EntityId,
    },
}
