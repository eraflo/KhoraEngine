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

//! The contact points found between two colliders.

use khora_core::math::Vec3;

/// Detailed information about a contact between two colliders.
///
/// The currency between [`collision`](super::collision) and [`solver`](super::solver) — the narrow phase
/// produces one per contact, the solver consumes it. It is not part of the
/// [`PhysicsProvider`](khora_core::physics::PhysicsProvider) contract, so it
/// belongs to this backend rather than to the engine's vocabulary.
///
/// Not serializable: it describes one contact during one step, and the derives
/// it used to carry in `khora-core` had no callers.
#[derive(Debug, Clone, Copy)]
pub struct ContactManifold {
    /// Normal vector pointing from entity A to entity B.
    pub normal: Vec3,
    /// Intersection depth.
    pub depth: f32,
    /// Contact point in world space.
    pub point: Vec3,
}

impl ContactManifold {
    /// Returns the inverted manifold (flipped normal).
    pub fn inverted(&self) -> Self {
        Self {
            normal: -self.normal,
            depth: self.depth,
            point: self.point,
        }
    }
}
