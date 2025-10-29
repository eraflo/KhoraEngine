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

//! Provides foundational mathematics primitives for 2D and 3D.
//!
//! This module contains a comprehensive set of types and functions for linear algebra
//! and geometry, forming the mathematical backbone of the Khora Engine. It includes
//! vectors, matrices, quaternions, and various utility functions designed for
//! performance and ease of use.
//!
//! All angular functions in this module operate in **radians** by default, unless
//! explicitly specified otherwise (e.g., `degrees_to_radians`).

// --- Fundamental Constants ---

/// A small constant for floating-point comparisons.
pub const EPSILON: f32 = 1e-5;

// Re-export standard mathematical constants for convenience.
pub use std::f32::consts::{
    E, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, FRAC_PI_8, LN_10, LN_2, LOG10_E, LOG2_E, PI,
    SQRT_2, TAU,
};

/// The factor to convert degrees to radians (PI / 180.0).
pub const DEG_TO_RAD: f32 = PI / 180.0;
/// The factor to convert radians to degrees (180.0 / PI).
pub const RAD_TO_DEG: f32 = 180.0 / PI;

// --- Declare Sub-Modules ---

pub mod affine_transform;
pub mod color;
pub mod dimension;
pub mod geometry;
pub mod matrix;
pub mod quaternion;
pub mod vector;

// --- Re-export Principal Types ---

pub use self::color::LinearRgba;
pub use self::dimension::{Extent1D, Extent2D, Extent3D, Origin2D, Origin3D};
pub use self::geometry::Aabb;
pub use self::matrix::{Mat3, Mat4};
pub use self::quaternion::Quaternion;
pub use self::vector::{Vec2, Vec3, Vec4};

// --- Utility Functions ---

/// Converts an angle from degrees to radians.
///
/// # Examples
///
/// ```
/// use khora_core::math::{degrees_to_radians, PI};
/// assert_eq!(degrees_to_radians(180.0), PI);
/// ```
#[inline]
pub fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * DEG_TO_RAD
}

/// Converts an angle from radians to degrees.
///
/// # Examples
///
/// ```
/// use khora_core::math::{radians_to_degrees, PI};
/// assert_eq!(radians_to_degrees(PI), 180.0);
/// ```
#[inline]
pub fn radians_to_degrees(radians: f32) -> f32 {
    radians * RAD_TO_DEG
}

/// Clamps a value to a specified minimum and maximum range.
///
/// # Examples
///
/// ```
/// use khora_core::math::clamp;
/// assert_eq!(clamp(1.5, 0.0, 1.0), 1.0);
/// assert_eq!(clamp(-1.0, 0.0, 1.0), 0.0);
/// assert_eq!(clamp(0.5, 0.0, 1.0), 0.5);
/// ```
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

/// Clamps a floating-point value to the `[0.0, 1.0]` range.
///
/// # Examples
///
/// ```
/// use khora_core::math::saturate;
/// assert_eq!(saturate(1.5), 1.0);
/// assert_eq!(saturate(-0.5), 0.0);
/// ```
#[inline]
pub fn saturate(value: f32) -> f32 {
    clamp(value, 0.0, 1.0)
}

/// Performs an approximate equality comparison between two floats with a custom tolerance.
///
/// # Examples
///
/// ```
/// use khora_core::math::approx_eq_eps;
/// assert!(approx_eq_eps(0.001, 0.002, 1e-2));
/// assert!(!approx_eq_eps(0.001, 0.002, 1e-4));
/// ```
#[inline]
pub fn approx_eq_eps(a: f32, b: f32, epsilon: f32) -> bool {
    (a - b).abs() < epsilon
}

/// Performs an approximate equality comparison using the module's default [`EPSILON`].
///
/// # Examples
///
/// ```
/// use khora_core::math::{approx_eq, EPSILON};
/// assert!(approx_eq(1.0, 1.0 + EPSILON / 2.0));
/// assert!(!approx_eq(1.0, 1.0 + EPSILON * 2.0));
/// ```
#[inline]
pub fn approx_eq(a: f32, b: f32) -> bool {
    approx_eq_eps(a, b, EPSILON)
}
