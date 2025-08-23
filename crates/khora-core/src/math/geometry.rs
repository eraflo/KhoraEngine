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

//! Provides geometric primitive shapes for spatial calculations.
//!
//! This module contains common geometric structures used in collision detection,
//! culling, and other spatial reasoning tasks within the engine.

use super::{Mat4, Vec3, Vec4, EPSILON};

/// Represents an Axis-Aligned Bounding Box (AABB).
///
/// An AABB is a rectangular prism aligned with the coordinate axes, defined by its
/// minimum and maximum corner points. It is a simple but highly efficient volume
/// for broad-phase collision detection and visibility culling.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Aabb {
    /// The corner of the box with the smallest coordinates on all axes.
    pub min: Vec3,
    /// The corner of the box with the largest coordinates on all axes.
    pub max: Vec3,
}

impl Aabb {
    /// An invalid `Aabb` where `min` components are positive infinity and `max` are negative infinity.
    ///
    /// This is useful as a neutral starting point for merging operations. Merging any
    /// valid `Aabb` with `INVALID` will result in that valid `Aabb`.
    pub const INVALID: Self = Self {
        min: Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
        max: Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
    };

    /// Creates a new `Aabb` from two corner points.
    ///
    /// This constructor automatically ensures that the `min` field holds the
    /// component-wise minimum and `max` holds the component-wise maximum,
    /// regardless of the order the points are passed in.
    #[inline]
    pub fn from_min_max(min_pt: Vec3, max_pt: Vec3) -> Self {
        Self {
            min: Vec3::new(
                min_pt.x.min(max_pt.x),
                min_pt.y.min(max_pt.y),
                min_pt.z.min(max_pt.z),
            ),
            max: Vec3::new(
                min_pt.x.max(max_pt.x),
                min_pt.y.max(max_pt.y),
                min_pt.z.max(max_pt.z),
            ),
        }
    }

    /// Creates a new `Aabb` from a center point and its half-extents.
    ///
    /// The half-extents represent the distance from the center to the faces of the box.
    /// The provided `half_extents` will be made non-negative.
    #[inline]
    pub fn from_center_half_extents(center: Vec3, half_extents: Vec3) -> Self {
        let safe_half_extents = Vec3::new(
            half_extents.x.abs(),
            half_extents.y.abs(),
            half_extents.z.abs(),
        ); // Assuming Vec3 doesn't have .abs()
        Self {
            min: center - safe_half_extents,
            max: center + safe_half_extents,
        }
    }

    /// Creates a degenerate `Aabb` containing a single point (min and max are the same).
    #[inline]
    pub fn from_point(point: Vec3) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// Creates an `Aabb` that tightly encloses a given set of points.
    ///
    /// # Returns
    ///
    /// Returns `Some(Aabb)` if the input slice is not empty, otherwise `None`.
    pub fn from_points(points: &[Vec3]) -> Option<Self> {
        if points.is_empty() {
            return None;
        }

        let mut min_pt = points[0];
        let mut max_pt = points[0];

        for point in points.iter().skip(1) {
            min_pt.x = min_pt.x.min(point.x);
            min_pt.y = min_pt.y.min(point.y);
            min_pt.z = min_pt.z.min(point.z);

            max_pt.x = max_pt.x.max(point.x);
            max_pt.y = max_pt.y.max(point.y);
            max_pt.z = max_pt.z.max(point.z);
        }

        Some(Self {
            min: min_pt,
            max: max_pt,
        })
    }

    /// Calculates the center point of the `Aabb`.
    #[inline]
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Calculates the half-extents (half the size on each axis) of the `Aabb`.
    #[inline]
    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// Calculates the full size (width, height, depth) of the `Aabb`.
    #[inline]
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// Checks if the `Aabb` is valid (i.e., `min` <= `max` on all axes).
    /// Degenerate boxes where `min == max` are considered valid.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }

    /// Checks if a point is contained within or on the boundary of the `Aabb`.
    #[inline]
    pub fn contains_point(&self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    /// Checks if this `Aabb` intersects with another `Aabb`.
    ///
    /// Two `Aabb`s intersect if they overlap on all three axes. Boxes that only
    /// touch at the boundary are considered to be intersecting.
    #[inline]
    pub fn intersects_aabb(&self, other: &Aabb) -> bool {
        (self.min.x <= other.max.x && self.max.x >= other.min.x)
            && (self.min.y <= other.max.y && self.max.y >= other.min.y)
            && (self.min.z <= other.max.z && self.max.z >= other.min.z)
    }

    /// Creates a new `Aabb` that encompasses both this `Aabb` and another one.
    #[inline]
    pub fn merge(&self, other: &Aabb) -> Self {
        Self {
            min: Vec3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Vec3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    /// Creates a new `Aabb` that encompasses both this `Aabb` and an additional point.
    #[inline]
    pub fn merged_with_point(&self, point: Vec3) -> Self {
        Self {
            min: Vec3::new(
                self.min.x.min(point.x),
                self.min.y.min(point.y),
                self.min.z.min(point.z),
            ),
            max: Vec3::new(
                self.max.x.max(point.x),
                self.max.y.max(point.y),
                self.max.z.max(point.z),
            ),
        }
    }

    /// Computes the bounding box that encloses this `Aabb` after a transformation.
    ///
    /// This method is more efficient than transforming all 8 corners of the box for
    /// affine transformations. It works by transforming the center of the box and
    /// then calculating the new extents by projecting the original extents onto
    /// the axes of the transformed space.
    ///
    /// # Note
    /// This method is designed for affine transformations (like rotation, translation, scale).
    /// It may not produce the tightest-fitting box for transformations involving perspective.
    pub fn transform(&self, matrix: &Mat4) -> Self {
        let center = self.center();
        let half_extents = self.half_extents();
        let transformed_center_v4 = *matrix * Vec4::from_vec3(center, 1.0);

        let transformed_center = if (transformed_center_v4.w - 1.0).abs() > EPSILON
            && transformed_center_v4.w.abs() > EPSILON
        {
            transformed_center_v4.truncate() / transformed_center_v4.w
        } else {
            transformed_center_v4.truncate()
        };

        let x_abs_col = Vec3::new(
            matrix.cols[0][0].abs(),
            matrix.cols[0][1].abs(),
            matrix.cols[0][2].abs(),
        );
        let y_abs_col = Vec3::new(
            matrix.cols[1][0].abs(),
            matrix.cols[1][1].abs(),
            matrix.cols[1][2].abs(),
        );
        let z_abs_col = Vec3::new(
            matrix.cols[2][0].abs(),
            matrix.cols[2][1].abs(),
            matrix.cols[2][2].abs(),
        );

        let new_half_extents =
            x_abs_col * half_extents.x + y_abs_col * half_extents.y + z_abs_col * half_extents.z;

        Aabb::from_center_half_extents(transformed_center, new_half_extents)
    }
}

impl Default for Aabb {
    /// Returns the default `Aabb`, which is `Aabb::INVALID`.
    #[inline]
    fn default() -> Self {
        Self::INVALID
    }
}

// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{approx_eq, matrix::Mat4, vector::Vec3}; // Use helpers from parent
    use std::f32::consts::PI;

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    // Helper for AABB comparison
    fn aabb_approx_eq(a: Aabb, b: Aabb) -> bool {
        vec3_approx_eq(a.min, b.min) && vec3_approx_eq(a.max, b.max)
    }

    #[test]
    fn test_aabb_from_min_max() {
        let aabb = Aabb::from_min_max(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0));
        assert_eq!(aabb.min, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb.max, Vec3::new(4.0, 5.0, 6.0));

        // Test swapped min/max
        let aabb_swapped = Aabb::from_min_max(Vec3::new(4.0, 5.0, 6.0), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb_swapped.min, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb_swapped.max, Vec3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn test_aabb_from_center_half_extents() {
        let center = Vec3::new(10.0, 20.0, 30.0);
        let half_extents = Vec3::new(1.0, 2.0, 3.0);
        let aabb = Aabb::from_center_half_extents(center, half_extents);

        assert_eq!(aabb.min, Vec3::new(9.0, 18.0, 27.0));
        assert_eq!(aabb.max, Vec3::new(11.0, 22.0, 33.0));
        assert!(aabb_approx_eq(aabb, Aabb::from_min_max(aabb.min, aabb.max)));
    }

    #[test]
    fn test_aabb_from_point() {
        let p = Vec3::new(5.0, 6.0, 7.0);
        let aabb = Aabb::from_point(p);

        assert_eq!(aabb.min, p);
        assert_eq!(aabb.max, p);
        assert!(aabb.is_valid());
    }

    #[test]
    fn test_aabb_from_points() {
        assert!(Aabb::from_points(&[]).is_none());

        let points = [
            Vec3::new(1.0, 5.0, -1.0),
            Vec3::new(0.0, 2.0, 3.0),
            Vec3::new(4.0, 8.0, 0.0),
        ];
        let aabb = Aabb::from_points(&points).unwrap();

        assert_eq!(aabb.min, Vec3::new(0.0, 2.0, -1.0));
        assert_eq!(aabb.max, Vec3::new(4.0, 8.0, 3.0));
    }

    #[test]
    fn test_aabb_utils() {
        let aabb = Aabb::from_min_max(Vec3::new(-1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 5.0));

        assert!(vec3_approx_eq(aabb.center(), Vec3::new(1.0, 1.0, 3.0)));
        assert!(vec3_approx_eq(aabb.size(), Vec3::new(4.0, 2.0, 4.0)));
        assert!(vec3_approx_eq(
            aabb.half_extents(),
            Vec3::new(2.0, 1.0, 2.0)
        ));
        assert!(aabb.is_valid());
        assert!(!Aabb::INVALID.is_valid());
        assert!(Aabb::from_point(Vec3::ZERO).is_valid());
    }

    #[test]
    fn test_aabb_contains_point() {
        let aabb = Aabb::from_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        // Inside
        assert!(aabb.contains_point(Vec3::new(0.5, 0.5, 0.5)));

        // On boundary
        assert!(aabb.contains_point(Vec3::new(0.0, 0.5, 0.5)));
        assert!(aabb.contains_point(Vec3::new(1.0, 0.5, 0.5)));
        assert!(aabb.contains_point(Vec3::new(0.5, 0.0, 0.5)));
        assert!(aabb.contains_point(Vec3::new(0.5, 1.0, 0.5)));
        assert!(aabb.contains_point(Vec3::new(0.5, 0.5, 0.0)));
        assert!(aabb.contains_point(Vec3::new(0.5, 0.5, 1.0)));
        assert!(aabb.contains_point(Vec3::new(0.0, 0.0, 0.0)));
        assert!(aabb.contains_point(Vec3::new(1.0, 1.0, 1.0)));

        // Outside
        assert!(!aabb.contains_point(Vec3::new(1.1, 0.5, 0.5)));
        assert!(!aabb.contains_point(Vec3::new(-0.1, 0.5, 0.5)));
        assert!(!aabb.contains_point(Vec3::new(0.5, 1.1, 0.5)));
        assert!(!aabb.contains_point(Vec3::new(0.5, -0.1, 0.5)));
        assert!(!aabb.contains_point(Vec3::new(0.5, 0.5, 1.1)));
        assert!(!aabb.contains_point(Vec3::new(0.5, 0.5, -0.1)));
    }

    #[test]
    fn test_aabb_intersects_aabb() {
        let aabb1 = Aabb::from_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        // Identical
        let aabb2 = Aabb::from_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
        assert!(aabb1.intersects_aabb(&aabb2));

        // Overlapping
        let aabb3 = Aabb::from_min_max(Vec3::new(1.0, 1.0, 1.0), Vec3::new(3.0, 3.0, 3.0));
        assert!(aabb1.intersects_aabb(&aabb3));
        assert!(aabb3.intersects_aabb(&aabb1));

        // Touching boundary
        let aabb4 = Aabb::from_min_max(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 2.0, 2.0));
        assert!(aabb1.intersects_aabb(&aabb4));
        assert!(aabb4.intersects_aabb(&aabb1));

        // Containing
        let aabb5 = Aabb::from_min_max(Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.5, 1.5, 1.5));
        assert!(aabb1.intersects_aabb(&aabb5));
        assert!(aabb5.intersects_aabb(&aabb1));

        // Non-overlapping X
        let aabb6 = Aabb::from_min_max(Vec3::new(2.1, 0.0, 0.0), Vec3::new(3.0, 2.0, 2.0));
        assert!(!aabb1.intersects_aabb(&aabb6));
        assert!(!aabb6.intersects_aabb(&aabb1));

        // Non-overlapping Y
        let aabb7 = Aabb::from_min_max(Vec3::new(0.0, 2.1, 0.0), Vec3::new(2.0, 3.0, 2.0));
        assert!(!aabb1.intersects_aabb(&aabb7));
        assert!(!aabb7.intersects_aabb(&aabb1));

        // Non-overlapping Z
        let aabb8 = Aabb::from_min_max(Vec3::new(0.0, 0.0, 2.1), Vec3::new(2.0, 2.0, 3.0));
        assert!(!aabb1.intersects_aabb(&aabb8));
        assert!(!aabb8.intersects_aabb(&aabb1));
    }

    #[test]
    fn test_aabb_merge() {
        let aabb1 = Aabb::from_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        let aabb2 = Aabb::from_min_max(Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.5, 1.5, 1.5));
        let merged_aabb = aabb1.merge(&aabb2);

        assert_eq!(merged_aabb.min, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(merged_aabb.max, Vec3::new(1.5, 1.5, 1.5));

        let point = Vec3::new(-1.0, 0.5, 2.0);
        let merged_point = aabb1.merged_with_point(point);

        assert_eq!(merged_point.min, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(merged_point.max, Vec3::new(1.0, 1.0, 2.0));

        // Test merging with invalid starts correctly
        let merged_with_invalid = Aabb::INVALID.merge(&aabb1);
        assert!(aabb_approx_eq(merged_with_invalid, aabb1));

        let merged_with_invalid_pt = Aabb::INVALID.merged_with_point(point);
        assert!(aabb_approx_eq(
            merged_with_invalid_pt,
            Aabb::from_point(point)
        ));
    }

    #[test]
    fn test_aabb_transform() {
        let aabb = Aabb::from_min_max(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)); // Unit cube centered at origin
        let matrix = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)); // Translate +10 on X
        let transformed_aabb = aabb.transform(&matrix);
        let expected_aabb =
            Aabb::from_min_max(Vec3::new(9.0, -1.0, -1.0), Vec3::new(11.0, 1.0, 1.0));

        assert!(aabb_approx_eq(transformed_aabb, expected_aabb));

        // Test with rotation (resulting AABB will be larger)
        let matrix_rot = Mat4::from_rotation_y(PI / 4.0); // Rotate 45 deg around Y
        let transformed_rot_aabb = aabb.transform(&matrix_rot);

        // The exact min/max are harder to calculate manually, but it should contain the original corners rotated
        // Max extent along X/Z should now be sqrt(1^2 + 1^2) = sqrt(2)
        let sqrt2 = 2.0f32.sqrt();

        assert!(approx_eq(transformed_rot_aabb.min.x, -sqrt2));
        assert!(approx_eq(transformed_rot_aabb.max.x, sqrt2));
        assert!(approx_eq(transformed_rot_aabb.min.y, -1.0)); // Y extent shouldn't change
        assert!(approx_eq(transformed_rot_aabb.max.y, 1.0));
        assert!(approx_eq(transformed_rot_aabb.min.z, -sqrt2));
        assert!(approx_eq(transformed_rot_aabb.max.z, sqrt2));

        // Test with scaling
        let matrix_scale = Mat4::from_scale(Vec3::new(2.0, 1.0, 0.5));
        let transformed_scale_aabb = aabb.transform(&matrix_scale);
        let expected_scale_aabb =
            Aabb::from_min_max(Vec3::new(-2.0, -1.0, -0.5), Vec3::new(2.0, 1.0, 0.5));

        assert!(aabb_approx_eq(transformed_scale_aabb, expected_scale_aabb));
    }
}
