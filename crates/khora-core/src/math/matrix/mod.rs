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

//! Defines the `Mat3` and `Mat4` types and associated operations.

use super::{Quaternion, Vec2, Vec3, Vec4, EPSILON};

mod cube;
mod mat3;
mod mat4;

pub use mat3::Mat3;
pub use mat4::Mat4;

/// Whether a matrix with determinant `det` is invertible, `scale` being the
/// product of its column lengths — by Hadamard's inequality the largest
/// magnitude `det` can reach. The ratio is scale-free: it says how far the
/// columns are from lying in a lower-dimensional space, whatever their size.
/// A NaN or an infinity anywhere fails the comparison, and so does a
/// determinant so small (subnormal) that its reciprocal overflows: the
/// inverse would hold infinities.
#[inline]
pub(crate) fn is_invertible(det: f32, scale: f32) -> bool {
    det.is_finite() && scale.is_finite() && det.abs() > EPSILON * scale && (1.0 / det).is_finite()
}

#[cfg(test)]
mod tests;
