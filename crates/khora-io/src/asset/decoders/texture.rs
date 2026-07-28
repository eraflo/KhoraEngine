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

//! Texture decoder: image bytes → `CpuTexture` (via the `image` crate).
//!
//! Auto-registered via [`inventory::submit!`] under the canonical
//! `"texture"` slot. Single canonical implementation — no swap needed.
//!
//! The decoder reports the pixel **layout** only. Whether the values are to be
//! read as sRGB or linear is the *role* of the material slot referencing them,
//! supplied downstream as a
//! [`TextureColorSpace`](khora_core::renderer::api::util::TextureColorSpace) —
//! the same PNG is an sRGB albedo map or a linear normal map depending on the
//! slot. Float sources (Radiance `.hdr`, OpenEXR) keep their high dynamic range
//! instead of being flattened to 8-bit, which is what makes an authored
//! environment map usable by the IBL bake.

use anyhow::{Context, Result};
use image::ColorType;
use khora_core::{
    math::Extent3D,
    renderer::api::{
        resource::{CpuTexture, TextureDimension, TextureUsage},
        util::{f32_to_f16_bits, SampleCount, TextureFormat},
    },
};

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes common image formats (PNG, JPEG, HDR, EXR, …) into a `CpuTexture`.
#[derive(Clone, Default)]
pub struct TextureDecoder;

impl AssetDecoder<CpuTexture> for TextureDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<CpuTexture, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let img = image::load_from_memory(bytes).context("Failed to decode image from memory")?;

        // Float sources carry HDR range that 8-bit cannot hold. They upload as
        // `Rgba16Float` rather than `Rgba32Float`: half is filterable on every
        // backend, whereas 32-bit float filtering is an optional wgpu feature
        // the environment bake cannot assume.
        let (pixels, format) = if matches!(img.color(), ColorType::Rgb32F | ColorType::Rgba32F) {
            let float_img = img.to_rgba32f();
            let halves: Vec<u8> = float_img
                .as_raw()
                .iter()
                .flat_map(|&c| f32_to_f16_bits(c).to_le_bytes())
                .collect();
            (halves, TextureFormat::Rgba16Float)
        } else {
            // 8-bit layout, color space decided by the consuming slot.
            (img.to_rgba8().into_raw(), TextureFormat::Rgba8Unorm)
        };
        let (width, height) = (img.width(), img.height());

        Ok(CpuTexture {
            pixels,
            size: Extent3D {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldr_images_decode_to_a_neutral_eight_bit_layout() {
        // 2x2 PNG, encoded in-memory so the test needs no asset on disk.
        let mut png = Vec::new();
        {
            let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .expect("encode png");
        }
        let tex = TextureDecoder.load(&png).expect("decode png");
        // Layout only — the sRGB/linear decision belongs to the material slot.
        assert_eq!(tex.format, TextureFormat::Rgba8Unorm);
        assert_eq!(tex.size.width, 2);
        assert_eq!(tex.pixels.len(), 2 * 2 * 4);
    }

    #[test]
    fn hdr_images_keep_their_range_as_half_float() {
        // Radiance .hdr written in-memory, with a value well above 1.0 that an
        // 8-bit decode would have clipped.
        let mut hdr = Vec::new();
        {
            let pixels = vec![image::Rgb([4.0f32, 0.5, 0.25]); 4];
            image::codecs::hdr::HdrEncoder::new(&mut hdr)
                .encode(&pixels, 2, 2)
                .expect("encode hdr");
        }
        let tex = TextureDecoder.load(&hdr).expect("decode hdr");
        assert_eq!(tex.format, TextureFormat::Rgba16Float);
        // 4 channels x 2 bytes per half.
        assert_eq!(tex.pixels.len(), 2 * 2 * 4 * 2);
        // The red channel must still exceed 1.0 — the whole point of HDR.
        let red = u16::from_le_bytes([tex.pixels[0], tex.pixels[1]]);
        assert_eq!(red, f32_to_f16_bits(4.0));
    }
}

inventory::submit! {
    DecoderRegistration {
        type_name: "texture",
        register: |svc| {
            svc.register_decoder::<CpuTexture>("texture", TextureDecoder);
        },
    }
}
