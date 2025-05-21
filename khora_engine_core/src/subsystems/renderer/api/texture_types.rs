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

use std::borrow::Cow;
use crate::khora_bitflags;
use crate::math::{Extent3D};
use super::common_types::{SampleCount, TextureFormat};
use super::pipeline_types::{CompareFunction};


/// The dimension of a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureDimension {
    D1,
    D2,
    D3,
}

/// The dimension of a texture view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureViewDimension {
    D1,
    D2,
    D2Array,
    Cube,
    CubeArray,
    D3,
}

/// Which aspects of a texture to view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageAspect {
    All,
    StencilOnly,
    DepthOnly,
}

/// Defines how texture coordinates are handled when sampling outside the [0, 1] range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressMode {
    Repeat,
    ClampToEdge,
    MirrorRepeat,
    ClampToBorder,
}

/// Defines the filtering mode for sampling textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterMode {
    Nearest,
    Linear,
}

/// Border color for `AddressMode::ClampToBorder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SamplerBorderColor {
    TransparentBlack,
    OpaqueBlack,
    OpaqueWhite,
}


khora_bitflags! {
    /// Defines the intended usage of a GPU texture.
    /// These flags can be combined.
    pub struct TextureUsage: u32 {
        const COPY_SRC = 1 << 0;
        const COPY_DST = 1 << 1;

        /// The texture can be bound in a shader for sampling.
        const TEXTURE_BINDING = 1 << 2;

        /// The texture can be used as a storage texture (read/write access from shaders).
        const STORAGE_BINDING = 1 << 3;

        /// The texture can be used as a render target.
        const RENDER_ATTACHMENT = 1 << 4;

        /// The texture can be used as a depth/stencil target (for render attachments).
        const DEPTH_STENCIL_ATTACHMENT = 1 << 5;
    }
}

/// Descriptor for creating a new GPU texture.
#[derive(Debug, Clone)]
pub struct TextureDescriptor<'a> {
    pub label: Option<Cow<'a, str>>,
    pub size: Extent3D,
    pub mip_level_count: u32,
    pub sample_count: SampleCount,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUsage,
    pub view_formats: Cow<'a, [TextureFormat]>,
}

/// Descriptor for creating a texture view.
#[derive(Debug, Clone)]
pub struct TextureViewDescriptor<'a> {
    pub label: Option<Cow<'a, str>>,
    pub format: Option<TextureFormat>,
    pub dimension: Option<TextureViewDimension>,
    pub aspect: ImageAspect,
    pub base_mip_level: u32,
    pub mip_level_count: Option<u32>,
    pub base_array_layer: u32,
    pub array_layer_count: Option<u32>,
}

/// Descriptor for creating a sampler.
#[derive(Debug, Clone)]
pub struct SamplerDescriptor<'a> {
    pub label: Option<Cow<'a, str>>,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub compare: Option<CompareFunction>,
    pub anisotropy_clamp: u16,
    pub border_color: Option<SamplerBorderColor>, // Only used with ClampToBorder address modes
}

/// Opaque handle representing a GPU texture managed by the GraphicsDevice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub usize);

/// Opaque handle representing a GPU texture view managed by the GraphicsDevice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureViewId(pub usize);

/// Opaque handle representing a GPU sampler managed by the GraphicsDevice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SamplerId(pub usize);