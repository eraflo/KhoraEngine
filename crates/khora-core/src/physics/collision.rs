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

//! Collisions: the events a backend reports and the channel they travel on.

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::handle::ColliderHandle;

/// Two entities beginning or ending contact.
///
/// The engine's form of a collision, as distinct from [`CollisionEvent`] which
/// is the backend's: that one names two colliders, this one names two entities,
/// and the translation happens where the provider is in hand rather than being
/// left to whoever consumes it.
///
/// **A transition, not a state.** `Started` and `Stopped` say what changed;
/// "who is touching whom right now" is a different question that this does not
/// answer, and that a relation between entities would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collision {
    /// Whether contact began or ended.
    pub kind: CollisionKind,
    /// One of the two. Which is which carries no meaning — a contact is
    /// symmetric, and a consumer that cares about one entity checks both.
    pub a: crate::ecs::entity::EntityId,
    /// The other.
    pub b: crate::ecs::entity::EntityId,
}

/// Whether a [`Collision`] began or ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionKind {
    /// The two started touching.
    Started,
    /// They stopped.
    Stopped,
}

impl crate::event::Supersedes for Collision {
    // Nothing coalesces. Two entities that touch, separate and touch again
    // within one frame did that twice, and a consumer counting hits is
    // counting hits.
}

/// How many collisions are kept for readers that have not caught up.
///
/// A heavy frame is hundreds of contacts; this is several frames of one. The
/// case that reaches it is a consumer that stopped reading, where the oldest
/// contact is also the least worth delivering.
pub const COLLISION_BACKLOG: usize = 4096;

/// A channel for the contacts the physics lane reports.
pub fn collision_channel() -> crate::event::Channel<Collision> {
    crate::event::Channel::bounded(COLLISION_BACKLOG, crate::event::WhenFull::DropOldest)
}

/// What one physics step reported, on its way from the lane to the channel.
///
/// A deck slot, because that is the sanctioned road out of a lane: a lane that
/// wrote the shared channel directly would be a lane reaching engine state, and
/// the scheduler would have no way to know it had.
#[derive(Debug, Clone, Default)]
pub struct ContactBatch {
    /// The contacts, in the order the backend reported them.
    pub contacts: Vec<Collision>,
}

/// Events representing collision start/end.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode)]
pub enum CollisionEvent {
    /// Collision between two colliders started.
    Started(ColliderHandle, ColliderHandle),
    /// Collision between two colliders stopped.
    Stopped(ColliderHandle, ColliderHandle),
}
