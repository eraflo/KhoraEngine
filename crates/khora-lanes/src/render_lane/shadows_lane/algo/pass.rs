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

//! Shared types and helpers for the canonical shadow lane's depth passes.
//!
//! Both the 2D atlas (directional / spot) and the cube atlas (point) emit
//! identical depth-only render passes; only the target view changes. This
//! module factors the per-pass record types and the `record_depth_pass`
//! helper so [`super::atlas_2d`] and [`super::atlas_cube`] stay focused on
//! their own data flow.

use khora_core::renderer::api::command::{
    BindGroupId, LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    StoreOp,
};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::{BufferId, TextureViewId};
use khora_core::renderer::api::util::IndexFormat;
use khora_core::renderer::traits::CommandEncoder;

/// Pre-collected draw command for one mesh within a shadow pass.
#[derive(Debug, Clone, Copy)]
pub struct ShadowDrawCmd {
    /// Bind group bound at slot 1 carrying the model uniforms (with a
    /// dynamic offset to address this mesh's slot in the model ring).
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

/// One render pass's worth of state — used for both 2D atlas slots and
/// individual cube faces, hence the generic name.
pub struct AttachmentPass {
    /// Depth target view this pass writes into.
    pub target_view: TextureViewId,
    /// Layer to render into within `target_view`. For 2D atlas passes
    /// this is the light's slot in the array; for cube faces the per-face
    /// view already targets the right layer so this is `0`.
    pub base_array_layer: u32,
    /// Bind group bound at slot 0 carrying the camera (light view-proj)
    /// uniforms with a dynamic offset.
    pub camera_bg: BindGroupId,
    /// Dynamic offset into the camera ring buffer.
    pub camera_offset: u32,
    /// Pre-built draw command list for this pass.
    pub draw_cmds: Vec<ShadowDrawCmd>,
}

/// Records one depth-only render pass into `encoder`.
///
/// Pulled out so [`super::atlas_2d`] and [`super::atlas_cube`] can drive
/// the GPU recorder identically — only their pass-collection step
/// differs.
pub fn record_depth_pass(
    encoder: &mut dyn CommandEncoder,
    pipeline: &RenderPipelineId,
    pass_data: &AttachmentPass,
) {
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
    });

    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &pass_data.camera_bg, &[pass_data.camera_offset]);

    for cmd in &pass_data.draw_cmds {
        pass.set_bind_group(1, &cmd.model_bg, &[cmd.model_offset]);
        pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
        pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
        pass.draw_indexed(0..cmd.index_count, 0, 0..1);
    }
}

/// Helper used by both atlas modules: builds the per-mesh draw command
/// list for one pass by pushing model uniforms into the shared model
/// ring buffer.
///
/// Each pass needs its own list because the ring-buffer push offsets
/// differ between passes — even though the mesh transforms are identical.
pub fn build_draw_cmds(
    device: &dyn khora_core::renderer::GraphicsDevice,
    render_world: &khora_data::render::RenderWorld,
    gpu_meshes: &khora_data::assets::Assets<khora_core::renderer::api::scene::GpuMesh>,
    model_ring: &mut khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer,
) -> Vec<ShadowDrawCmd> {
    let mut draw_cmds = Vec::with_capacity(render_world.meshes.len());
    for mesh in &render_world.meshes {
        if let Some(gpu_mesh) = gpu_meshes.get(&mesh.cpu_mesh_uuid) {
            let model_mat = mesh.transform.to_matrix();
            let normal_mat = if let Some(inv) = model_mat.inverse() {
                inv.transpose()
            } else {
                continue;
            };
            let model_uniforms = khora_core::renderer::api::scene::ModelUniforms {
                model_matrix: model_mat.to_cols_array_2d(),
                normal_matrix: normal_mat.to_cols_array_2d(),
            };
            let model_offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                Ok(off) => off,
                Err(e) => {
                    log::error!("StandardShadowsLane: failed to push model uniform: {:?}", e);
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
    }
    draw_cmds
}
