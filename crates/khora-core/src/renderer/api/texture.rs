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

//! Defines data structures related to GPU texture and sampler resources.

use crate::khora_bitflags;
use crate::math::Extent3D;
use crate::renderer::{CompareFunction, SampleCount, TextureFormat};
use std::borrow::Cow;

/// The dimensionality of a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureDimension {
    /// A one-dimensional texture.
    D1,
    /// A two-dimensional texture.
    D2,
    /// A three-dimensional (volumetric) texture.
    D3,
}

/// The dimensionality of a texture view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureViewDimension {
    /// A view of a 1D texture.
    D1,
    /// A view of a 2D texture.
    D2,
    /// A view of a 2D texture array.
    D2Array,
    /// A view of a cubemap texture (6 faces of a 2D texture).
    Cube,
    /// A view of a cubemap texture array.
    CubeArray,
    /// A view of a 3D texture.
    D3,
}

/// Defines which aspects of a texture are accessed by a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageAspect {
    /// Access all aspects (color, depth, and stencil).
    All,
    /// Access only the stencil component of a depth/stencil texture.
    StencilOnly,
    /// Access only the depth component of a depth/stencil texture.
    DepthOnly,
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

khora_bitflags! {
    /// A set of flags describing the allowed usages of a [`TextureId`].
    pub struct TextureUsage: u32 {
        /// The texture can be used as the source of a copy operation.
        const COPY_SRC = 1 << 0;
        /// The texture can be used as the destination of a copy operation.
        const COPY_DST = 1 << 1;
        /// The texture can be bound in a shader for sampling (reading).
        const TEXTURE_BINDING = 1 << 2;
        /// The texture can be used as a storage texture (read/write access from shaders).
        const STORAGE_BINDING = 1 << 3;
        /// The texture can be used as a color or multisample resolve attachment in a render pass.
        const RENDER_ATTACHMENT = 1 << 4;
        /// The texture can be used as a depth/stencil target (for render attachments).
        const DEPTH_STENCIL_ATTACHMENT = 1 << 5;
    }
}

/// A descriptor used to create a [`TextureId`].
#[derive(Debug, Clone)]
pub struct TextureDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The dimensions (width, height, depth/layers) of the texture.
    pub size: Extent3D,
    /// The number of mipmap levels for the texture.
    pub mip_level_count: u32,
    /// The number of samples per pixel (for multisampling).
    pub sample_count: SampleCount,
    /// The dimensionality of the texture.
    pub dimension: TextureDimension,
    /// The format of the texels in the texture.
    pub format: TextureFormat,
    /// A bitmask of [`TextureUsage`] flags describing how the texture will be used.
    pub usage: TextureUsage,
    /// A list of texture formats that views of this texture can have.
    pub view_formats: Cow<'a, [TextureFormat]>,
}

/// A descriptor used to create a [`TextureViewId`].
/// A view describes a specific way to access a texture's data.
#[derive(Debug, Clone)]
pub struct TextureViewDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The format of the texture view.
    pub format: Option<TextureFormat>,
    /// The dimensionality of the texture view.
    pub dimension: Option<TextureViewDimension>,
    /// The aspects of the texture to be accessed.
    pub aspect: ImageAspect,
    /// The first mipmap level to be accessed by the view.
    pub base_mip_level: u32,
    /// The number of mipmap levels to include in the view.
    pub mip_level_count: Option<u32>,
    /// The first array layer to be accessed by the view.
    pub base_array_layer: u32,
    /// The number of array layers to include in the view.
    pub array_layer_count: Option<u32>,
}

/// A descriptor used to create a [`SamplerId`].
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

/// A CPU-side representation of a decoded texture, ready to be uploaded to the GPU.
#[derive(Debug)]
pub struct CpuTexture {
    /// The raw pixel data (e.g., in RGBA format)
    pub pixels: Vec<u8>,
    /// The size of the texture
    pub size: Extent3D,
    /// The format of the pixel data
    pub format: TextureFormat,
    /// The number of mip levels
    pub mip_level_count: u32,
    /// The number of samples per pixel
    pub sample_count: SampleCount,
    /// The dimensionality of the texture
    pub dimension: TextureDimension,
    /// The allowed usages for the future GPU texture
    pub usage: TextureUsage,
}

impl crate::asset::Asset for CpuTexture {}

impl CpuTexture {
    /// Creates a texture descriptor from this CPU texture data
    pub fn to_descriptor<'a>(&self, label: Option<Cow<'a, str>>) -> TextureDescriptor<'a> {
        TextureDescriptor {
            label,
            size: self.size,
            mip_level_count: self.mip_level_count,
            sample_count: self.sample_count,
            dimension: self.dimension,
            format: self.format,
            usage: self.usage,
            view_formats: Cow::Borrowed(&[]),
        }
    }

    /// Gets the row size in bytes (important for texture upload alignment)
    pub fn row_size(&self) -> usize {
        let bytes_per_pixel = self.format.bytes_per_pixel();
        self.size.width as usize * bytes_per_pixel as usize
    }
}

/// An opaque handle to a GPU texture resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub usize);

/// An opaque handle to a GPU texture view resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureViewId(pub usize);

/// An opaque handle to a GPU sampler resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SamplerId(pub usize);
