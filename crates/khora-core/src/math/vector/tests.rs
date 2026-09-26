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

use super::*; // Import Vec3 from the parent module
use crate::math::approx_eq;

fn vec2_approx_eq(a: Vec2, b: Vec2) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
}

fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
}

// Test Vec2

#[test]
fn test_vec2_new() {
    let v = Vec2::new(1.0, 2.0);
    assert_eq!(v.x, 1.0);
    assert_eq!(v.y, 2.0);
}

#[test]
fn test_vec2_abs() {
    let v = Vec2::new(-1.0, 2.0);
    assert_eq!(v.abs(), Vec2::new(1.0, 2.0));
}

#[test]
fn test_vec2_constants() {
    assert_eq!(Vec2::ZERO, Vec2::new(0.0, 0.0));
    assert_eq!(Vec2::ONE, Vec2::new(1.0, 1.0));
    assert_eq!(Vec2::X, Vec2::new(1.0, 0.0));
    assert_eq!(Vec2::Y, Vec2::new(0.0, 1.0));
}

#[test]
fn test_vec2_ops() {
    let v1 = Vec2::new(1.0, 2.0);
    let v2 = Vec2::new(3.0, 4.0);
    assert_eq!(v1 + v2, Vec2::new(4.0, 6.0));
    assert_eq!(v2 - v1, Vec2::new(2.0, 2.0));
    assert_eq!(v1 * 2.0, Vec2::new(2.0, 4.0));
    assert_eq!(3.0 * v1, Vec2::new(3.0, 6.0));
    assert_eq!(v1 * v2, Vec2::new(3.0, 8.0)); // Component-wise
    assert_eq!(-v1, Vec2::new(-1.0, -2.0));
    assert!(vec2_approx_eq(
        Vec2::new(4.0, 6.0) / 2.0,
        Vec2::new(2.0, 3.0)
    ));
}

#[test]
fn test_vec2_dot() {
    let v1 = Vec2::new(1.0, 2.0);
    let v2 = Vec2::new(3.0, 4.0);
    assert!(approx_eq(v1.dot(v2), 1.0 * 3.0 + 2.0 * 4.0)); // 3 + 8 = 11
}

#[test]
fn test_vec2_length() {
    let v = Vec2::new(3.0, 4.0);
    assert!(approx_eq(v.length_squared(), 25.0));
    assert!(approx_eq(v.length(), 5.0));
    assert!(approx_eq(Vec2::ZERO.length(), 0.0));
}

#[test]
fn test_vec2_normalize() {
    let v1 = Vec2::new(3.0, 0.0);
    let norm_v1 = v1.normalize();
    assert!(vec2_approx_eq(norm_v1, Vec2::X));
    assert!(approx_eq(norm_v1.length(), 1.0));

    let v_zero = Vec2::ZERO;
    assert_eq!(v_zero.normalize(), Vec2::ZERO);
}

#[test]
fn test_vec2_lerp() {
    let start = Vec2::new(0.0, 10.0);
    let end = Vec2::new(10.0, 0.0);
    assert!(vec2_approx_eq(Vec2::lerp(start, end, 0.0), start));
    assert!(vec2_approx_eq(Vec2::lerp(start, end, 1.0), end));
    assert!(vec2_approx_eq(
        Vec2::lerp(start, end, 0.5),
        Vec2::new(5.0, 5.0)
    ));
    // Test clamping
    assert!(vec2_approx_eq(Vec2::lerp(start, end, -0.5), start));
    assert!(vec2_approx_eq(Vec2::lerp(start, end, 1.5), end));
}

#[test]
fn test_vec2_index() {
    let mut v = Vec2::new(5.0, 6.0);
    assert_eq!(v[0], 5.0);
    assert_eq!(v[1], 6.0);
    v[0] = 10.0;
    assert_eq!(v.x, 10.0);
}

#[test]
#[should_panic]
fn test_vec2_index_out_of_bounds() {
    let v = Vec2::new(1.0, 2.0);
    let _ = v[2]; // Should panic
}

// Test Vec3

#[test]
fn test_new() {
    let v = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(v.x, 1.0);
    assert_eq!(v.y, 2.0);
    assert_eq!(v.z, 3.0);
}

#[test]
fn test_vec3_abs() {
    let v = Vec3::new(-1.0, 2.0, -3.0);
    assert_eq!(v.abs(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(Vec3::ZERO.abs(), Vec3::ZERO);
}

#[test]
fn test_constants() {
    assert_eq!(Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(Vec3::ONE, Vec3::new(1.0, 1.0, 1.0));
    assert_eq!(Vec3::X, Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(Vec3::Y, Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(Vec3::Z, Vec3::new(0.0, 0.0, 1.0));
}

#[test]
fn test_add() {
    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, 5.0, 6.0);
    assert_eq!(v1 + v2, Vec3::new(5.0, 7.0, 9.0));
}

#[test]
fn test_sub() {
    let v1 = Vec3::new(5.0, 7.0, 9.0);
    let v2 = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(v1 - v2, Vec3::new(4.0, 5.0, 6.0));
}

#[test]
fn test_scalar_mul() {
    let v = Vec3::new(1.0, 2.0, 3.0);
    assert_eq!(v * 2.0, Vec3::new(2.0, 4.0, 6.0));
    assert_eq!(3.0 * v, Vec3::new(3.0, 6.0, 9.0)); // Test f32 * Vec3
}

#[test]
fn test_component_mul() {
    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, 5.0, 6.0);
    assert_eq!(v1 * v2, Vec3::new(4.0, 10.0, 18.0));
}

#[test]
fn test_scalar_div() {
    let v = Vec3::new(2.0, 4.0, 6.0);
    assert_eq!(v / 2.0, Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn test_neg() {
    let v = Vec3::new(1.0, -2.0, 3.0);
    assert_eq!(-v, Vec3::new(-1.0, 2.0, -3.0));
}

#[test]
fn test_length() {
    let v1 = Vec3::new(3.0, 4.0, 0.0);
    assert!(approx_eq(v1.length_squared(), 25.0));
    assert!(approx_eq(v1.length(), 5.0));

    let v2 = Vec3::ZERO;
    assert!(approx_eq(v2.length_squared(), 0.0));
    assert!(approx_eq(v2.length(), 0.0));
}

#[test]
fn test_dot() {
    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, -5.0, 6.0);
    // 1*4 + 2*(-5) + 3*6 = 4 - 10 + 18 = 12
    assert!(approx_eq(v1.dot(v2), 12.0));

    // Orthogonal vectors
    assert!(approx_eq(Vec3::X.dot(Vec3::Y), 0.0));
}

#[test]
fn test_distance() {
    let v1 = Vec3::new(1.0, 2.0, 3.0);
    let v2 = Vec3::new(4.0, 5.0, 6.0);
    // Distance = sqrt((4-1)^2 + (5-2)^2 + (6-3)^2) = sqrt(9 + 9 + 9) = sqrt(27) = 3*sqrt(3)
    assert!(approx_eq(v1.distance(v2), 3.0 * (3.0_f32).sqrt()));
}

#[test]
fn test_cross() {
    // Standard basis vectors
    assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
    assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
    assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);

    // Anti-commutative property
    assert_eq!(Vec3::Y.cross(Vec3::X), -Vec3::Z);

    // Parallel vectors
    assert_eq!(Vec3::X.cross(Vec3::X), Vec3::ZERO);
}

#[test]
fn test_normalize() {
    let v1 = Vec3::new(3.0, 0.0, 0.0);
    let norm_v1 = v1.normalize();
    assert!(vec3_approx_eq(norm_v1, Vec3::X));
    assert!(approx_eq(norm_v1.length(), 1.0));

    let v2 = Vec3::new(1.0, 1.0, 1.0);
    let norm_v2 = v2.normalize();
    assert!(approx_eq(norm_v2.length(), 1.0)); // Check length is 1

    // Test normalizing zero vector
    let v_zero = Vec3::ZERO;
    assert_eq!(v_zero.normalize(), Vec3::ZERO);
}

#[test]
fn test_lerp() {
    let start = Vec3::new(0.0, 0.0, 0.0);
    let end = Vec3::new(10.0, 10.0, 10.0);

    assert!(vec3_approx_eq(Vec3::lerp(start, end, 0.0), start));
    assert!(vec3_approx_eq(Vec3::lerp(start, end, 1.0), end));
    assert!(vec3_approx_eq(
        Vec3::lerp(start, end, 0.5),
        Vec3::new(5.0, 5.0, 5.0)
    ));
}

// Test Vec4

#[test]
fn test_vec4_new() {
    let v = Vec4::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(v.x, 1.0);
    assert_eq!(v.y, 2.0);
    assert_eq!(v.z, 3.0);
    assert_eq!(v.w, 4.0);
}

#[test]
fn test_vec4_abs() {
    let v = Vec4::new(-1.0, 2.0, -3.0, -0.5);
    assert_eq!(v.abs(), Vec4::new(1.0, 2.0, 3.0, 0.5));
}

#[test]
fn test_vec4_from_vec3() {
    let v3 = Vec3::new(1.0, 2.0, 3.0);
    let v4 = Vec4::from_vec3(v3, 4.0);
    assert_eq!(v4, Vec4::new(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn test_vec4_truncate() {
    let v4 = Vec4::new(1.0, 2.0, 3.0, 4.0);
    let v3 = v4.truncate();
    assert_eq!(v3, Vec3::new(1.0, 2.0, 3.0));
}

#[test]
fn vec3_neg_constants_have_unit_length() {
    assert!((Vec3::NEG_X.length() - 1.0).abs() < EPSILON);
    assert!((Vec3::NEG_Y.length() - 1.0).abs() < EPSILON);
    assert!((Vec3::NEG_Z.length() - 1.0).abs() < EPSILON);
}

#[test]
fn vec3_neg_constants_oppose_positive_axes() {
    assert_eq!(Vec3::NEG_X, -Vec3::X);
    assert_eq!(Vec3::NEG_Y, -Vec3::Y);
    assert_eq!(Vec3::NEG_Z, -Vec3::Z);
    assert_eq!(Vec3::NEG_X.dot(Vec3::X), -1.0);
    assert_eq!(Vec3::NEG_Y.dot(Vec3::Y), -1.0);
    assert_eq!(Vec3::NEG_Z.dot(Vec3::Z), -1.0);
}

#[test]
fn vec3_max_element_picks_largest_signed() {
    assert_eq!(Vec3::new(1.0, 2.0, 3.0).max_element(), 3.0);
    assert_eq!(Vec3::new(-1.0, -2.0, -3.0).max_element(), -1.0);
    assert_eq!(Vec3::new(5.0, -10.0, 0.0).max_element(), 5.0);
    assert_eq!(Vec3::ZERO.max_element(), 0.0);
}

#[test]
fn vec3_min_element_picks_smallest_signed() {
    assert_eq!(Vec3::new(1.0, 2.0, 3.0).min_element(), 1.0);
    assert_eq!(Vec3::new(-1.0, -2.0, -3.0).min_element(), -3.0);
    assert_eq!(Vec3::new(5.0, -10.0, 0.0).min_element(), -10.0);
    assert_eq!(Vec3::ZERO.min_element(), 0.0);
}
