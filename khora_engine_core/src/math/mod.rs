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

pub const EPSILON: f32 = 1e-5;

pub use std::f32::consts::{
    E, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, FRAC_PI_8, LN_2, LN_10, LOG2_E, LOG10_E, PI,
    SQRT_2, TAU,
};

/// Factor to convert degrees to radians (PI / 180.0).
pub const DEG_TO_RAD: f32 = PI / 180.0;
/// Factor to convert radians to degrees (180.0 / PI).
pub const RAD_TO_DEG: f32 = 180.0 / PI;

pub mod color;
pub mod dimension;
pub mod geometry;
pub mod matrix;
pub mod quaternion;
pub mod vector;

pub use color::LinearRgba;
pub use dimension::{Extent1D, Extent2D, Extent3D, Origin2D, Origin3D};
pub use geometry::Aabb;
pub use matrix::{Mat3, Mat4};
pub use quaternion::Quaternion;
pub use vector::{Vec2, Vec3, Vec4};

// --- Utility Functions ---

/// Converts degrees to radians.
/// ## Arguments
/// * `degrees` - The angle in degrees to convert.
/// ## Returns
/// * The angle in radians.
#[inline]
pub fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * (PI / 180.0)
}

/// Converts radians to degrees.
/// ## Arguments
/// * `radians` - The angle in radians to convert.
/// ## Returns
/// * The angle in degrees.
#[inline]
pub fn radians_to_degrees(radians: f32) -> f32 {
    radians * (180.0 / PI)
}

/// Clamps a value between a minimum and maximum.
/// ## Arguments
/// * `value` - The value to clamp.
/// * `min_val` - The minimum value.
/// * `max_val` - The maximum value.
/// ## Returns
/// * The clamped value.
#[inline]
pub fn clamp<T: PartialOrd>(value: T, min_val: T, max_val: T) -> T {
    if value < min_val {
        min_val
    } else if value > max_val {
        max_val
    } else {
        value
    }
}

/// Clamps a value between 0.0 and 1.0.
/// ## Arguments
/// * `value` - The value to clamp.
/// ## Returns
/// * The clamped value.
#[inline]
pub fn saturate(value: f32) -> f32 {
    clamp(value, 0.0, 1.0)
}

/// Performs approximate equality comparison between two floats.
/// ## Arguments
/// * `a` - The first float.
/// * `b` - The second float.
/// * `epsilon` - The tolerance for the comparison.
/// ## Returns
/// * `true` if the floats are approximately equal, `false` otherwise.
#[inline]
pub fn approx_eq_eps(a: f32, b: f32, epsilon: f32) -> bool {
    (a - b).abs() < epsilon
}

/// Performs approximate equality comparison using the module's default EPSILON.
/// ## Arguments
/// * `a` - The first float.
/// * `b` - The second float.
/// ## Returns
/// * `true` if the floats are approximately equal, `false` otherwise.
#[inline]
pub fn approx_eq(a: f32, b: f32) -> bool {
    approx_eq_eps(a, b, EPSILON)
}
