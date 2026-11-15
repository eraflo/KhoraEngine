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

use khora_core::renderer::api::command::{
    BindGroupId, LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor,
    StoreOp,
};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::IndexFormat;
use khora_core::renderer::api::resource::{BufferId, TextureViewId};
use khora_core::renderer::traits::CommandEncoder;

/// One mesh's draw within a shadow pass, collected before the pass is recorded.
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
    /// Pre-built draw command list for this pass.
    pub draw_cmds: Vec<ShadowDrawCmd>,
}

/// Records one depth-only render pass into `encoder`, clearing its layer first.
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

/// Builds one pass's draws, pushing each mesh's model uniforms into `model_ring`.
///
/// Each pass needs its own list: its ring offsets differ, though the
/// transforms are the same. A mesh whose transform does not invert is skipped.
pub fn build_draw_cmds(
    device: &dyn khora_core::renderer::GraphicsDevice,
    render_world: &khora_data::render::RenderWorld,
    gpu_meshes: &khora_data::assets::Assets<khora_core::renderer::api::gpu_scene::GpuMesh>,
    model_ring: &mut crate::render::util::DynamicUniformRingBuffer,
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
            let model_uniforms = khora_core::renderer::api::gpu_scene::ModelUniforms {
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
