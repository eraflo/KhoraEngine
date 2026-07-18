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

//! Broadphase collision-pair scratch.
//!
//! Plain data (never an ECS component, to keep it out of the editor scene
//! tree and off the structural-mutation path) meant to live as an
//! `Arc<Mutex<CollisionPairs>>` [`Resource`](khora_core::Resources). It has
//! no consumer today — the experimental native broadphase/solver lanes that
//! used it were removed — but is kept as a reusable serializable type.

use bincode::{Decode, Encode};
use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};

/// A pair of entities that are potentially colliding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct CollisionPair {
    /// The first entity in the pair.
    pub entity_a: EntityId,
    /// The second entity in the pair.
    pub entity_b: EntityId,
}

/// Sink of potential collision pairs identified during broadphase.
///
/// Intended to live in [`khora_core::Resources`] as
/// `Arc<Mutex<CollisionPairs>>`: a broadphase pass overwrites the inner
/// `pairs` vector each frame and a resolution pass reads it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollisionPairs {
    /// List of potential collision pairs detected this frame.
    pub pairs: Vec<CollisionPair>,
}
