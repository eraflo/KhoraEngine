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

//! [`BodyMotion`]: what a body is doing, as opposed to what it was told to start doing.
//!
//! See the book: concepts/physics.md, "Who owns a pose".

use khora_core::math::Vec3;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The velocity a body currently has, as the physics provider reports it.
///
/// Runtime state, never saved; present only while something simulates the
/// entity. The velocity an author gives is `RigidBody::initial_velocity`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Component, Serialize, Deserialize)]
#[component(domain = Physics, provenance = Runtime)]
pub struct BodyMotion {
    /// The linear velocity, in metres per second.
    pub linear: Vec3,
    /// The angular velocity, in radians per second about each axis.
    pub angular: Vec3,
}
