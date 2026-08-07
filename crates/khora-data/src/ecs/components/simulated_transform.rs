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

//! Where the simulation put an entity, as opposed to where its author did.
//!
//! # Why this is not `Transform`
//!
//! `ComponentProvenance` says who has the right to **write** a component, and
//! `Transform` is `Authored`: a human, a tool, or game code. The physics
//! writeback used to overwrite it every frame anyway, so one field held two
//! different facts — what the designer declared, and what gravity made of it —
//! and nothing could tell them apart. Everything downstream inherited the
//! ambiguity: saving a scene mid-play recorded the simulated pose over the
//! authored one, the inspector showed a number the designer never typed, and a
//! gizmo drag anchored on wherever the body had fallen to.
//!
//! # Why `Runtime` and not `Derived`
//!
//! `Derived` means *recomputed by the engine from `Authored` state* — that is
//! why a duplicate must not carry a copy, and why `GlobalTransform` is one. A
//! pose after five seconds of falling is **not** recomputable from the authored
//! data: it depends on the path taken, on every contact along the way, on the
//! integrator. That is `Runtime` exactly as the axis defines it — *per-run
//! transient state that no one authors and nothing recomputes from authored
//! data* — and it is what keeps this out of scene files without a single line
//! of filtering, because the serializer already honours the axis.
//!
//! # It is a world pose
//!
//! The provider reports where a body is in the world, and this holds that
//! verbatim. It is deliberately **not** composed with a parent: a body parented
//! to a moving platform is simulated in world space, and the old code's habit of
//! writing the world pose into the *local* `Transform` and letting propagation
//! multiply it by the parent again is what made a parented body drift by its
//! parent's transform every single frame.

use khora_core::math::{AffineTransform, Mat4, Quat, Vec3};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The world pose the simulation currently gives an entity.
///
/// Present only while something simulates it. [`transform_propagation`] prefers
/// it over the authored `Transform` when it is there.
///
/// [`transform_propagation`]: crate::ecs::systems::transform_propagation
#[derive(Debug, Clone, Copy, PartialEq, Component, Serialize, Deserialize)]
#[component(domain = Spatial, provenance = Runtime)]
pub struct SimulatedTransform(pub AffineTransform);

impl SimulatedTransform {
    /// The pose a provider just reported.
    ///
    /// No scale: a solver moves and turns a body, it does not resize one, and
    /// inventing a scale here would overwrite the author's on the way to
    /// `GlobalTransform`.
    pub fn from_parts(translation: Vec3, rotation: Quat) -> Self {
        Self(AffineTransform(
            Mat4::from_translation(translation) * Mat4::from_quat(rotation),
        ))
    }

    /// Where it is.
    pub fn translation(&self) -> Vec3 {
        self.0.translation()
    }

    /// How it is oriented.
    pub fn rotation(&self) -> Quat {
        self.0.rotation()
    }
}

impl Default for SimulatedTransform {
    fn default() -> Self {
        Self(AffineTransform::IDENTITY)
    }
}
