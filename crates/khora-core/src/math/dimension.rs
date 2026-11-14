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

//! Provides structs for representing extents (sizes) and origins (offsets) in 1D, 2D, and 3D.
//!
//! These types are commonly used to describe the dimensions of textures, windows, or
//! regions within them. They use integer (`u32`) components, making them suitable
//! for representing pixel-based coordinates and sizes.

pub use super::extent_1d::Extent1D;
pub use super::extent_3d::Extent3D;
pub use super::origin_2d::Origin2D;
pub use super::origin_3d::Origin3D;

/// A two-dimensional extent, typically representing width and height.
///
/// This is commonly used for texture dimensions or window sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent2D {
    /// The width component of the extent.
    pub width: u32,
    /// The height component of the extent.
    pub height: u32,
}
