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

//! Centralized texture atlas utility for UI and Text rendering.

use crate::math::{Extent3D, Origin3D, Vec2};
use crate::renderer::api::resource::{
    ImageAspect, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewId,
};
use crate::renderer::api::util::enums::{SampleCount, TextureFormat};
use crate::renderer::api::util::AtlasRect;
use crate::renderer::GraphicsDevice;
use std::borrow::Cow;

/// A dynamic texture atlas that manages allocation and GPU updates.
pub struct TextureAtlas {
    texture: TextureId,
    view: TextureViewId,
    size: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    padding: u32,
}

impl TextureAtlas {
    /// Creates a new texture atlas with the specified size.
    pub fn new(
        device: &dyn GraphicsDevice,
        size: u32,
        format: TextureFormat,
        label: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(Cow::Borrowed(label)),
            size: Extent3D {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
            view_formats: Cow::Borrowed(&[]),
        })?;

        let view = device.create_texture_view(
            texture,
            &TextureViewDescriptor {
                label: Some(Cow::Owned(format!("{}_view", label))),
                format: None,
                dimension: None,
                aspect: ImageAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
            },
        )?;

        // Initialize with black/transparent
        let clear_data = vec![0u8; (size * size * format.bytes_per_pixel() as u32) as usize];
        device.write_texture(
            texture,
            &clear_data,
            Some(format.bytes_per_pixel() as u32 * size),
            Origin3D::default(),
            Extent3D {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        )?;

        Ok(Self {
            texture,
            view,
            size,
            cursor_x: 2,
            cursor_y: 2,
            row_height: 0,
            padding: 2,
        })
    }

    /// Allocates space for a sub-texture and uploads data to the GPU.
    pub fn allocate_and_upload(
        &mut self,
        device: &dyn GraphicsDevice,
        width: u32,
        height: u32,
        pixels: &[u8],
        bytes_per_pixel: u32,
    ) -> Option<AtlasRect> {
        if self.cursor_x + width + self.padding > self.size {
            // New row
            self.cursor_x = self.padding;
            self.cursor_y += self.row_height + self.padding;
            self.row_height = 0;
        }

        if self.cursor_y + height + self.padding > self.size {
            return None; // Atlas full
        }

        let ax = self.cursor_x;
        let ay = self.cursor_y;

        // Perform GPU upload
        device
            .write_texture(
                self.texture,
                pixels,
                Some(width * bytes_per_pixel),
                Origin3D { x: ax, y: ay, z: 0 },
                Extent3D {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            )
            .ok()?;

        self.cursor_x += width + self.padding;
        self.row_height = self.row_height.max(height);

        Some(AtlasRect {
            min: Vec2::new(ax as f32 / self.size as f32, ay as f32 / self.size as f32),
            max: Vec2::new(
                (ax + width) as f32 / self.size as f32,
                (ay + height) as f32 / self.size as f32,
            ),
        })
    }

    pub fn texture(&self) -> TextureId {
        self.texture
    }
    pub fn view(&self) -> TextureViewId {
        self.view
    }
    pub fn size(&self) -> u32 {
        self.size
    }
}
