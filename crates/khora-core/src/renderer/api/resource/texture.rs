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

pub use super::sampler_descriptor::{
    AddressMode, FilterMode, MipmapFilterMode, SamplerBorderColor, SamplerDescriptor,
};
pub use super::sampler_id::SamplerId;
pub use super::texture_id::TextureId;
pub use super::texture_view_descriptor::{
    ImageAspect, TextureViewDescriptor, TextureViewDimension,
};
pub use super::texture_view_id::TextureViewId;
use crate::khora_bitflags;
use crate::math::Extent3D;
use crate::renderer::api::resource::TextureFormat;
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

/// The number of samples per pixel for Multisample Anti-Aliasing (MSAA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SampleCount {
    /// 1 sample per pixel (MSAA disabled).
    #[default]
    X1,
    /// 2 samples per pixel.
    X2,
    /// 4 samples per pixel.
    X4,
    /// 8 samples per pixel.
    X8,
    /// 16 samples per pixel.
    X16,
    /// 32 samples per pixel.
    X32,
    /// 64 samples per pixel.
    X64,
}

impl CpuTexture {
    /// Builds an HDR texture from linear `f32` RGBA channels, packed to
    /// `Rgba16Float`.
    ///
    /// `rgba` is row-major, 4 channels per pixel, and must hold exactly
    /// `width * height * 4` values. Half-float keeps values above `1.0` (the
    /// point of HDR) while staying filterable on every backend, unlike 32-bit
    /// float. Use for procedurally generated HDR content — an environment map,
    /// a lookup table — that does not come from a decoded file.
    ///
    /// Returns `None` if `rgba`'s length does not match `width * height * 4`.
    pub fn from_rgba32f(width: u32, height: u32, rgba: &[f32]) -> Option<Self> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            return None;
        }
        let pixels = rgba
            .iter()
            .flat_map(|&c| crate::renderer::api::util::f32_to_f16_bits(c).to_le_bytes())
            .collect();
        Some(Self {
            pixels,
            size: Extent3D {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba16Float,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
        })
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rgba32f_packs_to_half_float() {
        let rgba = vec![1.0f32; 2 * 2 * 4];
        let tex = CpuTexture::from_rgba32f(2, 2, &rgba).expect("valid pixel count");
        assert_eq!(tex.format, TextureFormat::Rgba16Float);
        // 4 channels x 2 bytes per half.
        assert_eq!(tex.pixels.len(), 2 * 2 * 4 * 2);
        assert_eq!(tex.row_size(), 2 * 8);
    }

    #[test]
    fn from_rgba32f_preserves_values_above_one() {
        // HDR is the whole reason this constructor exists: an 8-bit path would
        // clip this to white.
        let rgba = vec![8.0f32, 0.5, 0.25, 1.0];
        let tex = CpuTexture::from_rgba32f(1, 1, &rgba).expect("valid pixel count");
        let red = u16::from_le_bytes([tex.pixels[0], tex.pixels[1]]);
        assert_eq!(red, crate::renderer::api::util::f32_to_f16_bits(8.0));
    }

    #[test]
    fn from_rgba32f_rejects_a_mismatched_pixel_count() {
        assert!(CpuTexture::from_rgba32f(2, 2, &[0.0; 4]).is_none());
    }
}
