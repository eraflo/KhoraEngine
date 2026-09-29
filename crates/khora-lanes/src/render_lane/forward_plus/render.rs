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

use super::g3;
use khora_data::render::RenderWorld;

use super::gpu::render_pipeline_spec;
use super::ForwardPlusLane;
use khora_core::renderer::api::command::{
    ComputePassDescriptor, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::frame::RenderContext;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::CameraUniformData;
use khora_core::renderer::traits::CommandEncoder;
use khora_data::assets::Assets;
use std::sync::RwLock;

impl ForwardPlusLane {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &self,
        render_world: &RenderWorld,
        shadow_entries: &khora_core::renderer::api::shadow::ShadowEntries,
        shadow_bindings: Option<khora_core::renderer::api::shadow::ShadowGpuBindings>,
        ibl_bindings: Option<khora_core::renderer::api::ibl::IblGpuBindings>,
        device: &dyn khora_core::renderer::GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        transparent_encoder: Option<&mut dyn CommandEncoder>,
        render_ctx: &RenderContext,
        gpu_meshes: &RwLock<Assets<GpuMesh>>,
    ) {
        let mut resources =
            crate::lock_or_log!(self.gpu_resources.lock(), "ForwardPlusLane::render");

        // 1. Get Active Camera View.
        //
        // When no camera is present (e.g. editor viewport before any in-scene
        // camera is added), a clear render pass must still be submitted
        let Some(view) = render_world.views.first() else {
            let color_attachment = khora_core::renderer::api::command::RenderPassColorAttachment {
                view: render_ctx.color_target,
                resolve_target: None,
                ops: khora_core::renderer::api::command::Operations {
                    load: khora_core::renderer::api::command::LoadOp::Clear(render_ctx.clear_color),
                    store: khora_core::renderer::api::command::StoreOp::Store,
                },
                base_array_layer: 0,
                base_mip_level: 0,
            };
            let clear_desc = khora_core::renderer::api::command::RenderPassDescriptor {
                label: Some("ForwardPlus Clear-Only Pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
                    khora_core::renderer::api::command::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(khora_core::renderer::api::command::Operations {
                            load: khora_core::renderer::api::command::LoadOp::Clear(1.0),
                            store: khora_core::renderer::api::command::StoreOp::Store,
                        }),
                        stencil_ops: None,
                        base_array_layer: 0,
                    }
                }),
            };
            let _ = encoder.begin_render_pass(&clear_desc);
            return;
        };

        // 2. Prepare Camera Uniforms (Group 0)
        let camera_uniforms = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = if let Some(ref mut ring) = resources.camera_ring {
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group()
        } else {
            return;
        };

        // 3. Prepare Tile Info (sent to Group 3)
        if let Some(tile_buffer) = resources.tile_info_buffer {
            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());
            let tile_info = [
                num_tiles_x,
                num_tiles_y,
                config.tile_size.pixels(),
                config.max_lights_per_tile,
            ];
            if let Err(e) = device.write_buffer(tile_buffer, 0, bytemuck::cast_slice(&tile_info)) {
                log::error!("ForwardPlusLane::render: tile info buffer write failed: {e:?}");
            }
        }

        // 4. Update Light Data — fold per-light shadow metadata from the
        // ShadowFrame into each GpuLight, and accumulate a parallel
        // `shadow_view_projs` buffer indexed identically to `lights`.
        //
        // Directional / spot: `shadow_map_index` = 2D atlas layer,
        // `shadow_view_projs[i]` carries the per-light VP matrix.
        // Point: `shadow_map_index` = cube atlas index,
        // `shadow_far_plane` = perspective far plane used by the shadow
        // pass (matches `ShadowEntry::Cube.far_plane`).
        use khora_core::renderer::api::shadow::ShadowEntry;
        let lights: Vec<_> = render_world
            .lights
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut gl = khora_core::renderer::GpuLight::from_parts(
                    [l.position.x, l.position.y, l.position.z],
                    [l.direction.x, l.direction.y, l.direction.z],
                    &l.light_type,
                );
                match shadow_entries.get(i) {
                    Some(ShadowEntry::Atlas2D { atlas_index, .. }) => {
                        gl.shadow_map_index = *atlas_index;
                    }
                    Some(ShadowEntry::Cube {
                        cube_array_index,
                        far_plane,
                        ..
                    }) => {
                        gl.shadow_map_index = *cube_array_index;
                        gl.shadow_far_plane = *far_plane;
                    }
                    None => {}
                }
                gl
            })
            .collect();

        if let Some(light_buffer) = resources.light_buffer {
            if let Err(e) = device.write_buffer(light_buffer, 0, bytemuck::cast_slice(&lights)) {
                log::error!("ForwardPlusLane::render: light data buffer write failed: {e:?}");
            }
        }

        // Parallel shadow view-projection matrices — directional / spot
        // pull from the entry's `view_proj`, everything else gets the
        // identity (bypassed via `shadow_map_index < 0` early-out in
        // `sample_shadow_pcf`).
        let shadow_view_projs: Vec<[[f32; 4]; 4]> = render_world
            .lights
            .iter()
            .enumerate()
            .map(|(i, _)| match shadow_entries.get(i) {
                Some(ShadowEntry::Atlas2D { view_proj, .. }) => view_proj.to_cols_array_2d(),
                _ => khora_core::math::Mat4::IDENTITY.to_cols_array_2d(),
            })
            .collect();

        if let Some(svp_buffer) = resources.shadow_view_projs_buffer {
            if !shadow_view_projs.is_empty() {
                if let Err(e) =
                    device.write_buffer(svp_buffer, 0, bytemuck::cast_slice(&shadow_view_projs))
                {
                    log::error!(
                        "ForwardPlusLane::render: shadow view-proj buffer write failed: {e:?}"
                    );
                }
            }
        }

        // Prepare and write Culling Uniforms
        if let Some(culling_buffer) = resources.culling_uniforms_buffer {
            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());

            let inv_vp = view.view_proj.inverse().unwrap_or_default();

            let culling_data = khora_core::renderer::light::CullingUniformsData {
                view_projection: view.view_proj.to_cols_array_2d(),
                inverse_projection: inv_vp.to_cols_array_2d(),
                screen_dimensions: [width as f32, height as f32],
                tile_count: [num_tiles_x, num_tiles_y],
                num_lights: lights.len() as u32,
                tile_size: config.tile_size.pixels(),
                _padding: [0.0; 2],
            };

            if let Err(e) =
                device.write_buffer(culling_buffer, 0, bytemuck::bytes_of(&culling_data))
            {
                log::error!("ForwardPlusLane::render: culling uniforms buffer write failed: {e:?}");
            }
        }

        // Run Culling Compute Pass
        if let (Some(culling_pipeline), Some(culling_bg)) =
            (resources.culling_pipeline, resources.culling_bind_group)
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Forward+ Light Culling Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&culling_pipeline);
            compute_pass.set_bind_group(0, &culling_bg, &[]);

            let config = self.tile_config;
            let (width, height) = self.screen_size;
            let num_tiles_x = width.div_ceil(config.tile_size.pixels());
            let num_tiles_y = height.div_ceil(config.tile_size.pixels());
            compute_pass.dispatch_workgroups(num_tiles_x, num_tiles_y, 1);
        }

        // 5. Prepare Per-Mesh Data (Dynamic Uniforms). Opaque draws batch by
        // pipeline; blended draws are deferred to a back-to-front sorted batch.
        let mut draw_commands = Vec::new();
        let mut transparent_draws: Vec<(f32, khora_core::renderer::api::command::DrawCommand)> =
            Vec::new();

        if let Some(ref mut ring) = resources.model_ring {
            ring.advance();
        }

        // Fallback pipeline (empty variant) used only if the per-variant
        // resolve fails.
        let fallback_pipeline = resources.render_pipeline.unwrap_or(RenderPipelineId(0));

        let gpu_mesh_assets = crate::lock_or_log!(gpu_meshes.read(), "ForwardPlusLane::render");
        for extracted_mesh in &render_world.meshes {
            if let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) {
                // The material projection tags every rendered entity with a
                // `GpuMaterial` before RenderFlow runs.
                let Some(gpu_material) = &extracted_mesh.gpu_material else {
                    continue;
                };

                // Resolve the pipeline for this material's texture variant
                // (cheap cache hit after first compile; never retained).
                let pipeline_id = self
                    .pipeline_system
                    .get()
                    .map(|ps| {
                        ps.pipeline(
                            device,
                            &render_pipeline_spec(
                                device,
                                gpu_material.variant.clone(),
                                gpu_material.double_sided,
                                gpu_material.blend,
                            ),
                        )
                    })
                    .transpose()
                    .unwrap_or_else(|e| {
                        log::error!("ForwardPlusLane: pipeline resolve failed: {:?}", e);
                        None
                    })
                    .unwrap_or(fallback_pipeline);

                // Compute Matrices
                let model_mat = extracted_mesh.transform.to_matrix();
                let normal_mat = model_mat.inverse().unwrap_or_default().transpose();

                let model_uniforms = khora_core::renderer::api::gpu_scene::ModelUniforms {
                    model_matrix: model_mat.to_cols_array_2d(),
                    normal_matrix: normal_mat.to_cols_array_2d(),
                };

                // Model transform → dynamic ring; material → cached GpuMaterial.
                let (model_bg, model_offset) = if let Some(ref mut ring) = resources.model_ring {
                    let offset = match ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                        Ok(off) => off,
                        Err(_) => continue,
                    };
                    (*ring.current_bind_group(), offset)
                } else {
                    continue;
                };

                let command = khora_core::renderer::api::command::DrawCommand {
                    pipeline: pipeline_id,
                    vertex_buffer: gpu_mesh_handle.vertex_buffer,
                    index_buffer: gpu_mesh_handle.index_buffer,
                    index_count: gpu_mesh_handle.index_count,
                    index_format: gpu_mesh_handle.index_format,
                    model_bind_group: Some(model_bg),
                    model_offset,
                    material_bind_group: Some(gpu_material.bind_group),
                    material_offset: 0,
                };
                if gpu_material.blend {
                    transparent_draws.push((
                        crate::render_lane::camera_distance_sq(&model_mat, view.position),
                        command,
                    ));
                } else {
                    draw_commands.push(command);
                }
            }
        }

        // Batch by pipeline (variant): one `set_pipeline` per variant.
        draw_commands.sort_by_key(|cmd| cmd.pipeline.0);
        // Transparent draws sort farthest-first instead: correct compositing
        // outranks pipeline batching.
        transparent_draws
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // 6. Render Pass
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
            label: Some("ForwardPlus Render Pass"),
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

        // Build the per-frame group-3 (lighting) bind group. It packs
        // the lighting domain into one group per the canonical 4-group
        // render convention: the light list (0), the shadow atlases
        // (1/2/3, from the shared `khora::shadow::bindings` contract),
        // the per-tile culling results (4/5/6) and the shadow
        // view-projections (7). It mixes persistent light buffers with
        // the per-frame shadow atlas views, so it cannot be cached.
        //
        // If no shadow strategy ran this frame, skip — wgpu requires
        // every pipeline-declared group to be bound.
        let Some(shadow_bindings) = shadow_bindings else {
            log::warn!(
                "ForwardPlusLane: ShadowGpuBindings not available (shadow agent inactive?), skipping render"
            );
            return;
        };
        let (
            Some(lighting_layout),
            Some(light_buffer),
            Some(light_index_buffer),
            Some(light_grid_buffer),
            Some(tile_info_buffer),
            Some(shadow_view_projs_buffer),
        ) = (
            resources.lighting_layout,
            resources.light_buffer,
            resources.light_index_buffer,
            resources.light_grid_buffer,
            resources.tile_info_buffer,
            resources.shadow_view_projs_buffer,
        )
        else {
            log::warn!("ForwardPlusLane: lighting GPU resources not initialized, skipping render");
            return;
        };

        use khora_core::renderer::api::command::{BindGroupDescriptor, BindGroupEntry};
        let mut lighting_entries: Vec<BindGroupEntry> = Vec::with_capacity(8);
        lighting_entries.push(BindGroupEntry::buffer(g3::LIGHTS, light_buffer, 0, None));
        // Bindings 1/2/3 — shadow atlas 2D + sampler + cube atlas.
        khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries(
            &shadow_bindings,
            &mut lighting_entries,
        );
        lighting_entries.push(BindGroupEntry::buffer(
            g3::LIGHT_INDICES,
            light_index_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::LIGHT_GRID,
            light_grid_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::TILE_INFO,
            tile_info_buffer,
            0,
            None,
        ));
        lighting_entries.push(BindGroupEntry::buffer(
            g3::SHADOW_VIEW_PROJS,
            shadow_view_projs_buffer,
            0,
            None,
        ));
        // Bindings 8..12 — image-based lighting (Forward+ packs its culling
        // buffers at 4..8, so IBL follows at 8).
        let Some(ibl) = ibl_bindings else {
            log::warn!("ForwardPlusLane: IBL bindings not available, skipping render");
            return;
        };
        khora_core::renderer::api::ibl::fill_ibl_bind_group_entries(&ibl, 8, &mut lighting_entries);
        let lighting_bg = match device.create_bind_group(&BindGroupDescriptor {
            label: Some("forward_plus_lighting_bind_group"),
            layout: lighting_layout,
            entries: &lighting_entries,
        }) {
            Ok(bg) => bg,
            Err(e) => {
                log::error!(
                    "ForwardPlusLane: failed to create lighting bind group: {:?}",
                    e
                );
                return;
            }
        };

        {
            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
            render_pass.set_bind_group(0, &camera_bind_group, &[]);
            render_pass.set_bind_group(3, &lighting_bg, &[]);
            crate::render_lane::record_draws(render_pass.as_mut(), draw_commands.iter());
        }

        // The blended half goes into its own command buffer, so the engine can
        // fold the skybox pass between the two. Keeping it here would have the
        // sky repaint the glass: a blended surface writes no depth, and the sky
        // paints exactly the pixels left at the far plane.
        crate::render_lane::record_transparent_pass(
            transparent_encoder,
            &transparent_draws,
            render_ctx,
            &camera_bind_group,
            &lighting_bg,
        );

        let _ = device.destroy_bind_group(lighting_bg);
    }
}
