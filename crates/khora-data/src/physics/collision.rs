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

//! Broadphase collision pairs: plain data shared through
//! [`Resources`](khora_core::Resources), never an ECS component, so it stays
//! out of the scene tree and off the structural-mutation path.

use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

/// A pair of entities that are potentially colliding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionPair {
    /// The first entity in the pair.
    pub entity_a: EntityId,
    /// The second entity in the pair.
    pub entity_b: EntityId,
}

/// The potential collision pairs a broadphase pass found this frame.
///
/// Lives in [`khora_core::Resources`] as `Arc<Mutex<CollisionPairs>>`; the
/// broadphase overwrites `pairs` each frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollisionPairs {
    /// List of potential collision pairs detected this frame.
    pub pairs: Vec<CollisionPair>,
}
