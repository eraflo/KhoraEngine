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

//! Cube shadow atlas — point lights.
//!
//! Owns the `texture_depth_cube_array` atlas the lane samples for
//! omnidirectional shadow casters. Six layers per shadow-casting point
//! light (one per cube face).
//!
//! The dimensions are passed in by the lane — different quality tiers
//! pick different `face_resolution` and `max_cubes`; the algorithm is
//! identical.

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

/// Cube atlas resources — owned by whichever shadows lane created it.
pub struct AtlasCube {
    /// Underlying 2D-array texture id.
    pub texture: RwLock<Option<TextureId>>,
    /// `CubeArray` view bound by the lit shader at
    /// [`khora_data::render::shadow_bindings::binding::ATLAS_CUBE`].
    pub view: RwLock<Option<TextureViewId>>,
    /// Per-face 2D views used as depth render targets when rasterising
    /// each cube face. Indexed `cube_layer * 6 + face_index` (with
    /// `face_index` matching [`khora_core::math::CubeFace::index`]).
    pub face_views: RwLock<Vec<TextureViewId>>,
}

impl Default for AtlasCube {
    fn default() -> Self {
        Self {
            texture: RwLock::new(None),
            view: RwLock::new(None),
            face_views: RwLock::new(Vec::new()),
        }
    }
}

impl AtlasCube {
    /// Creates the cube depth atlas + cube-array view + per-face views at
    /// the requested dimensions. Idempotent — call once at
    /// `on_initialize`.
    pub fn create(
        &self,
        device: &dyn GraphicsDevice,
        face_resolution: u32,
        max_cubes: u32,
        label: &str,
    ) -> Result<(), RenderError> {
        use crate::render_lane::util::lock::write_lock_render;

        let cube_layers = max_cubes * 6;

        let texture = device
            .create_texture(&TextureDescriptor {
                label: Some(Cow::Owned(format!("{label} Texture"))),
                size: Extent3D {
                    width: face_resolution,
                    height: face_resolution,
                    depth_or_array_layers: cube_layers,
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
                    dimension: Some(TextureViewDimension::CubeArray),
                    aspect: ImageAspect::DepthOnly,
                    base_mip_level: 0,
                    mip_level_count: Some(1),
                    base_array_layer: 0,
                    array_layer_count: Some(cube_layers),
                },
            )
            .map_err(RenderError::ResourceError)?;

        let mut face_views = Vec::with_capacity(cube_layers as usize);
        for layer in 0..cube_layers {
            let face_view = device
                .create_texture_view(
                    texture,
                    &TextureViewDescriptor {
                        label: Some(Cow::Owned(format!("{label} Face View [{}]", layer))),
                        format: Some(TextureFormat::Depth32Float),
                        dimension: Some(TextureViewDimension::D2),
                        aspect: ImageAspect::DepthOnly,
                        base_mip_level: 0,
                        mip_level_count: Some(1),
                        base_array_layer: layer,
                        array_layer_count: Some(1),
                    },
                )
                .map_err(RenderError::ResourceError)?;
            face_views.push(face_view);
        }

        *write_lock_render(&self.texture, "AtlasCube.texture")? = Some(texture);
        *write_lock_render(&self.view, "AtlasCube.view")? = Some(view);
        *write_lock_render(&self.face_views, "AtlasCube.face_views")? = face_views;
        Ok(())
    }

    /// Returns the cube-array view id, if any.
    pub fn view_id(&self) -> Option<TextureViewId> {
        self.view.read().ok().and_then(|g| *g)
    }

    /// Returns a clone of the per-face view list (cheap — just `Vec<TextureViewId>`).
    pub fn face_views_snapshot(&self) -> Vec<TextureViewId> {
        self.face_views
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Destroys the atlas resources. No-op if already destroyed.
    pub fn destroy(&self, device: &dyn GraphicsDevice) {
        if let Ok(mut face_views) = self.face_views.write() {
            for view in face_views.drain(..) {
                if let Err(e) = device.destroy_texture_view(view) {
                    log::warn!("AtlasCube: failed to destroy face view: {:?}", e);
                }
            }
        }
        if let Some(view) = self.view.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture_view(view) {
                log::warn!("AtlasCube: failed to destroy view: {:?}", e);
            }
        }
        if let Some(texture) = self.texture.write().ok().and_then(|mut g| g.take()) {
            if let Err(e) = device.destroy_texture(texture) {
                log::warn!("AtlasCube: failed to destroy texture: {:?}", e);
            }
        }
    }
}

/// Records six shadow passes for one point light (one per cube face).
#[allow(clippy::too_many_arguments)]
pub fn collect_passes(
    cube_face_views: &[TextureViewId],
    cube_layer: u32,
    light_pos: khora_core::math::Vec3,
    face_view_projs: &[Mat4; 6],
    device: &dyn GraphicsDevice,
    render_world: &khora_data::render::RenderWorld,
    gpu_meshes: &khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
    camera_ring: &mut DynamicUniformRingBuffer,
    model_ring: &mut DynamicUniformRingBuffer,
) -> Vec<AttachmentPass> {
    let mut passes = Vec::with_capacity(6);
    for (face_idx, face_vp) in face_view_projs.iter().enumerate() {
        let face_view_index = (cube_layer as usize) * 6 + face_idx;
        let Some(face_view) = cube_face_views.get(face_view_index).copied() else {
            log::error!(
                "AtlasCube: missing cube face view {} (cube_layer={}, face={})",
                face_view_index,
                cube_layer,
                face_idx
            );
            continue;
        };

        let camera_data = CameraUniformData {
            view_projection: face_vp.to_cols_array_2d(),
            camera_position: [light_pos.x, light_pos.y, light_pos.z, 1.0],
        };
        let camera_offset = match camera_ring.push(device, bytemuck::bytes_of(&camera_data)) {
            Ok(off) => off,
            Err(e) => {
                log::error!("AtlasCube: failed to push cube face uniform: {:?}", e);
                continue;
            }
        };
        let camera_bg = *camera_ring.current_bind_group();

        let draw_cmds = build_draw_cmds(device, render_world, gpu_meshes, model_ring);
        passes.push(AttachmentPass {
            target_view: face_view,
            base_array_layer: 0,
            camera_bg,
            camera_offset,
            draw_cmds,
        });
    }
    passes
}
