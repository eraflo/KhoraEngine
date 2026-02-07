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

//! # Impulse Solver
//!
//! Pure mathematical implementation of constraint resolution using impulses.

use super::{BodyType, ContactManifold};
use crate::math::Vec3;

/// Represents the physical state of a body relevant to impulse resolution.
#[derive(Debug, Clone, Copy)]
pub struct VelocityState {
    /// Linear velocity vector.
    pub linear_velocity: Vec3,
    /// Angular velocity vector.
    pub angular_velocity: Vec3,
    /// Mass of the body.
    pub mass: f32,
    /// Type of the body (Dynamic, Static, Kinematic).
    pub body_type: BodyType,
}

/// A mathematical solver for impulse-based constraint resolution.
pub struct ImpulseSolver {
    /// Coefficient of restitution (bounciness).
    pub restitution: f32,
    /// Percentage of penetration to resolve per frame (Baumgarte stabilization).
    pub baumgarte_percent: f32,
    /// Penetration allowance to avoid jitter.
    pub slop: f32,
}

impl ImpulseSolver {
    /// Creates a new `ImpulseSolver` with default physical constants.
    pub fn new() -> Self {
        Self {
            restitution: 0.2,
            baumgarte_percent: 0.2,
            slop: 0.01,
        }
    }

    /// Resolves the collision between two bodies.
    ///
    /// Returns the updated velocity states for both bodies.
    pub fn resolve(
        &self,
        mut a: VelocityState,
        mut b: VelocityState,
        manifold: &ContactManifold,
    ) -> (VelocityState, VelocityState) {
        if a.body_type == BodyType::Static && b.body_type == BodyType::Static {
            return (a, b);
        }

        // 1. Calculate relative velocity in the direction of the collision normal.
        let rv = b.linear_velocity - a.linear_velocity;
        let vel_along_normal = rv.dot(manifold.normal);

        // 2. If already separating, no impulse is needed.
        if vel_along_normal > 0.0 {
            return (a, b);
        }

        // 3. Calculate impulse magnitude (j).
        let inv_mass_a = if a.body_type == BodyType::Dynamic {
            1.0 / a.mass
        } else {
            0.0
        };
        let inv_mass_b = if b.body_type == BodyType::Dynamic {
            1.0 / b.mass
        } else {
            0.0
        };
        let total_inv_mass = inv_mass_a + inv_mass_b;

        if total_inv_mass <= 0.0 {
            return (a, b);
        }

        let mut j = -(1.0 + self.restitution) * vel_along_normal;
        j /= total_inv_mass;

        // 4. Apply impulse as a change in linear velocity.
        let impulse = manifold.normal * j;
        if a.body_type == BodyType::Dynamic {
            a.linear_velocity = a.linear_velocity - (impulse * inv_mass_a);
        }
        if b.body_type == BodyType::Dynamic {
            b.linear_velocity = b.linear_velocity + (impulse * inv_mass_b);
        }

        // 5. Positional correction (Linear Projection) using velocity "slack".
        let correction_mag =
            (manifold.depth - self.slop).max(0.0) / total_inv_mass * self.baumgarte_percent;
        let correction = manifold.normal * correction_mag;

        if a.body_type == BodyType::Dynamic {
            a.linear_velocity = a.linear_velocity - (correction * inv_mass_a);
        }
        if b.body_type == BodyType::Dynamic {
            b.linear_velocity = b.linear_velocity + (correction * inv_mass_b);
        }

        (a, b)
    }
}

impl Default for ImpulseSolver {
    fn default() -> Self {
        Self::new()
    }
}
