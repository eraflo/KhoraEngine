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

use super::{ColliderShape, ContactManifold};
use crate::math::{AffineTransform, Vec3, Vec4};

/// Narrow-phase collision detection system.
pub struct NarrowPhase;

impl NarrowPhase {
    /// Creates a new `NarrowPhase` instance.
    pub fn new() -> Self {
        Self
    }

    /// Detects collision between two shapes.
    ///
    /// Returns a `ContactManifold` if a collision is detected.
    pub fn detect(
        &self,
        shape_a: &ColliderShape,
        trans_a: &AffineTransform,
        shape_b: &ColliderShape,
        trans_b: &AffineTransform,
    ) -> Option<ContactManifold> {
        match (shape_a, shape_b) {
            (ColliderShape::Sphere(ra), ColliderShape::Sphere(rb)) => {
                let pa = trans_a.translation();
                let pb = trans_b.translation();
                let delta = pb - pa;
                let dist_sq = delta.length_squared();
                let total_r = ra + rb;
                if dist_sq < total_r * total_r {
                    let dist = dist_sq.sqrt();
                    let normal = if dist > 0.0001 {
                        delta / dist
                    } else {
                        Vec3::new(0.0, 1.0, 0.0)
                    };
                    Some(ContactManifold {
                        normal,
                        depth: total_r - dist,
                        point: pa + normal * (*ra),
                    })
                } else {
                    None
                }
            }
            (ColliderShape::Sphere(ra), ColliderShape::Box(half_b)) => {
                let pa = trans_a.translation();
                // Transform sphere center to box local space
                let inv_b = trans_b.inverse()?;
                let local_pa = (inv_b.0 * Vec4::from_vec3(pa, 1.0)).truncate();

                // Closest point on box
                let closest = Vec3::new(
                    local_pa.x.clamp(-half_b.x, half_b.x),
                    local_pa.y.clamp(-half_b.y, half_b.y),
                    local_pa.z.clamp(-half_b.z, half_b.z),
                );

                let delta = local_pa - closest;
                let dist_sq = delta.length_squared();
                if dist_sq < ra * ra {
                    let dist = dist_sq.sqrt();
                    let local_normal = if dist > 0.0001 {
                        delta / dist
                    } else {
                        Vec3::new(0.0, 1.0, 0.0)
                    };
                    // Normal back to world space
                    let normal = trans_b.rotation().rotate_vec3(local_normal);
                    Some(ContactManifold {
                        normal,
                        depth: ra - dist,
                        point: (trans_b.0 * Vec4::from_vec3(closest, 1.0)).truncate(),
                    })
                } else {
                    None
                }
            }
            // Mirror Sphere-Box for Box-Sphere
            (ColliderShape::Box(_half_a), ColliderShape::Sphere(_rb)) => self
                .detect(shape_b, trans_b, shape_a, trans_a)
                .map(|m| m.inverted()),
            _ => None,
        }
    }
}

impl Default for NarrowPhase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    #[test]
    fn test_sphere_sphere_collision() {
        let narrow = NarrowPhase::new();
        let sphere_a = ColliderShape::Sphere(1.0);
        let sphere_b = ColliderShape::Sphere(1.0);
        let trans_a = AffineTransform::from_translation(Vec3::new(0.0, 0.0, 0.0));
        let trans_b = AffineTransform::from_translation(Vec3::new(1.5, 0.0, 0.0));

        let manifold = narrow
            .detect(&sphere_a, &trans_a, &sphere_b, &trans_b)
            .unwrap();
        assert!(manifold.depth > 0.0);
        assert!((manifold.normal.x - 1.0).abs() < 0.001);
        assert!((manifold.depth - 0.5).abs() < 0.001);
    }
}
