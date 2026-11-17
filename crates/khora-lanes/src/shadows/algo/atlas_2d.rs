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

//! The 2D shadow atlas: a `texture_depth_2d_array` with one layer per
//! shadow-casting directional or spot light.

use std::borrow::Cow;

use khora_core::math::Extent3D;
use khora_core::renderer::api::resource::{
    ImageAspect, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::GraphicsDevice;

/// A 2D atlas on the device: its depth texture and the array view the lit
/// lanes bind.
pub struct Atlas2D {
    /// The depth texture.
    pub texture: TextureId,
    /// The `D2Array` view bound by the lit shader at
    /// [`khora_core::renderer::api::shadow::bindings::binding::ATLAS_2D`].
    pub view: TextureViewId,
}

impl Atlas2D {
    /// Creates the depth atlas and its array view, one layer per light up to `max_lights`.
    ///
    /// `resolution` is in texels a side. A view the device refuses gives back
    /// the texture already made.
    pub(crate) fn create(
        device: &dyn GraphicsDevice,
        resolution: u32,
        max_lights: u32,
        label: &str,
    ) -> Result<Self, RenderError> {
        let texture = device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Owned(format!("{label} Texture"))),
                size: Extent3D {
                    width: resolution,
                    height: resolution,
                    depth_or_array_layers: max_lights,
                },
                mip_level_count: 1,
                sample_count: SampleCount::X1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
                view_formats: Cow::Borrowed(&[]),
            })
            .map_err(RenderError::ResourceError)?;

        let view = device.create_texture_view(
            texture,
            &TextureViewDescriptor {
                label: Some(Cow::Owned(format!("{label} View"))),
                format: Some(TextureFormat::Depth32Float),
                dimension: Some(TextureViewDimension::D2Array),
                aspect: ImageAspect::DepthOnly,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(max_lights),
            },
        );
        match view {
            Ok(view) => Ok(Self { texture, view }),
            Err(error) => {
                if let Err(e) = device.destroy_texture(texture) {
                    log::warn!("Atlas2D: failed to destroy texture: {:?}", e);
                }
                Err(RenderError::ResourceError(error))
            }
        }
    }

    /// Gives the atlas back to the device.
    pub fn destroy(self, device: &dyn GraphicsDevice) {
        if let Err(e) = device.destroy_texture_view(self.view) {
            log::warn!("Atlas2D: failed to destroy view: {:?}", e);
        }
        if let Err(e) = device.destroy_texture(self.texture) {
            log::warn!("Atlas2D: failed to destroy texture: {:?}", e);
        }
    }
}
