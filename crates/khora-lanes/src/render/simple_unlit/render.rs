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

//! Recording the lane's draw calls for one frame.

use super::gpu::UnlitGpu;
use crate::render::recording::{record_clear, scene_color, scene_depth};
use crate::render::UnlitInputs;
use khora_core::asset::Material;
use khora_core::math::LinearRgba;
use khora_core::renderer::api::command::{DrawCommand, LoadOp, RenderPassDescriptor};
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::material::MaterialUniforms;
use khora_core::renderer::api::resource::CameraUniformData;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::GraphicsDevice;
use khora_data::assets::Assets;

impl UnlitGpu {
    /// Records the scene pass into `encoder`: a clear without a camera, else
    /// every mesh `gpu_meshes` holds, flat.
    ///
    /// A mesh is drawn in the base colour of the material `materials` holds
    /// under its material uuid, white when there is none.
    pub(super) fn record(
        &mut self,
        inputs: &UnlitInputs<'_>,
        gpu_meshes: &Assets<GpuMesh>,
        materials: &Assets<Box<dyn Material>>,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
    ) -> Result<(), RenderError> {
        let render_world = inputs.render_world;
        let target = &inputs.target;
        let clear = inputs.clear_color.0;
        let (Some(view), Some((_, pipeline))) = (render_world.views.first(), self.pipeline) else {
            record_clear(encoder, target, clear, "Simple Unlit Clear-Only Pass")?;
            return Ok(());
        };

        let camera = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };
        self.camera_ring.advance();
        self.camera_ring
            .write(device, bytemuck::bytes_of(&camera))
            .map_err(RenderError::ResourceError)?;
        let camera_bind_group = *self.camera_ring.current_bind_group();
        self.model_ring.advance();
        self.material_ring.advance();

        let mut draws = Vec::with_capacity(render_world.meshes.len());
        for mesh in &render_world.meshes {
            let Some(gpu_mesh) = gpu_meshes.get(&mesh.mesh) else {
                continue;
            };
            // A degenerate transform has no normal matrix: nothing to draw.
            let Some(model_uniforms) = ModelUniforms::from_model(mesh.transform.to_matrix()) else {
                continue;
            };
            let base_color = mesh
                .material
                .and_then(|uuid| materials.get(&uuid))
                .map_or(LinearRgba::WHITE, |material| material.base_color());
            let material_uniforms = MaterialUniforms {
                base_color,
                emissive: LinearRgba::BLACK,
                ambient: LinearRgba::BLACK,
                pbr_factors: [0.0, 1.0, 0.5, 0.0],
                specular: [1.0, 1.0, 1.0, 1.0],
            };
            let model_offset = self
                .model_ring
                .push(device, bytemuck::bytes_of(&model_uniforms))
                .map_err(RenderError::ResourceError)?;
            let material_offset = self
                .material_ring
                .push(device, bytemuck::bytes_of(&material_uniforms))
                .map_err(RenderError::ResourceError)?;
            draws.push(DrawCommand {
                pipeline,
                vertex_buffer: gpu_mesh.vertex_buffer,
                index_buffer: gpu_mesh.index_buffer,
                index_format: gpu_mesh.index_format,
                index_count: gpu_mesh.index_count,
                model_bind_group: Some(*self.model_ring.current_bind_group()),
                model_offset,
                material_bind_group: Some(*self.material_ring.current_bind_group()),
                material_offset,
            });
        }

        let color = [scene_color(target, LoadOp::Clear(clear))];
        let descriptor = RenderPassDescriptor {
            label: Some("Simple Unlit Pass"),
            color_attachments: &color,
            depth_stencil_attachment: Some(scene_depth(target, LoadOp::Clear(1.0))),
        };
        let mut pass = encoder.begin_render_pass(&descriptor)?;
        pass.set_bind_group(0, &camera_bind_group, &[]);
        pass.set_pipeline(&pipeline);
        // The unlit material is a dynamic-offset uniform, unlike the lit
        // lanes' cached material bind groups.
        for draw in &draws {
            if let Some(ref group) = draw.model_bind_group {
                pass.set_bind_group(1, group, &[draw.model_offset]);
            }
            if let Some(ref group) = draw.material_bind_group {
                pass.set_bind_group(2, group, &[draw.material_offset]);
            }
            pass.set_vertex_buffer(0, &draw.vertex_buffer, 0);
            pass.set_index_buffer(&draw.index_buffer, 0, draw.index_format);
            pass.draw_indexed(0..draw.index_count, 0, 0..1);
        }
        Ok(())
    }
}
