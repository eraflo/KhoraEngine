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

//! Texture pixel formats and the color space they are read in.

/// Defines the memory format of pixels in a texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    // 8-bit formats
    /// One 8-bit unsigned normalized component.
    R8Unorm,
    /// Two 8-bit unsigned normalized components.
    Rg8Unorm,
    /// Four 8-bit unsigned normalized components (RGBA).
    Rgba8Unorm,
    /// Four 8-bit unsigned normalized components (RGBA) in the sRGB color space.
    Rgba8UnormSrgb,
    /// Four 8-bit unsigned normalized components (BGRA) in the sRGB color space. This is a common swapchain format.
    Bgra8UnormSrgb,
    // 16-bit float formats
    /// One 16-bit float component.
    R16Float,
    /// Two 16-bit float components.
    Rg16Float,
    /// Four 16-bit float components.
    Rgba16Float,
    // 32-bit float formats
    /// One 32-bit float component.
    R32Float,
    /// Two 32-bit float components.
    Rg32Float,
    /// Four 32-bit float components.
    Rgba32Float,
    // Depth/stencil formats
    /// A 16-bit unsigned normalized depth format.
    Depth16Unorm,
    /// A 24-bit unsigned normalized depth format.
    Depth24Plus,
    /// A 24-bit unsigned normalized depth format with an 8-bit stencil component.
    Depth24PlusStencil8,
    /// A 32-bit float depth format.
    Depth32Float,
    /// A 32-bit float depth format with an 8-bit stencil component.
    Depth32FloatStencil8,
}

/// How the values stored in a texture are to be interpreted by the sampler.
///
/// It belongs to the texture's role, not its file: one PNG is a colour map or a
/// data map depending on the material slot using it. The decoder reports the
/// layout, the consumer the colour space, and
/// [`TextureFormat::with_color_space`] combines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureColorSpace {
    /// sRGB-encoded values the sampler linearizes, for authored colour.
    ///
    /// Base colour and emissive maps.
    Srgb,
    /// Linear values sampled verbatim, for data maps and any float/HDR texture.
    ///
    /// Normal, metallic-roughness and occlusion maps: transforming them would
    /// break the lighting math.
    Linear,
}

impl TextureFormat {
    /// Returns the format to upload this decoded layout into for `color_space`.
    ///
    /// Only 8-bit RGBA has both variants, so only it switches. Any other
    /// format, float HDR included, is returned unchanged: it has no sRGB
    /// counterpart, so an sRGB request is ignored.
    pub fn with_color_space(self, color_space: TextureColorSpace) -> Self {
        match self {
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => match color_space {
                TextureColorSpace::Srgb => TextureFormat::Rgba8UnormSrgb,
                TextureColorSpace::Linear => TextureFormat::Rgba8Unorm,
            },
            other => other,
        }
    }

    /// Returns the size in bytes of a single pixel for this format.
    ///
    /// Approximate for packed formats.
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            TextureFormat::R8Unorm => 1,
            TextureFormat::Rg8Unorm => 2,
            TextureFormat::Rgba8Unorm => 4,
            TextureFormat::Rgba8UnormSrgb => 4,
            TextureFormat::Bgra8UnormSrgb => 4,
            TextureFormat::R16Float => 2,
            TextureFormat::Rg16Float => 4,
            TextureFormat::Rgba16Float => 8,
            TextureFormat::R32Float => 4,
            TextureFormat::Rg32Float => 8,
            TextureFormat::Rgba32Float => 16,
            TextureFormat::Depth16Unorm => 2,
            TextureFormat::Depth24Plus => 4,
            TextureFormat::Depth24PlusStencil8 => 4,
            TextureFormat::Depth32Float => 4,
            TextureFormat::Depth32FloatStencil8 => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_bit_rgba_switches_on_color_space() {
        for decoded in [TextureFormat::Rgba8Unorm, TextureFormat::Rgba8UnormSrgb] {
            assert_eq!(
                decoded.with_color_space(TextureColorSpace::Srgb),
                TextureFormat::Rgba8UnormSrgb
            );
            assert_eq!(
                decoded.with_color_space(TextureColorSpace::Linear),
                TextureFormat::Rgba8Unorm
            );
        }
    }

    #[test]
    fn float_formats_ignore_an_srgb_request() {
        // HDR data is linear by construction; asking for sRGB must not corrupt
        // it into a format that would re-transform the values.
        for hdr in [TextureFormat::Rgba16Float, TextureFormat::Rgba32Float] {
            assert_eq!(hdr.with_color_space(TextureColorSpace::Srgb), hdr);
            assert_eq!(hdr.with_color_space(TextureColorSpace::Linear), hdr);
        }
    }

    #[test]
    fn upload_stride_follows_the_resolved_format() {
        // The row stride must come from the resolved format, not a hardcoded
        // 4 bytes/pixel — an HDR upload is twice as wide per pixel.
        assert_eq!(TextureFormat::Rgba8Unorm.bytes_per_pixel(), 4);
        assert_eq!(TextureFormat::Rgba16Float.bytes_per_pixel(), 8);
    }
}
