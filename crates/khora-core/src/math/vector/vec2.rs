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

//! A 2-component vector.

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::EPSILON;
use std::ops::{Add, Div, Index, IndexMut, Mul, Neg, Sub};

// --- Vec2 ---

/// A 2-dimensional vector with `f32` components.
#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    Serialize,
    Deserialize,
    Encode,
    Decode,
)]
#[repr(C)]
pub struct Vec2 {
    /// The x component of the vector.
    pub x: f32,
    /// The y component of the vector.
    pub y: f32,
}

impl Vec2 {
    /// A vector with all components set to `0.0`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    /// A vector with all components set to `1.0`.
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };
    /// The unit vector pointing along the positive X-axis.
    pub const X: Self = Self { x: 1.0, y: 0.0 };
    /// The unit vector pointing along the positive Y-axis.
    pub const Y: Self = Self { x: 0.0, y: 1.0 };

    /// Creates a new `Vec2` with the specified components.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns a new vector with the absolute value of each component.
    #[inline]
    pub const fn abs(self) -> Self {
        Self {
            x: if self.x < 0.0 { -self.x } else { self.x },
            y: if self.y < 0.0 { -self.y } else { self.y },
        }
    }

    /// Calculates the squared length (magnitude) of the vector.
    /// This is faster than `length()` as it avoids a square root.
    #[inline]
    pub fn length_squared(&self) -> f32 {
        self.dot(*self)
    }

    /// Calculates the length (magnitude) of the vector.
    #[inline]
    pub fn length(&self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized version of the vector with a length of 1.
    /// If the vector's length is near zero, it returns `Vec2::ZERO`.
    #[inline]
    pub fn normalize(&self) -> Self {
        let len_sq = self.length_squared();
        if len_sq > EPSILON * EPSILON {
            *self * (1.0 / len_sq.sqrt())
        } else {
            Self::ZERO
        }
    }

    /// Calculates the dot product of this vector and another.
    #[inline]
    pub fn dot(&self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y
    }

    /// Performs a linear interpolation between two vectors.
    /// The interpolation factor `t` is clamped to the `[0.0, 1.0]` range.
    #[inline]
    pub fn lerp(start: Self, end: Self, t: f32) -> Self {
        start + (end - start) * t.clamp(0.0, 1.0)
    }

    /// Converts the vector to a `[f32; 2]` array.
    #[inline]
    pub fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }
}

impl From<Vec2> for [f32; 2] {
    #[inline]
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}

impl From<[f32; 2]> for Vec2 {
    #[inline]
    fn from(v: [f32; 2]) -> Self {
        Self::new(v[0], v[1])
    }
}

// --- Operator Overloads ---

impl Add for Vec2 {
    type Output = Self;
    /// Adds two vectors component-wise.
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    /// Subtracts two vectors component-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    /// Multiplies the vector by a scalar.
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    /// Multiplies a scalar by a vector.
    #[inline]
    fn mul(self, rhs: Vec2) -> Self::Output {
        rhs * self
    }
}

impl Mul<Vec2> for Vec2 {
    type Output = Self;
    /// Multiplies two vectors component-wise.
    #[inline]
    fn mul(self, rhs: Vec2) -> Self::Output {
        Self {
            x: self.x * rhs.x,
            y: self.y * rhs.y,
        }
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    /// Divides the vector by a scalar.
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        let inv_rhs = 1.0 / rhs;
        Self {
            x: self.x * inv_rhs,
            y: self.y * inv_rhs,
        }
    }
}

impl Neg for Vec2 {
    type Output = Self;
    /// Negates the vector.
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl Index<usize> for Vec2 {
    type Output = f32;
    /// Allows accessing a vector component by index (`v[0]`, `v[1]`).
    ///
    /// # Panics
    /// Panics if `index` is not 0 or 1.
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("Index out of bounds for Vec2"),
        }
    }
}

impl IndexMut<usize> for Vec2 {
    /// Allows mutably accessing a vector component by index (`v[0] = ...`).
    ///
    /// # Panics
    /// Panics if `index` is not 0 or 1.
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => panic!("Index out of bounds for Vec2"),
        }
    }
}
