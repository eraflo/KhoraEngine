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

use anyhow::{Context, Result};
use khora_core::{
    math::Extent3D,
    renderer::api::{
        resource::{CpuTexture, TextureDimension, TextureUsage},
        util::{SampleCount, TextureFormat},
    },
};

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes common image formats (PNG, JPEG, etc.) into a `CpuTexture`.
#[derive(Clone, Default)]
pub struct TextureDecoder;

impl AssetDecoder<CpuTexture> for TextureDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<CpuTexture, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let img = image::load_from_memory(bytes).context("Failed to decode image from memory")?;

        let rgba_img = img.to_rgba8();
        let (width, height) = rgba_img.dimensions();

        Ok(CpuTexture {
            pixels: rgba_img.into_raw(),
            size: Extent3D {
                width,
                height,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
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
