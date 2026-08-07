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

//! What a body is doing, as opposed to what it was told to start doing.
//!
//! `RigidBody::initial_velocity` used to be documented *"Current linear
//! velocity"* while being `Authored` and serialized into scene files. Both
//! could not be true, and the sync could not tell which it held: it wrote the
//! authored value into the solver every frame, so gravity accumulated for one
//! step and was then erased. A body fell a fraction of a millimetre per frame,
//! forever, instead of falling.
//!
//! The split is the same one [`SimulatedTransform`](crate::ecs::SimulatedTransform)
//! makes for the pose, and for the same reason: an author declares a starting
//! condition, a solver produces a live value, and one field cannot be both.

use khora_core::math::Vec3;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The velocity a body currently has.
///
/// Written by the physics writeback from what the provider reports, so reading
/// it tells you what the body is doing rather than what its author typed.
/// Present only while something simulates the entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Component, Serialize, Deserialize)]
#[component(domain = Physics, provenance = Runtime)]
pub struct BodyMotion {
    /// Metres per second.
    pub linear: Vec3,
    /// Radians per second, about each axis.
    pub angular: Vec3,
}
