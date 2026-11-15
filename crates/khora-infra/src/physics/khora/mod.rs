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
//! the engine actually runs. The contract stays in `khora-core`: this backend
//! will implement [`PhysicsProvider`] with [`ColliderShape`] and [`BodyType`],
//! as the Rapier one does.
//!
//! See the book: concepts/physics.md, "The in-house one — started, not finished".
//!
//! [`PhysicsProvider`]: khora_core::physics::PhysicsProvider
//! [`ColliderShape`]: khora_core::physics::ColliderShape
//! [`BodyType`]: khora_core::physics::BodyType

pub mod collision;
pub mod dynamic_tree;
pub mod solver;

mod contact;

pub use contact::ContactManifold;
