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

//! Texture loading and management.

use super::AssetLoaderLane;
use anyhow::{Context, Result};
use khora_core::{
    math::Extent3D,
    renderer::{
        api::{SampleCount, TextureDimension, TextureFormat, TextureUsage},
        CpuTexture,
    },
};

/// A lane dedicated to loading and decoding texture files on the CPU
#[derive(Clone)]
pub struct TextureLoaderLane;

impl AssetLoaderLane<CpuTexture> for TextureLoaderLane {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<CpuTexture, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // Decode the image using the `image` crate
        let img = image::load_from_memory(bytes).context("Failed to decode image from memory")?;

        // Convert to RGBA8 (keep in sRGB space)
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
