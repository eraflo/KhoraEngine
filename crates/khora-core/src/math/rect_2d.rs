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

//! A 2D axis-aligned rectangle.

use super::Vec2;

/// A 2D axis-aligned rectangle defined by its top-left corner (`min`)
/// and bottom-right corner (`max`).
///
/// Mirrors the design of [`Aabb`](super::Aabb) for 2D — used by UI surfaces (panels,
/// painters, hit-tests). Y axis points downward, matching most window
/// systems.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct Rect2D {
    /// Top-left corner (smallest x, smallest y).
    pub min: Vec2,
    /// Bottom-right corner (largest x, largest y).
    pub max: Vec2,
}

impl Rect2D {
    /// A zero-sized rectangle anchored at the origin.
    pub const ZERO: Self = Self {
        min: Vec2::ZERO,
        max: Vec2::ZERO,
    };

    /// Creates a rectangle from its `min` and `max` corners.
    pub const fn from_min_max(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    /// Creates a rectangle from a top-left corner and a size.
    pub const fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self {
            min,
            max: Vec2::new(min.x + size.x, min.y + size.y),
        }
    }

    /// Creates a rectangle centered on `center` with the given `size`.
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = Vec2::new(size.x * 0.5, size.y * 0.5);
        Self {
            min: Vec2::new(center.x - half.x, center.y - half.y),
            max: Vec2::new(center.x + half.x, center.y + half.y),
        }
    }

    /// Width of the rectangle.
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    /// Height of the rectangle.
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Size as a [`Vec2`] (`width`, `height`).
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width(), self.height())
    }

    /// Center point of the rectangle.
    pub fn center(&self) -> Vec2 {
        Vec2::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    /// Returns `true` if `point` lies inside this rectangle (inclusive
    /// of `min`, exclusive of `max` — typical hit-test convention).
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }
}
