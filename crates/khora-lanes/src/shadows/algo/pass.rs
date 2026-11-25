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

//! The depth-only passes both shadow atlases record: only their target differs.

use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupId, LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    StoreOp,
};
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::IndexFormat;
use khora_core::renderer::api::resource::{BufferId, CameraUniformData, TextureViewId};
use khora_core::renderer::traits::{CommandEncoder, RenderPassError};
use khora_core::renderer::GraphicsDevice;
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;

use crate::render::util::DynamicUniformRingBuffer;

/// One mesh's draw, the same in every shadow pass of a frame.
#[derive(Debug, Clone, Copy)]
pub struct ShadowDrawCmd {
    /// The model-ring bind group, bound at slot 1.
    pub model_bg: BindGroupId,
    /// Dynamic offset into the model ring buffer.
    pub model_offset: u32,
    /// Vertex buffer to draw from.
    pub vertex_buffer: BufferId,
    /// Index buffer to draw from.
    pub index_buffer: BufferId,
    /// Index count to issue.
    pub index_count: u32,
    /// Index format (`Uint16` / `Uint32`).
    pub index_format: IndexFormat,
}

/// One depth pass, into a 2D atlas layer or a cube face.
pub struct AttachmentPass {
    /// Depth target view this pass writes into.
    pub target_view: TextureViewId,
    /// The layer rendered into: the light's slot in the 2D atlas, or the face's
    /// absolute cube-array layer.
    pub base_array_layer: u32,
    /// The camera-ring bind group (the light's view-projection), bound at slot 0.
    pub camera_bg: BindGroupId,
    /// Dynamic offset into the camera ring buffer.
    pub camera_offset: u32,
}

/// The depth passes of a frame, collected before any is recorded.
pub struct PassRecorder<'a> {
    device: &'a dyn GraphicsDevice,
    camera_ring: &'a mut DynamicUniformRingBuffer,
    passes: Vec<AttachmentPass>,
}

impl<'a> PassRecorder<'a> {
    /// Starts a frame's passes, each light's view pushed into `camera_ring`.
    pub fn new(
        device: &'a dyn GraphicsDevice,
        camera_ring: &'a mut DynamicUniformRingBuffer,
    ) -> Self {
        Self {
            device,
            camera_ring,
            passes: Vec::new(),
        }
    }

    /// Adds a pass into `layer` of `target_view`, seen from `light_pos` through `view_proj`.
    ///
    /// A view the ring refuses is logged, and its light gets no pass.
    pub(crate) fn collect_pass(
        &mut self,
        target_view: TextureViewId,
        layer: u32,
        light_pos: Vec3,
        view_proj: Mat4,
    ) {
        let camera_data = CameraUniformData {
            view_projection: view_proj.to_cols_array_2d(),
            camera_position: [light_pos.x, light_pos.y, light_pos.z, 1.0],
        };
        let pushed = self
            .camera_ring
            .push(self.device, bytemuck::bytes_of(&camera_data));
        let camera_offset = match pushed {
            Ok(offset) => offset,
            Err(e) => {
                log::error!("Shadows: failed to push a light's view uniform: {:?}", e);
                return;
            }
        };
        self.passes.push(AttachmentPass {
            target_view,
            base_array_layer: layer,
            camera_bg: *self.camera_ring.current_bind_group(),
            camera_offset,
        });
    }

    /// Returns the passes collected, in order.
    pub(crate) fn into_passes(self) -> Vec<AttachmentPass> {
        self.passes
    }
}

/// Records one depth-only render pass into `encoder`: clears its layer, then
/// draws every caster of `draws`.
///
/// # Errors
///
/// The [`RenderPassError`] of a pass the encoder refuses; nothing is recorded.
pub(crate) fn record_depth_pass(
    encoder: &mut dyn CommandEncoder,
    pipeline: &RenderPipelineId,
    pass_data: &AttachmentPass,
    draws: &[ShadowDrawCmd],
) -> Result<(), RenderPassError> {
    let depth_attachment = RenderPassDepthStencilAttachment {
        view: &pass_data.target_view,
        depth_ops: Some(Operations {
            load: LoadOp::Clear(1.0),
            store: StoreOp::Store,
        }),
        stencil_ops: None,
        base_array_layer: pass_data.base_array_layer,
    };

    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("Shadow Pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(depth_attachment),
    })?;

    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &pass_data.camera_bg, &[pass_data.camera_offset]);

    for cmd in draws {
        pass.set_bind_group(1, &cmd.model_bg, &[cmd.model_offset]);
        pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
        pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
        pass.draw_indexed(0..cmd.index_count, 0, 0..1);
    }
    Ok(())
}

/// Builds the frame's caster draws, pushing each mesh's model uniforms into `model_ring`.
///
/// Every pass of the frame draws this one list. A mesh whose transform does
/// not invert, or whose uniforms the ring refuses, is left out.
pub(crate) fn build_draw_cmds(
    device: &dyn GraphicsDevice,
    render_world: &RenderWorld,
    gpu_meshes: &Assets<GpuMesh>,
    model_ring: &mut DynamicUniformRingBuffer,
) -> Vec<ShadowDrawCmd> {
    let mut draw_cmds = Vec::with_capacity(render_world.meshes.len());
    for mesh in &render_world.meshes {
        let Some(gpu_mesh) = gpu_meshes.get(&mesh.mesh) else {
            continue;
        };
        let Some(model_uniforms) = ModelUniforms::from_model(mesh.transform.to_matrix()) else {
            continue;
        };
        let model_offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
            Ok(off) => off,
            Err(e) => {
                log::error!("Shadows: failed to push a caster's model uniform: {:?}", e);
                continue;
            }
        };
        draw_cmds.push(ShadowDrawCmd {
            model_bg: *model_ring.current_bind_group(),
            model_offset,
            vertex_buffer: gpu_mesh.vertex_buffer,
            index_buffer: gpu_mesh.index_buffer,
            index_count: gpu_mesh.index_count,
            index_format: gpu_mesh.index_format,
        });
    }
    draw_cmds
}
