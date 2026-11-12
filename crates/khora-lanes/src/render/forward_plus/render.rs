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

//! Recording the lane's culling pass and draw calls for one frame.

use super::g3;
use super::gpu::{render_pipeline_spec, LIGHT_BUFFER_CAPACITY};
use super::lane_gpu::PlusGpu;
use super::tile_grid::TileGrid;
use crate::render::camera_distance_sq;
use crate::render::family::{LitInputs, RenderOutputs};
use crate::render::recording::{
    record_clear, record_draws, record_transparent_pass, scene_color, scene_depth,
};
use crate::render::util::variant_pipelines::Surface;
use khora_core::math::Mat4;
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, ComputePassDescriptor, DrawCommand, LoadOp,
    RenderPassDescriptor,
};
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::ibl::fill_ibl_bind_group_entries;
use khora_core::renderer::api::resource::CameraUniformData;
use khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries;
use khora_core::renderer::api::shadow::ShadowEntry;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::light::CullingUniformsData;
use khora_core::renderer::{ForwardPlusTileConfig, GpuLight};
use khora_data::assets::Assets;

/// The first group-3 binding of the image-based lighting, after the light
/// buffers, the shadow atlases and the tile lists.
const IBL_BASE: u32 = 8;

/// A buffer the lane builds at its prepare, missing at a record.
fn not_built(what: &str) -> RenderError {
    RenderError::ResourceError(ResourceError::BackendError(format!(
        "Forward+: {what} is not built"
    )))
}

impl PlusGpu {
    /// Records the frame: a clear when the world has no camera; else the
    /// culling of every tile of the target, then the opaque meshes into the
    /// scene pass and the blended ones into the transparent pass.
    pub(super) fn record(
        &mut self,
        config: &ForwardPlusTileConfig,
        inputs: &LitInputs<'_>,
        gpu_meshes: &Assets<GpuMesh>,
        out: &mut RenderOutputs<'_>,
    ) -> Result<(), RenderError> {
        let target = inputs.target;
        let clear = inputs.clear_color.0;
        let device = out.scene.device();
        let Some(view) = inputs.render_world.views.first() else {
            record_clear(
                out.scene.encoder(),
                &target,
                clear,
                "ForwardPlus Clear-Only Pass",
            );
            return Ok(());
        };
        let resources = &mut self.resources;

        let camera = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };
        let camera_ring = resources
            .camera_ring
            .as_mut()
            .ok_or_else(|| not_built("the camera ring"))?;
        camera_ring.advance();
        camera_ring
            .write(device, bytemuck::bytes_of(&camera))
            .map_err(RenderError::ResourceError)?;
        let camera_bind_group = *camera_ring.current_bind_group();

        // The tile grid of the image this frame renders into; the light
        // lists grow with it before anything indexes them.
        let grid = TileGrid::new(config, target.size);
        resources
            .ensure_tile_buffers(device, config, &grid)
            .map_err(RenderError::ResourceError)?;
        let tile_info = grid.tile_info(config.max_lights_per_tile);
        let tile_info_buffer = resources
            .tile_info_buffer
            .ok_or_else(|| not_built("the tile info"))?;
        device
            .write_buffer(tile_info_buffer, 0, bytemuck::cast_slice(&tile_info))
            .map_err(RenderError::ResourceError)?;

        // Each light with its shadow entry: the atlas layer (or cube) it
        // samples, and its view-projection in a parallel buffer. The buffer
        // holds a fixed number; the ones past it are not shaded, and the
        // quote says so.
        let shadow_entries = inputs
            .shadows
            .map(|frame| &frame.entries)
            .cloned()
            .unwrap_or_default();
        let world_lights = &inputs.render_world.lights;
        if world_lights.len() > LIGHT_BUFFER_CAPACITY {
            log::debug!(
                "Forward+: {} lights, the light buffer holds {LIGHT_BUFFER_CAPACITY}",
                world_lights.len()
            );
        }
        let lights: Vec<GpuLight> = world_lights
            .iter()
            .take(LIGHT_BUFFER_CAPACITY)
            .enumerate()
            .map(|(index, light)| {
                let mut gpu_light = GpuLight::from_parts(
                    [light.position.x, light.position.y, light.position.z],
                    [light.direction.x, light.direction.y, light.direction.z],
                    &light.light_type,
                );
                match shadow_entries.get(index) {
                    Some(ShadowEntry::Atlas2D { atlas_index, .. }) => {
                        gpu_light.shadow_map_index = *atlas_index;
                    }
                    Some(ShadowEntry::Cube {
                        cube_array_index,
                        far_plane,
                        ..
                    }) => {
                        gpu_light.shadow_map_index = *cube_array_index;
                        gpu_light.shadow_far_plane = *far_plane;
                    }
                    None => {}
                }
                gpu_light
            })
            .collect();
        let shadow_view_projs: Vec<[[f32; 4]; 4]> = (0..lights.len())
            .map(|index| match shadow_entries.get(index) {
                Some(ShadowEntry::Atlas2D { view_proj, .. }) => view_proj.to_cols_array_2d(),
                _ => Mat4::IDENTITY.to_cols_array_2d(),
            })
            .collect();
        let light_buffer = resources
            .light_buffer
            .ok_or_else(|| not_built("the light buffer"))?;
        let shadow_view_projs_buffer = resources
            .shadow_view_projs_buffer
            .ok_or_else(|| not_built("the shadow matrices"))?;
        if !lights.is_empty() {
            device
                .write_buffer(light_buffer, 0, bytemuck::cast_slice(&lights))
                .map_err(RenderError::ResourceError)?;
            device
                .write_buffer(
                    shadow_view_projs_buffer,
                    0,
                    bytemuck::cast_slice(&shadow_view_projs),
                )
                .map_err(RenderError::ResourceError)?;
        }
        let culling = CullingUniformsData {
            view_projection: view.view_proj.to_cols_array_2d(),
            screen_dimensions: grid.screen_dimensions(),
            tile_count: [grid.tiles_x, grid.tiles_y],
            num_lights: lights.len() as u32,
            tile_size: grid.tile_size,
            max_lights_per_tile: config.max_lights_per_tile,
            _padding: 0,
        };
        let culling_buffer = resources
            .culling_uniforms_buffer
            .ok_or_else(|| not_built("the culling uniforms"))?;
        device
            .write_buffer(culling_buffer, 0, bytemuck::bytes_of(&culling))
            .map_err(RenderError::ResourceError)?;

        // One workgroup per tile, before the draws that read the lists.
        if let (Some(pipeline), Some(group), true) = (
            resources.culling_pipeline,
            resources.culling_bind_group,
            grid.total() > 0,
        ) {
            let mut pass = out
                .scene
                .encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("Forward+ Light Culling Pass"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(grid.tiles_x, grid.tiles_y, 1);
        }

        // The group-3 bind group: the light list (0), the shadow atlases
        // (1/2/3), the tile lists (4/5/6), the shadow matrices (7) and the
        // environment (8..12) — the shadows family's and the bake's, or the
        // lane's own neutral ones.
        let shadows = inputs
            .shadows
            .and_then(|frame| frame.bindings)
            .unwrap_or(self.neutral.shadows);
        let ibl = inputs.ibl.bindings().unwrap_or(self.neutral.ibl);
        let layout = resources
            .lighting_layout
            .ok_or_else(|| not_built("the lighting layout"))?;
        let (Some(light_index_buffer), Some(light_grid_buffer)) =
            (resources.light_index_buffer, resources.light_grid_buffer)
        else {
            return Err(not_built("the light lists"));
        };
        let lighting_bind_group = self
            .lighting_group
            .get((light_index_buffer, shadows, ibl), device, || {
                let mut entries = vec![BindGroupEntry::buffer(g3::LIGHTS, light_buffer, 0, None)];
                fill_shadow_bind_group_entries(&shadows, &mut entries);
                entries.extend([
                    BindGroupEntry::buffer(g3::LIGHT_INDICES, light_index_buffer, 0, None),
                    BindGroupEntry::buffer(g3::LIGHT_GRID, light_grid_buffer, 0, None),
                    BindGroupEntry::buffer(g3::TILE_INFO, tile_info_buffer, 0, None),
                    BindGroupEntry::buffer(
                        g3::SHADOW_VIEW_PROJS,
                        shadow_view_projs_buffer,
                        0,
                        None,
                    ),
                ]);
                fill_ibl_bind_group_entries(&ibl, IBL_BASE, &mut entries);
                device.create_bind_group(&BindGroupDescriptor {
                    label: Some("Forward+ Lighting Bind Group"),
                    layout,
                    entries: &entries,
                })
            })
            .map_err(RenderError::ResourceError)?;

        let model_ring = resources
            .model_ring
            .as_mut()
            .ok_or_else(|| not_built("the model ring"))?;
        model_ring.advance();
        let lent = out.scene.pipelines();
        let plain = resources.render_pipeline;
        let mut opaque = Vec::with_capacity(inputs.render_world.meshes.len());
        let mut blended: Vec<(f32, DrawCommand)> = Vec::new();
        for mesh in &inputs.render_world.meshes {
            let Some(gpu_mesh) = gpu_meshes.get(&mesh.cpu_mesh_uuid) else {
                continue;
            };
            // The material projection tags every rendered entity with a
            // `GpuMaterial` before the render flow runs.
            let Some(material) = &mesh.gpu_material else {
                continue;
            };
            let surface = Surface {
                variant: material.variant.clone(),
                double_sided: material.double_sided,
                mirrored: mesh.mirrored,
                blend: material.blend,
            };
            let pipeline = match self
                .pipelines
                .get(&surface, device, lent, render_pipeline_spec)
            {
                Ok(Some(pipeline)) => pipeline,
                Ok(None) => match plain {
                    Some(pipeline) => pipeline,
                    None => continue,
                },
                Err(error) => {
                    log::error!("Forward+: no pipeline for {surface:?}: {error:?}");
                    continue;
                }
            };
            let model = mesh.transform.to_matrix();
            // A degenerate transform has no normal matrix: nothing to draw.
            let Some(inverse) = model.inverse() else {
                continue;
            };
            let model_uniforms = ModelUniforms {
                model_matrix: model.to_cols_array_2d(),
                normal_matrix: inverse.transpose().to_cols_array_2d(),
            };
            let model_offset = model_ring
                .push(device, bytemuck::bytes_of(&model_uniforms))
                .map_err(RenderError::ResourceError)?;
            let draw = DrawCommand {
                pipeline,
                vertex_buffer: gpu_mesh.vertex_buffer,
                index_buffer: gpu_mesh.index_buffer,
                index_format: gpu_mesh.index_format,
                index_count: gpu_mesh.index_count,
                model_bind_group: Some(*model_ring.current_bind_group()),
                model_offset,
                material_bind_group: Some(material.bind_group),
                material_offset: 0,
            };
            if material.blend {
                blended.push((camera_distance_sq(&model, view.position), draw));
            } else {
                opaque.push(draw);
            }
        }
        // One `set_pipeline` per variant across the pass; blending is
        // order-dependent, so the blended draws go farthest first instead.
        opaque.sort_by_key(|draw| draw.pipeline.0);
        blended.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        {
            let color = [scene_color(&target, LoadOp::Clear(clear))];
            let descriptor = RenderPassDescriptor {
                label: Some("ForwardPlus Render Pass"),
                color_attachments: &color,
                depth_stencil_attachment: Some(scene_depth(&target, LoadOp::Clear(1.0))),
            };
            let encoder = out.scene.encoder();
            let mut pass = encoder.begin_render_pass(&descriptor);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            pass.set_bind_group(3, &lighting_bind_group, &[]);
            record_draws(pass.as_mut(), opaque.iter());
        }
        record_transparent_pass(
            out.transparent.encoder(),
            &blended,
            &target,
            &camera_bind_group,
            &lighting_bind_group,
        );
        Ok(())
    }
}
