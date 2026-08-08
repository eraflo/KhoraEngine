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

//! Khora's own physics backend — the pieces of it that exist.
//!
//! # Status
//!
//! **This does not implement [`PhysicsProvider`] yet.** Three stages of a
//! simulation step are written and tested — a broad phase
//! ([`DynamicTree`](dynamic_tree::DynamicTree), an incremental AABB BVH), a
//! narrow phase ([`NarrowPhase`](collision::NarrowPhase)) and a contact solver
//! ([`ImpulseSolver`](solver::ImpulseSolver)) — but nothing stitches them into
//! a world that the engine can step. [`rapier`](super::rapier) is the backend
//! the engine actually runs.
//!
//! Saying so here matters: these modules have no callers, and code with no
//! callers reads as abandoned unless something says otherwise. This is the
//! start of a second backend, kept because Rapier is a general-purpose engine
//! and the intent is to eventually do things it does not do.
//!
//! # Why it lives here and not in `khora-core`
//!
//! It used to sit in `khora-core::physics`, beside the [`PhysicsProvider`]
//! trait it will one day implement. That put concrete simulation code — a
//! restitution coefficient of `0.2`, a Baumgarte factor, a BVH free-list — in
//! the crate whose whole job is to depend on nothing and declare contracts, and
//! `khora-sdk` re-exports `khora_core`, so every game shipped with a solver it
//! never called in its public API.
//!
//! Here it sits beside `rapier`, which is where a backend belongs and where the
//! two must be for a choice between them to mean anything. The contract stays
//! in `khora-core`: this backend will import [`PhysicsProvider`],
//! [`ColliderShape`] and [`BodyType`] exactly as the Rapier one does.
//!
//! [`PhysicsProvider`]: khora_core::physics::PhysicsProvider
//! [`ColliderShape`]: khora_core::physics::ColliderShape
//! [`BodyType`]: khora_core::physics::BodyType

pub mod collision;
pub mod dynamic_tree;
pub mod solver;

use khora_core::math::Vec3;

/// Detailed information about a contact between two colliders.
///
/// The currency between [`collision`] and [`solver`] — the narrow phase
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
