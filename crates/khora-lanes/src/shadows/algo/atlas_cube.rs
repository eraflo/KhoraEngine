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
//! The `texture_depth_cube_array` atlas the lit lanes sample for
//! omnidirectional shadow casters. Six layers per shadow-casting point light
//! (one per cube face). The face resolution is the lane level's.

use std::borrow::Cow;

use crate::render_lane::util::DynamicUniformRingBuffer;
use khora_core::math::{Extent3D, Mat4};
use khora_core::renderer::api::resource::{
    CameraUniformData, ImageAspect, TextureDescriptor, TextureDimension, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::GraphicsDevice;

use super::pass::{build_draw_cmds, AttachmentPass};

/// A cube atlas on the device: its depth texture, the cube-array view the lit
/// lanes bind, and one 2D view per face to render into.
pub struct AtlasCube {
    /// The depth texture, six layers per cube.
    pub texture: TextureId,
    /// The `CubeArray` view bound by the lit shader at
    /// [`khora_core::renderer::api::shadow::bindings::binding::ATLAS_CUBE`].
    pub view: TextureViewId,
    /// Per-face 2D views used as depth render targets. Indexed
    /// `cube_layer * 6 + face_index` (with `face_index` matching
    /// [`khora_core::math::CubeFace::index`]).
    pub face_views: Vec<TextureViewId>,
}

impl AtlasCube {
    /// Creates the cube depth atlas, its cube-array view and its face views:
    /// `face_resolution` texels a side, `max_cubes` cubes. Whatever was made
    /// before a refusal is given back.
    pub fn create(
        device: &dyn GraphicsDevice,
        face_resolution: u32,
        max_cubes: u32,
        label: &str,
    ) -> Result<Self, RenderError> {
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

        let mut made = Vec::with_capacity(cube_layers as usize + 1);
        let built = make_views(device, texture, cube_layers, label, &mut made);
        match built {
            Ok((view, face_views)) => Ok(Self {
                texture,
                view,
                face_views,
            }),
            Err(error) => {
                for view in made {
                    if let Err(e) = device.destroy_texture_view(view) {
                        log::warn!("AtlasCube: failed to destroy view: {:?}", e);
                    }
                }
                if let Err(e) = device.destroy_texture(texture) {
                    log::warn!("AtlasCube: failed to destroy texture: {:?}", e);
                }
                Err(RenderError::ResourceError(error))
            }
        }
    }

    /// Gives the atlas back to the device.
    pub fn destroy(self, device: &dyn GraphicsDevice) {
        for view in self.face_views {
            if let Err(e) = device.destroy_texture_view(view) {
                log::warn!("AtlasCube: failed to destroy face view: {:?}", e);
            }
        }
        if let Err(e) = device.destroy_texture_view(self.view) {
            log::warn!("AtlasCube: failed to destroy view: {:?}", e);
        }
        if let Err(e) = device.destroy_texture(self.texture) {
            log::warn!("AtlasCube: failed to destroy texture: {:?}", e);
        }
    }
}

/// The cube-array view and the face views of `texture`, each recorded in
/// `made` as it is created, so a refusal can give back the ones before it.
fn make_views(
    device: &dyn GraphicsDevice,
    texture: TextureId,
    cube_layers: u32,
    label: &str,
    made: &mut Vec<TextureViewId>,
) -> Result<(TextureViewId, Vec<TextureViewId>), ResourceError> {
    let mut make = |descriptor: TextureViewDescriptor<'_>| -> Result<TextureViewId, ResourceError> {
        let view = device.create_texture_view(texture, &descriptor)?;
        made.push(view);
        Ok(view)
    };
    let view = make(TextureViewDescriptor {
        label: Some(Cow::Owned(format!("{label} View"))),
        format: Some(TextureFormat::Depth32Float),
        dimension: Some(TextureViewDimension::CubeArray),
        aspect: ImageAspect::DepthOnly,
        base_mip_level: 0,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(cube_layers),
    })?;
    let mut face_views = Vec::with_capacity(cube_layers as usize);
    for layer in 0..cube_layers {
        face_views.push(make(TextureViewDescriptor {
            label: Some(Cow::Owned(format!("{label} Face View [{layer}]"))),
            format: Some(TextureFormat::Depth32Float),
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::DepthOnly,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: layer,
            array_layer_count: Some(1),
        })?);
    }
    Ok((view, face_views))
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
    gpu_meshes: &khora_data::assets::Assets<khora_core::renderer::api::gpu_scene::GpuMesh>,
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
            // The depth attachment path recreates the render target from the
            // source texture + this layer index (it ignores the view's own
            // layer), so it MUST be the absolute cube-array layer — passing 0
            // would collapse every face of every cube into layer 0. Matches the
            // 2D atlas, which passes its real cascade layer.
            target_view: face_view,
            base_array_layer: face_view_index as u32,
            camera_bg,
            camera_offset,
            draw_cmds,
        });
    }
    passes
}
