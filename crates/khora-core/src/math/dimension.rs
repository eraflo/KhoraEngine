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

/// A one-dimensional extent, typically representing a width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent1D {
    /// The width component of the extent.
    pub width: u32,
}

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

/// A three-dimensional extent, representing width, height, and depth.
///
/// This is used for 3D textures, texture arrays, or cubemaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent3D {
    /// The width component of the extent.
    pub width: u32,
    /// The height component of the extent.
    pub height: u32,
    /// The depth or number of array layers.
    pub depth_or_array_layers: u32,
}

/// A two-dimensional origin, typically representing an (x, y) offset.
///
/// This is often used to specify the top-left corner of a rectangular region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin2D {
    /// The x-coordinate of the origin.
    pub x: u32,
    /// The y-coordinate of the origin.
    pub y: u32,
}

/// A three-dimensional origin, representing an (x, y, z) offset.
///
/// This is often used to specify the corner of a 3D volume or an offset
/// into a texture array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin3D {
    /// The x-coordinate of the origin.
    pub x: u32,
    /// The y-coordinate of the origin.
    pub y: u32,
    /// The z-coordinate or array layer of the origin.
    pub z: u32,
}
