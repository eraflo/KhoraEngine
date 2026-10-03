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

//! Handles to bodies and colliders owned by a physics backend.

use serde::{Deserialize, Serialize};

/// Opaque handle to a rigid body in the physics engine.
///
/// Carries the backend's slot **and its generation**. A backend that recycles
/// slots — Rapier does — would otherwise resolve a handle to whatever now
/// occupies the slot the caller meant, silently: a stale handle would move
/// somebody else's body rather than fail. See [`SlotId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RigidBodyHandle(pub u64);

/// Opaque handle to a collider in the physics engine.
///
/// Carries the slot and its generation, for the reason [`RigidBodyHandle`]
/// gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColliderHandle(pub u64);

/// A backend slot and the generation that says which occupant is meant.
///
/// Both handle types are one of these packed into a `u64`. Packed rather than
/// two fields because a handle is stored on components and crosses the
/// serialization boundary, and one number stays one number there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotId {
    /// Which slot in the backend's arena.
    pub index: u32,
    /// How many times that slot has been reused before this occupant.
    pub generation: u32,
}

impl SlotId {
    /// Packs into the `u64` a handle carries.
    pub const fn pack(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }

    /// Unpacks what [`pack`](Self::pack) wrote.
    pub const fn unpack(packed: u64) -> Self {
        Self {
            index: packed as u32,
            generation: (packed >> 32) as u32,
        }
    }
}

impl RigidBodyHandle {
    /// The slot and generation this addresses.
    pub const fn slot(self) -> SlotId {
        SlotId::unpack(self.0)
    }
}

impl ColliderHandle {
    /// The slot and generation this addresses.
    pub const fn slot(self) -> SlotId {
        SlotId::unpack(self.0)
    }
}
