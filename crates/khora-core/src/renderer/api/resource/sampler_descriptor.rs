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

//! The descriptor of a sampler: how a shader samples a texture.

use crate::renderer::api::pipeline::CompareFunction;
use std::borrow::Cow;

/// A descriptor used to create a [`SamplerId`](super::SamplerId).
/// A sampler defines how a shader will sample from a texture.
#[derive(Debug, Clone)]
pub struct SamplerDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The address mode for the U (or S) texture coordinate.
    pub address_mode_u: AddressMode,
    /// The address mode for the V (or T) texture coordinate.
    pub address_mode_v: AddressMode,
    /// The address mode for the W (or R) texture coordinate.
    pub address_mode_w: AddressMode,
    /// The filter mode for magnification (when the texture is larger on screen than its resolution).
    pub mag_filter: FilterMode,
    /// The filter mode for minification (when the texture is smaller on screen than its resolution).
    pub min_filter: FilterMode,
    /// The filter mode to use between mipmap levels.
    pub mipmap_filter: MipmapFilterMode,
    /// The minimum level of detail (LOD) to use for mipmapping.
    pub lod_min_clamp: f32,
    /// The maximum level of detail (LOD) to use for mipmapping.
    pub lod_max_clamp: f32,
    /// If `Some`, creates a comparison sampler for tasks like shadow mapping.
    pub compare: Option<CompareFunction>,
    /// The maximum anisotropy level to use.
    pub anisotropy_clamp: u16,
    /// The border color to use if any address mode is `ClampToBorder`.
    pub border_color: Option<SamplerBorderColor>,
}

/// Defines how texture coordinates are handled when sampling outside the `[0, 1]` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressMode {
    /// Coordinates wrap around. `1.1` becomes `0.1`.
    Repeat,
    /// Coordinates are clamped to the edge. `1.1` becomes `1.0`.
    ClampToEdge,
    /// Coordinates wrap around, mirroring at each integer boundary.
    MirrorRepeat,
    /// Coordinates outside the range are given a fixed border color.
    ClampToBorder,
}

/// Defines the filtering mode for texture sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterMode {
    /// Point sampling. Returns the value of the nearest texel.
    Nearest,
    /// Linear interpolation. Returns a weighted average of the four nearest texels.
    Linear,
}

/// Defines the filtering mode between mipmap levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MipmapFilterMode {
    /// Use the nearest mipmap level.
    Nearest,
    /// Linearly interpolate between the two nearest mipmap levels.
    Linear,
}

/// The border color to use when `AddressMode::ClampToBorder` is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SamplerBorderColor {
    /// A transparent black color `[0.0, 0.0, 0.0, 0.0]`.
    TransparentBlack,
    /// An opaque black color `[0.0, 0.0, 0.0, 1.0]`.
    OpaqueBlack,
    /// An opaque white color `[1.0, 1.0, 1.0, 1.0]`.
    OpaqueWhite,
}
