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

//! 2D shadow atlas — directional and spot lights.
//!
//! Owns the `texture_depth_2d_array` atlas the lane samples for non-point
//! shadow casters. One layer per shadow-casting directional / spot light.
//!
//! The dimensions are passed in by the lane — different quality tiers
//! (`StandardShadowsLane`, `LowResShadowsLane`, …) use different
//! resolutions but the algorithm is identical.

use std::borrow::Cow;
use std::sync::RwLock;

use khora_core::math::{Extent3D, Mat4};
use khora_core::renderer::api::resource::{
    CameraUniformData, ImageAspect, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::api::util::{SampleCount, TextureFormat};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::GraphicsDevice;

use super::pass::{build_draw_cmds, AttachmentPass};

/// 2D atlas resources — owned by whichever shadows lane created it.
pub struct Atlas2D {
    /// Texture id (depth-only).
    pub texture: RwLock<Option<TextureId>>,
    /// `D2Array` view bound by the lit shader at
    /// [`khora_data::render::shadow_bindings::binding::ATLAS_2D`].
    pub view: RwLock<Option<TextureViewId>>,
}

impl Default for Atlas2D {
    fn default() -> Self {
        Self {
            texture: RwLock::new(None),
            view: RwLock::new(None),
        }
    }
}

impl Atlas2D {
    /// Creates the depth atlas + array view at the requested dimensions.
    /// Idempotent — call once at `on_initialize`.
    ///
    /// `resolution` is the per-layer side length in texels;
    /// `max_lights` is the number of array layers (== number of shadow
    /// casters this lane will accept per frame).
    pub fn create(
        &self,
        device: &dyn GraphicsDevice,
        resolution: u32,
        max_lights: u32,
        label: &str,
    ) -> Result<(), RenderError> {
        use crate::render_lane::util::lock::write_lock_render;

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

        let view = device
            .create_texture_view(
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
            )
            .map_err(RenderError::ResourceError)?;

        *write_lock_render(&self.texture, "Atlas2D.texture")? = Some(texture);
        *write_lock_render(&self.view, "Atlas2D.view")? = Some(view);
        Ok(())
    }

    /// Returns the current view id, if any.
    pub fn view_id(&self) -> Option<TextureViewId> {
        self.view.read().ok().and_then(|g| *g)
    }

    /// Destroys the atlas resources. No-op if already destroyed.
    pub fn destroy(&self, device: &dyn GraphicsDevice) {
        if let Some(view) = self.view.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture_view(view) {
                log::warn!("Atlas2D: failed to destroy view: {:?}", e);
            }
        }
        if let Some(texture) = self.texture.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture(texture) {
                log::warn!("Atlas2D: failed to destroy texture: {:?}", e);
            }
        }
    }
}

/// Records one shadow pass for a directional or spot light.
///
/// Pushes the camera UBO into the camera ring, builds the draw command
/// list, and returns an [`AttachmentPass`] ready to be replayed by
/// [`super::pass::record_depth_pass`]. Returns `None` if the camera
/// uniform push fails.
#[allow(clippy::too_many_arguments)]
pub fn collect_pass(
    atlas_view: TextureViewId,
    layer: u32,
    light_pos: khora_core::math::Vec3,
    view_proj: Mat4,
    device: &dyn GraphicsDevice,
    render_world: &khora_data::render::RenderWorld,
    gpu_meshes: &khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
    camera_ring: &mut DynamicUniformRingBuffer,
    model_ring: &mut DynamicUniformRingBuffer,
) -> Option<AttachmentPass> {
    let camera_data = CameraUniformData {
        view_projection: view_proj.to_cols_array_2d(),
        camera_position: [light_pos.x, light_pos.y, light_pos.z, 1.0],
    };
    let camera_offset = match camera_ring.push(device, bytemuck::bytes_of(&camera_data)) {
        Ok(off) => off,
        Err(e) => {
            log::error!("Atlas2D: failed to push camera uniform: {:?}", e);
            return None;
        }
    };
    let camera_bg = *camera_ring.current_bind_group();

    let draw_cmds = build_draw_cmds(device, render_world, gpu_meshes, model_ring);

    Some(AttachmentPass {
        target_view: atlas_view,
        base_array_layer: layer,
        camera_bg,
        camera_offset,
        draw_cmds,
    })
}
