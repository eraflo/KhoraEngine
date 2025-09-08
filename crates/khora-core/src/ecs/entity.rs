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

//! Defines core types related to entities in the ECS architecture.

use serde::{Deserialize, Serialize};

/// A unique identifier for an entity in the world.
///
/// It combines an index with a generation count to solve the "ABA problem".
/// When an entity is despawned, its index can be recycled for a new entity,
/// but the generation is incremented. This ensures that old `EntityId` handles
/// pointing to a recycled index become invalid and cannot accidentally affect
/// the new entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId {
    /// The index of the entity's metadata in the central `Vec<EntityMetadata>`.
    pub index: u32,
    /// A generation counter that is incremented each time the index is recycled.
    pub generation: u32,
}
