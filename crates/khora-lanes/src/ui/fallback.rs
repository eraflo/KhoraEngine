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

//! The lane's one-texel fallback: a texture set 2 binds in place of an atlas
//! not made yet, since the layouts need a texture even when nothing samples
//! one. Made the first time an atlas is missing, kept until shutdown.

use std::borrow::Cow;

use khora_core::math::Extent3D;
use khora_core::renderer::api::resource::{
    ImageAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::GraphicsDevice;

use super::lane::UiGpu;

/// The view of the lane's one-texel fallback, made the first time an atlas
/// is missing and kept until shutdown.
pub(super) fn fallback_view(
    built: &mut UiGpu,
    device: &dyn GraphicsDevice,
) -> Result<TextureViewId, ResourceError> {
    if let Some((_, view)) = built.fallback {
        return Ok(view);
    }
    let fallback = fallback_texture(device)?;
    built.fallback = Some(fallback);
    Ok(fallback.1)
}

/// A one-texel texture and its view, for set 2 while an atlas has none.
fn fallback_texture(
    device: &dyn GraphicsDevice,
) -> Result<(TextureId, TextureViewId), ResourceError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("ui_atlas_fallback")),
        size: Extent3D {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: khora_core::renderer::api::resource::SampleCount::X1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;
    let view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("ui_atlas_fallback_view")),
            format: None,
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    );
    match view {
        Ok(view) => Ok((texture, view)),
        Err(error) => {
            if let Err(e) = device.destroy_texture(texture) {
                log::warn!("UiLane: could not destroy the fallback texture: {e:?}");
            }
            Err(error)
        }
    }
}

pub(super) fn destroy_texture(
    device: &dyn GraphicsDevice,
    texture: TextureId,
    view: TextureViewId,
) {
    if let Err(e) = device.destroy_texture_view(view) {
        log::warn!("UiLane: could not destroy the fallback view: {e:?}");
    }
    if let Err(e) = device.destroy_texture(texture) {
        log::warn!("UiLane: could not destroy the fallback texture: {e:?}");
    }
}
