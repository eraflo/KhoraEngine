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

use super::gpu::pipeline_spec;
use super::LitForwardLane;
use khora_core::renderer::{
    api::{
        command::{
            LoadOp, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
            RenderPassDescriptor, StoreOp,
        },
        core::RenderContext,
        pipeline::RenderPipelineId,
        scene::{
            DirectionalLightUniform, GpuMesh, LightingUniforms, ModelUniforms, PointLightUniform,
            SpotLightUniform, MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
        },
    },
    traits::CommandEncoder,
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::RwLock;

impl LitForwardLane {
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
        use khora_core::renderer::api::command::{
            BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding,
        };

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
                label: Some("LitForward Clear-Only Pass"),
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

        // 2. Prepare Global Uniforms via Persistent Ring Buffers
        //    Instead of creating new GPU buffers every frame, we advance the ring
        //    buffer to the next slot and write the updated data in-place.

        // Camera Uniforms — write to persistent ring buffer
        let camera_uniforms = khora_core::renderer::api::resource::CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };

        let camera_bind_group = {
            let mut lock = crate::lock_or_log!(
                self.camera_ring.lock(),
                "LitForwardLane::render camera_ring"
            );
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("LitForwardLane: camera ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&camera_uniforms)) {
                log::error!("Failed to write camera ring buffer: {:?}", e);
                return;
            }
            *ring.current_bind_group() // Copy the BindGroupId out
        };

        // Lighting Uniforms — build struct CPU-side, then write to persistent ring buffer
        let mut lighting_uniforms = LightingUniforms {
            directional_lights: [DirectionalLightUniform {
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                shadow_view_proj: [[0.0; 4]; 4],
                shadow_params: [0.0; 4],
            }; MAX_DIRECTIONAL_LIGHTS],
            point_lights: [PointLightUniform {
                position: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                shadow_params: [0.0; 4],
            }; MAX_POINT_LIGHTS],
            spot_lights: [SpotLightUniform {
                position: [0.0; 4],
                direction: [0.0; 4],
                color: khora_core::math::LinearRgba::BLACK,
                params: [0.0; 4],
                shadow_view_proj: [[0.0; 4]; 4],
                shadow_params: [0.0; 4],
            }; MAX_SPOT_LIGHTS],
            num_directional_lights: 0,
            num_point_lights: 0,
            num_spot_lights: 0,
            _padding: 0,
        };

        for (light_index, light) in render_world.lights.iter().enumerate() {
            let shadow = shadow_entries.get(light_index);

            // Atlas2D / single-matrix path used by directional + spot.
            let (atlas2d_view_proj, atlas2d_index) = match shadow {
                Some(khora_core::renderer::api::shadow::ShadowEntry::Atlas2D {
                    view_proj,
                    atlas_index,
                }) => (*view_proj, *atlas_index as f32),
                _ => (khora_core::math::Mat4::IDENTITY, -1.0),
            };

            // Cube path used by point lights.
            let (cube_layer, cube_far_plane) = match shadow {
                Some(khora_core::renderer::api::shadow::ShadowEntry::Cube {
                    cube_array_index,
                    far_plane,
                    ..
                }) => (*cube_array_index as f32, *far_plane),
                _ => (-1.0, 0.0),
            };

            match light.light_type {
                khora_core::renderer::light::LightType::Directional(ref d) => {
                    if (lighting_uniforms.num_directional_lights as usize) < MAX_DIRECTIONAL_LIGHTS
                    {
                        let idx = lighting_uniforms.num_directional_lights as usize;
                        lighting_uniforms.directional_lights[idx] = DirectionalLightUniform {
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                0.0,
                            ],
                            color: d.color.with_alpha(d.intensity),
                            shadow_view_proj: atlas2d_view_proj.to_cols_array_2d(),
                            shadow_params: [
                                atlas2d_index,
                                d.shadow_bias,
                                d.shadow_normal_bias,
                                0.0,
                            ],
                        };
                        lighting_uniforms.num_directional_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Point(ref p) => {
                    if (lighting_uniforms.num_point_lights as usize) < MAX_POINT_LIGHTS {
                        let idx = lighting_uniforms.num_point_lights as usize;
                        // `shadow_params` for point lights:
                        //   x = cube layer (or -1 for none)
                        //   y = depth bias
                        //   z = normal bias
                        //   w = far plane (matches the perspective the
                        //       shadow pass used; the WGSL `sample_point_shadow`
                        //       helper recomputes the depth value using it).
                        lighting_uniforms.point_lights[idx] = PointLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                p.range,
                            ],
                            color: p.color.with_alpha(p.intensity),
                            shadow_params: [
                                cube_layer,
                                p.shadow_bias,
                                p.shadow_normal_bias,
                                cube_far_plane,
                            ],
                        };
                        lighting_uniforms.num_point_lights += 1;
                    }
                }
                khora_core::renderer::light::LightType::Spot(ref s) => {
                    if (lighting_uniforms.num_spot_lights as usize) < MAX_SPOT_LIGHTS {
                        let idx = lighting_uniforms.num_spot_lights as usize;
                        lighting_uniforms.spot_lights[idx] = SpotLightUniform {
                            position: [
                                light.position.x,
                                light.position.y,
                                light.position.z,
                                s.range,
                            ],
                            direction: [
                                light.direction.x,
                                light.direction.y,
                                light.direction.z,
                                s.inner_cone_angle.cos(),
                            ],
                            color: s.color.with_alpha(s.intensity),
                            params: [s.outer_cone_angle.cos(), 0.0, 0.0, 0.0],
                            shadow_view_proj: atlas2d_view_proj.to_cols_array_2d(),
                            shadow_params: [
                                atlas2d_index,
                                s.shadow_bias,
                                s.shadow_normal_bias,
                                0.0,
                            ],
                        };
                        lighting_uniforms.num_spot_lights += 1;
                    }
                }
            }
        }

        let (_lighting_bind_group, lighting_ring_buffer_id) = {
            let mut lock = crate::lock_or_log!(
                self.lighting_ring.lock(),
                "LitForwardLane::render lighting_ring"
            );
            let ring = match lock.as_mut() {
                Some(r) => r,
                None => {
                    log::warn!("LitForwardLane: lighting ring buffer not initialized");
                    return;
                }
            };
            ring.advance();
            if let Err(e) = ring.write(device, bytemuck::bytes_of(&lighting_uniforms)) {
                log::error!("Failed to write lighting ring buffer: {:?}", e);
                return;
            }
            (*ring.current_bind_group(), ring.current_buffer())
        };

        // Shadow data (view-proj matrices, atlas indices) is already embedded
        // in the lighting_uniforms via the ExtractedLight fields patched by
        // ShadowPassLane::execute() Phase 2.  The shadow atlas texture and
        // comparison sampler are passed through the RenderContext.

        // Acquire locks
        let gpu_mesh_assets =
            crate::lock_or_log!(gpu_meshes.read(), "LitForwardLane::render gpu_meshes");

        // Fallback pipeline (empty variant) used only if the per-variant
        // resolve fails; lock-free read via OnceLock.
        let fallback_pipeline = self.pipeline.get().copied().unwrap_or(RenderPipelineId(0));

        // Prepare Draw Commands. Opaque draws batch by pipeline; blended draws
        // go to a second batch sorted back-to-front (blending is
        // order-dependent), keyed by squared distance to the camera.
        let mut draw_commands = Vec::with_capacity(render_world.meshes.len());
        let mut transparent_draws: Vec<(f32, khora_core::renderer::api::command::DrawCommand)> =
            Vec::new();

        let mut temp_bind_groups = Vec::new();

        // Model transforms go through the per-frame dynamic ring; materials
        // are consumed from the cached `GpuMaterial` (no per-draw churn).
        let mut model_ring_lock =
            crate::lock_or_log!(self.model_ring.lock(), "LitForwardLane::render model_ring");
        let model_ring = match model_ring_lock.as_mut() {
            Some(r) => r,
            None => {
                log::warn!("LitForwardLane: model ring buffer not initialized");
                return;
            }
        };
        model_ring.advance();

        for extracted_mesh in &render_world.meshes {
            let Some(gpu_mesh_handle) = gpu_mesh_assets.get(&extracted_mesh.cpu_mesh_uuid) else {
                continue;
            };
            // The material projection tags every rendered entity with a
            // `GpuMaterial` before `RenderFlow` runs.
            let Some(gpu_material) = &extracted_mesh.gpu_material else {
                continue;
            };
            // Resolve the pipeline for this material's texture variant. The
            // backend caches per `(shader, variant, format)` so this is a
            // cheap lookup after first compile; the lane never retains a
            // long-term `RenderPipelineId` per variant.
            let pipeline_id = self
                .pipeline_system
                .get()
                .map(|ps| {
                    ps.pipeline(
                        device,
                        &pipeline_spec(
                            device,
                            gpu_material.variant.clone(),
                            gpu_material.double_sided,
                            gpu_material.blend,
                        ),
                    )
                })
                .transpose()
                .unwrap_or_else(|e| {
                    log::error!("LitForwardLane: pipeline resolve failed: {:?}", e);
                    None
                })
                .unwrap_or(fallback_pipeline);
            let model_mat = extracted_mesh.transform.to_matrix();
            let normal_mat = if let Some(inverse) = model_mat.inverse() {
                inverse.transpose()
            } else {
                continue;
            };
            let model_uniforms = ModelUniforms {
                model_matrix: model_mat.to_cols_array_2d(),
                normal_matrix: normal_mat.to_cols_array_2d(),
            };
            let model_offset = match model_ring.push(device, bytemuck::bytes_of(&model_uniforms)) {
                Ok(offset) => offset,
                Err(e) => {
                    log::error!("LitForwardLane: failed to push model uniforms: {:?}", e);
                    continue;
                }
            };
            let model_bg = *model_ring.current_bind_group();

            let command = khora_core::renderer::api::command::DrawCommand {
                pipeline: pipeline_id,
                vertex_buffer: gpu_mesh_handle.vertex_buffer,
                index_buffer: gpu_mesh_handle.index_buffer,
                index_format: gpu_mesh_handle.index_format,
                index_count: gpu_mesh_handle.index_count,
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

        // Batch by pipeline (variant) so each pipeline is set once across the
        // whole pass — avoids per-draw pipeline thrash when materials mix
        // variants. Stable sort keeps submission order within a variant.
        draw_commands.sort_by_key(|cmd| cmd.pipeline.0);
        // Transparent draws sort farthest-first instead: correct compositing
        // outranks pipeline batching.
        transparent_draws
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Render Pass
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
            label: Some("Lit Forward Pass"),
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

        // Build Lighting Bind Group. Shadow-related entries (atlas2D,
        // sampler, atlas_cube) are filled by
        // [`khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries`]
        // — we never reference the individual texture types here.
        let final_lighting_bind_group = if let Some(layout) = self.light_layout.get().copied() {
            let Some(shadow_bindings) = shadow_bindings else {
                log::warn!(
                    "LitForwardLane: ShadowGpuBindings not available (shadow agent inactive?), skipping lit render"
                );
                return;
            };

            // binding 0 — lighting uniform buffer (lit lane's responsibility)
            let mut entries = vec![BindGroupEntry {
                binding: khora_core::renderer::api::shadow::bindings::binding::LIGHTING_UNIFORMS,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: lighting_ring_buffer_id,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }];
            // bindings 1, 2, 3 — shadow lane's responsibility (opaque)
            khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries(
                &shadow_bindings,
                &mut entries,
            );
            // bindings 4..8 — image-based lighting (after shadow)
            let Some(ibl) = ibl_bindings else {
                log::warn!("LitForwardLane: IBL bindings not available, skipping lit render");
                return;
            };
            khora_core::renderer::api::ibl::fill_ibl_bind_group_entries(&ibl, 4, &mut entries);

            match device.create_bind_group(&BindGroupDescriptor {
                label: Some("lit_forward_lighting_bind_group_dynamic"),
                layout,
                entries: &entries,
            }) {
                Ok(bg) => {
                    temp_bind_groups.push(bg);
                    bg
                }
                Err(e) => {
                    log::error!(
                        "LitForwardLane: Failed to create lighting bind group: {:?}",
                        e
                    );
                    return;
                }
            }
        } else {
            log::warn!("LitForwardLane: light layout not initialized");
            return;
        };

        {
            let mut render_pass = encoder.begin_render_pass(&render_pass_desc);
            render_pass.set_bind_group(0, &camera_bind_group, &[]);
            render_pass.set_bind_group(3, &final_lighting_bind_group, &[]);
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
            &final_lighting_bind_group,
        );

        // Only the per-frame group-3 lighting bind group is transient now;
        // model uniforms live in the ring, materials in the GpuMaterial cache.
        for bg in temp_bind_groups {
            let _ = device.destroy_bind_group(bg);
        }
    }
}
