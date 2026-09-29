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

use super::SimpleUnlitLane;
use khora_core::renderer::api::command::{
    LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::frame::RenderContext;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::RwLock;

impl SimpleUnlitLane {
    pub(super) fn render(
        &self,
        render_world: &RenderWorld,
        device: &dyn khora_core::renderer::GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) {
        use khora_core::renderer::api::gpu_scene::ModelUniforms;
        use khora_core::renderer::api::resource::CameraUniformData;

        // 1. Get Active Camera View.
        //
        // When no camera is present (e.g. editor viewport before any in-scene
        // camera is added), a clear render pass must still be submitted
        let Some(view) = render_world.views.first() else {
            let color_attachment = RenderPassColorAttachment {
                view: render_ctx.color_target,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(render_ctx.clear_color),
                    store: StoreOp::Store,
                },
                base_array_layer: 0,
                base_mip_level: 0,
            };
            let clear_desc = RenderPassDescriptor {
                label: Some("Simple Unlit Clear-Only Pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
                    RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(Operations {
                            load: LoadOp::Clear(1.0),
                            store: StoreOp::Store,
                        }),
                        stencil_ops: None,
                        base_array_layer: 0,
                    }
                }),
            };
            // Dropping the render pass immediately ends it — the clear is
            // recorded into the encoder without any draw calls.
            let _ = encoder.begin_render_pass(&clear_desc);
            return;
        };

        // 2. Prepare Camera Uniforms via Persistent Ring Buffer
        let camera_uniforms = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = {
            let mut lock = crate::lock_or_log!(
                self.camera_ring.lock(),
                "SimpleUnlitLane::render camera_ring"
            );
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("SimpleUnlitLane: camera ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group()
        };

        // Lock the model ring buffer and advance it for this frame
        let mut model_ring_lock =
            crate::lock_or_log!(self.model_ring.lock(), "SimpleUnlitLane::render model_ring");
        let model_ring = match model_ring_lock.as_mut() {
            Some(mr) => {
                mr.advance();
                mr
            }
            None => return,
        };

        let mut material_ring_lock = crate::lock_or_log!(
            self.material_ring.lock(),
            "SimpleUnlitLane::render material_ring"
        );
        let material_ring = match material_ring_lock.as_mut() {
            Some(mr) => {
                mr.advance();
                mr
            }
            None => return,
        };

        // Acquire read locks on the caches
        let gpu_mesh_assets =
            crate::lock_or_log!(gpu_meshes.read(), "SimpleUnlitLane::render gpu_meshes");

        // 3. Prepare Draw Commands
        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());

        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // Get the pre-computed pipeline for this mesh
                let pipeline = self.get_pipeline_for_material(extracted_mesh.material.as_ref());

                // Create Per-Mesh Uniforms
                let model_mat = extracted_mesh.transform.to_matrix();

                let normal_mat = if let Some(inverse) = model_mat.inverse() {
                    inverse.transpose()
                } else {
                    continue; // Skip if degenerate transform
                };

                let mut base_color = khora_core::math::LinearRgba::WHITE;
                if let Some(mat_handle) = &extracted_mesh.material {
                    base_color = mat_handle.base_color();
                }

                let model_uniforms = ModelUniforms {
                    model_matrix: model_mat.to_cols_array_2d(),
                    normal_matrix: normal_mat.to_cols_array_2d(),
                };

                let offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                    Ok(off) => off,
                    Err(_) => continue,
                };
                let model_bg = *model_ring.current_bind_group();

                // Build MaterialUniforms
                let material_uniforms = khora_core::renderer::api::material::MaterialUniforms {
                    base_color,
                    emissive: khora_core::math::LinearRgba::BLACK,
                    ambient: khora_core::math::LinearRgba::BLACK,
                    pbr_factors: [0.0, 1.0, 0.5, 0.0],
                };

                let mat_offset =
                    match material_ring.push(device, bytemuck::bytes_of(&material_uniforms)) {
                        Ok(off) => off,
                        Err(_) => continue,
                    };
                let material_bg = *material_ring.current_bind_group();

                draw_commands.push(khora_core::renderer::api::command::DrawCommand {
                    pipeline,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_format: gpu_mesh_handle.index_format,
                    index_count: gpu_mesh_handle.index_count,
                    model_bind_group: Some(model_bg),
                    model_offset: offset,
                    material_bind_group: Some(material_bg),
                    material_offset: mat_offset,
                });
            }
        }

        // Configure the render pass to render into the provided color target
        let color_attachment = RenderPassColorAttachment {
            view: render_ctx.color_target,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(render_ctx.clear_color),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
            base_mip_level: 0,
        };

        let render_pass_desc = RenderPassDescriptor {
            label: Some("Simple Unlit Pass"),
            color_attachments: &[color_attachment],
            depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
                RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                    base_array_layer: 0,
                }
            }),
        };

        // Begin the render pass
        let mut render_pass = encoder.begin_render_pass(&render_pass_desc);

        // Bind global camera
        render_pass.set_bind_group(0, &camera_bind_group, &[]);

        // Track the last pipeline we bound to avoid redundant state changes
        let mut current_pipeline: Option<RenderPipelineId> = None;

        for cmd in &draw_commands {
            if current_pipeline != Some(cmd.pipeline) {
                render_pass.set_pipeline(&cmd.pipeline);
                current_pipeline = Some(cmd.pipeline);
            }

            if let Some(ref bg) = cmd.model_bind_group {
                render_pass.set_bind_group(1, bg, &[cmd.model_offset]);
            }

            if let Some(ref bg) = cmd.material_bind_group {
                render_pass.set_bind_group(2, bg, &[cmd.material_offset]);
            }

            render_pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
            render_pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
            render_pass.draw_indexed(0..cmd.index_count, 0, 0..1);
        }
    }
}
