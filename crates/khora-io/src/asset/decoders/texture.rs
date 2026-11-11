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
//! [`TextureColorSpace`](khora_core::renderer::api::resource::TextureColorSpace) —
//! the same PNG is an sRGB albedo map or a linear normal map depending on the
//! slot. Float sources (Radiance `.hdr`, OpenEXR) keep their high dynamic range
//! instead of being flattened to 8-bit, which is what makes an authored
//! environment map usable by the IBL bake.

use anyhow::{Context, Result};
use image::ColorType;
use khora_core::math::Extent3D;
use khora_core::renderer::api::resource::{
    CpuTexture, SampleCount, TextureDimension, TextureFormat, TextureUsage,
};
use khora_core::renderer::api::util::f32_to_f16_bits;

use crate::asset::{AssetDecoder, DecoderRegistration};
use crate::trust::{FileKind, LimitExceeded, Limits, Trust};
use image::ImageDecoder;

/// Decodes common image formats (PNG, JPEG, HDR, EXR, …) into a `CpuTexture`.
#[derive(Clone, Default)]
pub struct TextureDecoder;

impl AssetDecoder<CpuTexture> for TextureDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        // The formats `image` recognises by their first bytes: what
        // `load_from_memory` decodes.
        image::guess_format(head).is_ok()
    }

    fn limits(&self, trust: Trust) -> Limits {
        Limits::of(FileKind::Texture, trust)
    }

    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<CpuTexture, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.load_limited(
            bytes,
            std::path::Path::new(""),
            &Limits::of(FileKind::Texture, Trust::Project),
        )
    }

    fn load_limited(
        &self,
        bytes: &[u8],
        _source_path: &std::path::Path,
        limits: &Limits,
    ) -> Result<CpuTexture, Box<dyn std::error::Error + Send + Sync + 'static>> {
        check_header(bytes, limits)?;
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .context("Failed to read the image's format")?;
        let mut image_limits = image::Limits::default();
        image_limits.max_image_width = Some(limits.max_dimension);
        image_limits.max_image_height = Some(limits.max_dimension);
        image_limits.max_alloc = Some(limits.max_decoded_bytes);
        reader.limits(image_limits);
        let img = reader
            .decode()
            .context("Failed to decode image from memory")?;

        let (width, height) = (img.width(), img.height());
        // Float sources carry HDR range that 8-bit cannot hold. They upload as
        // `Rgba16Float` rather than `Rgba32Float`: half is filterable on every
        // backend, whereas 32-bit float filtering is an optional wgpu feature
        // the environment bake cannot assume.
        let (pixels, format) = if matches!(img.color(), ColorType::Rgb32F | ColorType::Rgba32F) {
            let float_img = img.into_rgba32f();
            let halves: Vec<u8> = float_img
                .as_raw()
                .iter()
                .flat_map(|&c| f32_to_f16_bits(c).to_le_bytes())
                .collect();
            (halves, TextureFormat::Rgba16Float)
        } else {
            // 8-bit layout, color space decided by the consuming slot.
            (img.into_rgba8().into_raw(), TextureFormat::Rgba8Unorm)
        };

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

/// Refuses an image whose header announces more than `limits` allow — a
/// side past the dimension cap, or pixels filling more than the decoded
/// limit — before a pixel is decoded.
fn check_header(bytes: &[u8], limits: &Limits) -> Result<(), LimitExceeded> {
    let Ok(decoder) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ())
        .and_then(|reader| reader.into_decoder().map_err(|_| ()))
    else {
        // An unreadable header is the decode's error to report.
        return Ok(());
    };
    let (width, height) = decoder.dimensions();
    for (side, length) in [("image width", width), ("image height", height)] {
        if length > limits.max_dimension {
            return Err(LimitExceeded {
                what: format!("{side} of {length} pixels"),
                limit: u64::from(limits.max_dimension),
            });
        }
    }
    // What the decode fills: the texture it becomes, 8 bytes a pixel for a
    // float image, 4 otherwise. The decoded image is turned into it without
    // a copy when it already has its layout; `image`'s own limit bounds the
    // decode itself.
    let float = matches!(decoder.color_type(), ColorType::Rgb32F | ColorType::Rgba32F);
    let pixels = u64::from(width) * u64::from(height);
    let decoded = pixels.saturating_mul(if float { 8 } else { 4 });
    if decoded > limits.max_decoded_bytes {
        return Err(LimitExceeded {
            what: format!("{width} × {height} image, {decoded} bytes decoded"),
            limit: limits.max_decoded_bytes,
        });
    }
    Ok(())
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

    // ── Limits ──────────────────────────────────────────────────────────

    use crate::trust::{FileKind, LimitExceeded, Limits, Trust};
    use std::path::Path;

    /// The `LimitExceeded` an error is, or holds among its sources.
    fn exceeded<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a LimitExceeded> {
        let mut next = Some(error);
        while let Some(error) = next {
            if let Some(limit) = error.downcast_ref::<LimitExceeded>() {
                return Some(limit);
            }
            next = error.source();
        }
        None
    }

    /// The CRC-32 a PNG chunk ends with (ISO 3309, reflected).
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// Appends the PNG chunk `kind` holding `data`.
    fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut checked = kind.to_vec();
        checked.extend_from_slice(data);
        png.extend_from_slice(&checked);
        png.extend_from_slice(&crc32(&checked).to_be_bytes());
    }

    /// A PNG of a few dozen bytes whose header announces `width` × `height`
    /// 8-bit RGBA pixels, and which holds none.
    fn png_announcing(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        // 8 bits per channel, RGBA, deflate, no filter method, no interlace.
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &header);
        // An empty zlib stream: the image data starts, and holds no pixel.
        chunk(
            &mut png,
            b"IDAT",
            &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
        );
        chunk(&mut png, b"IEND", &[]);
        png
    }

    /// A real PNG of `width` × `height` pixels.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        png
    }

    /// **An image announcing huge dimensions is refused before decoding**:
    /// a PNG whose header says 100000 × 100000 — 40 GB of pixels — is
    /// refused as a `LimitExceeded` by its header alone, at a trusted
    /// project's limits and at the strict ones; so is one only too wide, or
    /// only too tall, against the dimension cap.
    #[test]
    fn an_image_announcing_huge_dimensions_is_refused_by_its_header() {
        for trust in [Trust::Project, Trust::Restricted] {
            let limits = Limits::of(FileKind::Texture, trust);
            let refused = TextureDecoder
                .load_limited(&png_announcing(100_000, 100_000), Path::new(""), &limits)
                .expect_err("an image of 40 GB of pixels is refused");
            assert!(
                exceeded(&*refused).is_some(),
                "{trust:?}: refused as a LimitExceeded, not: {refused}"
            );
        }

        let limits = Limits::of(FileKind::Texture, Trust::Project);
        for (width, height) in [(100_000, 1), (1, 100_000)] {
            let refused = TextureDecoder
                .load_limited(&png_announcing(width, height), Path::new(""), &limits)
                .expect_err("an image past the dimension cap is refused");
            let limit = exceeded(&*refused)
                .unwrap_or_else(|| panic!("{width}×{height}: a LimitExceeded, not: {refused}"));
            assert_eq!(limit.limit, 16384, "{width}×{height}: the dimension cap");
        }
    }

    /// **An image whose pixels pass the decoded limit is refused**: within
    /// the dimension cap, 100 × 100 RGBA pixels are 40 000 bytes, past a
    /// 1 000-byte `max_decoded_bytes`; under ample limits it decodes.
    #[test]
    fn an_image_whose_pixels_pass_the_decoded_limit_is_refused() {
        let file = png(100, 100);
        let tight = Limits {
            max_bytes: u64::MAX,
            max_decoded_bytes: 1000,
            max_elements: u64::MAX,
            max_dimension: 16384,
        };
        let refused = TextureDecoder
            .load_limited(&file, Path::new(""), &tight)
            .expect_err("40 000 bytes of pixels past a 1 000-byte limit");
        let limit =
            exceeded(&*refused).unwrap_or_else(|| panic!("a LimitExceeded, not: {refused}"));
        assert_eq!(limit.limit, 1000);

        let ample = Limits {
            max_decoded_bytes: 1 << 20,
            ..tight
        };
        let texture = TextureDecoder
            .load_limited(&file, Path::new(""), &ample)
            .unwrap_or_else(|e| panic!("the image decodes within ample limits: {e}"));
        assert_eq!((texture.size.width, texture.size.height), (100, 100));
    }

    /// **A square image at the dimension cap is within the decoded limit**:
    /// an 8-bit RGBA image as wide and as tall as the cap allows — 16384² at a
    /// project's limits, 8192² at the strict ones — fills `width × height × 4`
    /// bytes, exactly its trust's decoded limit (1 GiB, 256 MiB), so its
    /// header is not refused.
    #[test]
    fn a_square_image_at_the_dimension_cap_passes_its_header_check() {
        for (trust, side) in [(Trust::Project, 16384), (Trust::Restricted, 8192)] {
            let limits = Limits::of(FileKind::Texture, trust);
            assert_eq!(
                u64::from(side) * u64::from(side) * 4,
                limits.max_decoded_bytes,
                "{trust:?}: the cap's square fills the decoded limit exactly"
            );
            if let Err(refused) = check_header(&png_announcing(side, side), &limits) {
                panic!("{trust:?}: a {side} × {side} RGBA image is refused: {refused}");
            }
        }
    }

    /// **An image wider than the strict dimension decodes only under the
    /// larger limits**: 16384 × 1 pixels is a trusted project's widest, and
    /// twice an untrusted one's.
    #[test]
    fn an_image_wider_than_the_strict_dimension_decodes_only_under_the_larger_limits() {
        let file = png(16384, 1);
        let texture = TextureDecoder
            .load_limited(
                &file,
                Path::new(""),
                &Limits::of(FileKind::Texture, Trust::Project),
            )
            .unwrap_or_else(|e| panic!("16384 px decode at a project's limits: {e}"));
        assert_eq!(texture.size.width, 16384);

        let refused = TextureDecoder
            .load_limited(
                &file,
                Path::new(""),
                &Limits::of(FileKind::Texture, Trust::Restricted),
            )
            .expect_err("16384 px are past the strict 8192");
        let limit =
            exceeded(&*refused).unwrap_or_else(|| panic!("a LimitExceeded, not: {refused}"));
        assert_eq!(limit.limit, 8192);
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
