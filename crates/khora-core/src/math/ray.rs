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

//! Rays and the intersection tests that go with them.
//!
//! One shared implementation for every caster in the engine: physics queries,
//! editor picking, and gizmo manipulation all measure against the same
//! primitives rather than each rolling their own. [`Ray`] lived in
//! [`crate::physics`] for a while — it is re-exported from there, since a ray is
//! geometry and only *some* of its users are physics.
//!
//! Every test returns a **signed distance along the ray** (its `t` parameter),
//! never a point, because the caller almost always wants to compare hits before
//! it wants a position. Recover the position with [`Ray::at`].

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::geometry::Aabb;
use super::Vec3;

/// Denominators below this mean the ray is parallel to what it is being tested
/// against, so the intersection is undefined rather than merely far away.
const PARALLEL_EPSILON: f32 = 1e-6;

/// A half-line: an origin and a direction.
///
/// `direction` is expected to be normalised. [`Ray::new`] guarantees it; the
/// struct literal — kept public because a ray is plain data — does not.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct Ray {
    /// Origin point.
    pub origin: Vec3,
    /// Direction vector (should be normalized).
    pub direction: Vec3,
}

impl Ray {
    /// Creates a ray, normalising `direction`.
    #[inline]
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize(),
        }
    }

    /// The point `distance` along the ray.
    #[inline]
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }

    /// The component-wise reciprocal of the direction, as the slab test wants
    /// it.
    ///
    /// A zero component yields an infinity, which [`Aabb::intersect_ray`]
    /// handles correctly: a ray parallel to a slab simply never leaves it.
    #[inline]
    pub fn inv_direction(&self) -> Vec3 {
        Vec3::new(
            1.0 / self.direction.x,
            1.0 / self.direction.y,
            1.0 / self.direction.z,
        )
    }

    /// Distance to the near face of `aabb`, or `None` if the ray misses it.
    ///
    /// Negative when the origin is already inside or past the box. Hoist
    /// [`Ray::inv_direction`] and call [`Aabb::intersect_ray`] directly when
    /// testing one ray against many boxes.
    #[inline]
    pub fn intersect_aabb(&self, aabb: &Aabb) -> Option<f32> {
        aabb.intersect_ray(self.origin, self.inv_direction())
    }

    /// Distance to the plane through `point` with normal `normal`, or `None`
    /// when the ray is parallel to it or the plane lies behind the origin.
    ///
    /// `normal` need not be normalised — it only appears in a ratio.
    #[inline]
    pub fn intersect_plane(&self, point: Vec3, normal: Vec3) -> Option<f32> {
        let denominator = self.direction.dot(normal);
        if denominator.abs() < PARALLEL_EPSILON {
            return None;
        }
        let distance = (point - self.origin).dot(normal) / denominator;
        (distance >= 0.0).then_some(distance)
    }

    /// Perpendicular distance from `point` to the ray.
    ///
    /// Measured from the *half*-line: a point behind the origin is measured
    /// from the origin itself, not from the line the ray extends backwards
    /// along.
    #[inline]
    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        let along = (point - self.origin).dot(self.direction).max(0.0);
        (point - self.at(along)).length()
    }

    /// How far along the line `origin + t * direction` its closest approach to
    /// this ray sits, or `None` if the two are parallel.
    ///
    /// The classic closest-approach of two skew lines. `direction` must be
    /// normalised. Note this parameterises the *other* line, not the ray —
    /// which is what a caller dragging along an axis wants to know.
    pub fn closest_param_on_line(&self, origin: Vec3, direction: Vec3) -> Option<f32> {
        let between = origin - self.origin;
        let alignment = direction.dot(self.direction);
        // Both directions are unit, so the general `a*c - b*b` reduces to this.
        let denominator = 1.0 - alignment * alignment;
        if denominator.abs() < PARALLEL_EPSILON {
            return None;
        }
        Some((alignment * self.direction.dot(between) - direction.dot(between)) / denominator)
    }

    /// The point on segment `a..b` closest to this ray.
    ///
    /// Falls back to the midpoint when the segment is parallel to the ray or
    /// degenerate — every point on it is then equally close, and a caller
    /// hit-testing a handle wants an answer rather than an absence.
    pub fn closest_point_on_segment(&self, a: Vec3, b: Vec3) -> Vec3 {
        let span = b - a;
        let length = span.length();
        if length < PARALLEL_EPSILON {
            return a;
        }
        let direction = span / length;
        match self.closest_param_on_line(a, direction) {
            Some(t) => a + direction * t.clamp(0.0, length),
            None => a + span * 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ray down -Z from +Z, the canonical "looking at the origin" case.
    fn toward_origin() -> Ray {
        Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0))
    }

    #[test]
    fn new_normalizes_the_direction() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(0.0, 3.0, 0.0));
        assert!((ray.direction - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn at_walks_along_the_direction() {
        assert!((toward_origin().at(5.0) - Vec3::ZERO).length() < 1e-6);
    }

    /// A zero direction component must give an infinite reciprocal, not a NaN —
    /// the slab test relies on the sign of that infinity.
    #[test]
    fn inv_direction_handles_axis_parallel_rays() {
        let inv = toward_origin().inv_direction();
        assert!(inv.x.is_infinite() && inv.y.is_infinite());
        assert!((inv.z + 1.0).abs() < 1e-6);
    }

    #[test]
    fn intersect_aabb_reports_the_near_face() {
        let box_at_origin = Aabb::from_half_extents(Vec3::new(1.0, 1.0, 1.0));
        let hit = toward_origin()
            .intersect_aabb(&box_at_origin)
            .expect("the ray runs straight through the box");
        assert!((hit - 4.0).abs() < 1e-4, "got {hit}");
    }

    #[test]
    fn intersect_aabb_misses_what_it_misses() {
        let elsewhere = Aabb::from_min_max(Vec3::new(10.0, 10.0, 10.0), Vec3::new(11.0, 11.0, 11.0));
        assert_eq!(toward_origin().intersect_aabb(&elsewhere), None);
    }

    #[test]
    fn intersect_plane_finds_the_crossing() {
        let hit = toward_origin()
            .intersect_plane(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0))
            .expect("the ray crosses the XY plane");
        assert!((hit - 5.0).abs() < 1e-4, "got {hit}");
    }

    /// A plane the ray runs along has no single crossing, and must report that
    /// rather than an arbitrary one.
    #[test]
    fn intersect_plane_rejects_a_parallel_plane() {
        assert_eq!(
            toward_origin().intersect_plane(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0)),
            None
        );
    }

    /// A plane behind the origin is not something the ray reaches — returning a
    /// negative distance would let a caller select what is behind the camera.
    #[test]
    fn intersect_plane_ignores_what_is_behind() {
        assert_eq!(
            toward_origin().intersect_plane(Vec3::new(0.0, 0.0, 9.0), Vec3::new(0.0, 0.0, 1.0)),
            None
        );
    }

    #[test]
    fn distance_to_point_measures_perpendicular() {
        let distance = toward_origin().distance_to_point(Vec3::new(2.0, 0.0, 0.0));
        assert!((distance - 2.0).abs() < 1e-4, "got {distance}");
    }

    /// Behind the origin the distance is measured from the origin, so a point
    /// on the backward extension of the line is *not* reported as a hit.
    #[test]
    fn distance_to_point_clamps_behind_the_origin() {
        let distance = toward_origin().distance_to_point(Vec3::new(0.0, 0.0, 9.0));
        assert!((distance - 4.0).abs() < 1e-4, "got {distance}");
    }

    /// The closest approach to the X axis of a ray aimed 2 units along it sits
    /// at t = 2 on that axis.
    #[test]
    fn closest_param_on_line_parameterises_the_other_line() {
        let ray = Ray::new(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
        let t = ray
            .closest_param_on_line(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0))
            .expect("the ray is perpendicular to the X axis");
        assert!((t - 2.0).abs() < 1e-4, "got {t}");
    }

    #[test]
    fn closest_param_on_line_rejects_a_parallel_line() {
        let along_x = Ray::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(
            along_x.closest_param_on_line(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
            None
        );
    }

    /// The segment is finite: an approach beyond its end clamps to that end
    /// instead of running off along the line it lies on.
    #[test]
    fn closest_point_on_segment_clamps_to_the_ends() {
        let ray = Ray::new(Vec3::new(9.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
        let point = ray.closest_point_on_segment(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert!((point - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-4, "got {point:?}");
    }

    /// A degenerate segment has one point, and asking for the closest one must
    /// return it rather than dividing by its zero length.
    #[test]
    fn closest_point_on_segment_survives_a_zero_length_segment() {
        let point = toward_origin().closest_point_on_segment(Vec3::ONE, Vec3::ONE);
        assert_eq!(point, Vec3::ONE);
    }
}
