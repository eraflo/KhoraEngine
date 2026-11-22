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

use super::gpu::{pipeline_spec, LitGpu, IBL_BASE};
use crate::render::family::{LitInputs, RenderOutputs};
use crate::render::recording::{
    record_clear, record_draws, record_transparent_pass, scene_color, scene_depth,
};
use crate::render::util::lit_draws::{collect_lit_draws, LitDraws, LitPipelines};
use crate::render::util::variant_pipelines::Surface;
use khora_core::math::{LinearRgba, Mat4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, LoadOp, RenderPassDescriptor,
};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::ibl::fill_ibl_bind_group_entries;
use khora_core::renderer::api::resource::CameraUniformData;
use khora_core::renderer::api::shadow::bindings::{binding, fill_shadow_bind_group_entries};
use khora_core::renderer::api::shadow::{ShadowEntries, ShadowEntry};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::light::{
    DirectionalLightUniform, LightType, LightingUniforms, PointLightUniform, SpotLightUniform,
    MAX_DIRECTIONAL_LIGHTS, MAX_POINT_LIGHTS, MAX_SPOT_LIGHTS,
};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;

impl LitGpu {
    /// Records the frame: the opaque meshes grouped by pipeline, then the
    /// blended ones farthest first.
    ///
    /// Without a camera the scene pass only clears.
    pub(super) fn record(
        &mut self,
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
                "LitForward Clear-Only Pass",
            )?;
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

        let entries = inputs
            .shadows
            .map(|frame| &frame.entries)
            .cloned()
            .unwrap_or_default();
        let lighting = lighting_uniforms(inputs.render_world, &entries);
        self.lighting_ring.advance();
        self.lighting_ring
            .write(device, bytemuck::bytes_of(&lighting))
            .map_err(RenderError::ResourceError)?;
        let lighting_buffer = self.lighting_ring.current_buffer();

        // The shadows family's atlases and the bake's environment, or the
        // lane's own neutral ones when the frame has none.
        let shadows = inputs
            .shadows
            .and_then(|frame| frame.bindings)
            .unwrap_or(self.neutral.shadows);
        let ibl = inputs.ibl.bindings().unwrap_or(self.neutral.ibl);
        let layout = self.light_layout;
        let lighting_bind_group = self
            .lighting_groups
            .get((lighting_buffer, shadows, ibl), device, || {
                let mut entries = vec![BindGroupEntry::buffer(
                    binding::LIGHTING_UNIFORMS,
                    lighting_buffer,
                    0,
                    None,
                )];
                fill_shadow_bind_group_entries(&shadows, &mut entries);
                fill_ibl_bind_group_entries(&ibl, IBL_BASE, &mut entries);
                device.create_bind_group(&BindGroupDescriptor {
                    label: Some("LitForward Lighting Bind Group"),
                    layout,
                    entries: &entries,
                })
            })
            .map_err(RenderError::ResourceError)?;

        self.model_ring.advance();
        let lent = out.scene.pipelines();
        let plain = self
            .pipelines
            .get(&Surface::plain(), device, lent, pipeline_spec)?;
        let LitDraws { opaque, blended } = collect_lit_draws(
            inputs.render_world,
            gpu_meshes,
            view.position,
            device,
            LitPipelines {
                kept: &mut self.pipelines,
                lent,
                spec: pipeline_spec,
                plain,
                label: super::gpu::LABEL,
            },
            &mut self.model_ring,
        )?;

        {
            let color = [scene_color(&target, LoadOp::Clear(clear))];
            let descriptor = RenderPassDescriptor {
                label: Some("Lit Forward Pass"),
                color_attachments: &color,
                depth_stencil_attachment: Some(scene_depth(&target, LoadOp::Clear(1.0))),
            };
            let encoder = out.scene.encoder();
            let mut pass = encoder.begin_render_pass(&descriptor)?;
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
        )?;
        Ok(())
    }
}

/// Returns the frame's lighting uniforms: each light up to the pass's caps, with
/// its shadow entry when the shadows family drew one.
fn lighting_uniforms(world: &RenderWorld, shadows: &ShadowEntries) -> LightingUniforms {
    let mut uniforms = LightingUniforms {
        directional_lights: [DirectionalLightUniform {
            direction: [0.0; 4],
            color: LinearRgba::BLACK,
            shadow_view_proj: [[0.0; 4]; 4],
            shadow_params: [0.0; 4],
        }; MAX_DIRECTIONAL_LIGHTS],
        point_lights: [PointLightUniform {
            position: [0.0; 4],
            color: LinearRgba::BLACK,
            shadow_params: [0.0; 4],
        }; MAX_POINT_LIGHTS],
        spot_lights: [SpotLightUniform {
            position: [0.0; 4],
            direction: [0.0; 4],
            color: LinearRgba::BLACK,
            params: [0.0; 4],
            shadow_view_proj: [[0.0; 4]; 4],
            shadow_params: [0.0; 4],
        }; MAX_SPOT_LIGHTS],
        num_directional_lights: 0,
        num_point_lights: 0,
        num_spot_lights: 0,
        _padding: 0,
    };

    for (index, light) in world.lights.iter().enumerate() {
        let shadow = shadows.get(index);
        // The 2D atlas layer and matrix of a directional or spot light; -1
        // when it casts none.
        let (atlas_view_proj, atlas_index) = match shadow {
            Some(ShadowEntry::Atlas2D {
                view_proj,
                atlas_index,
            }) => (*view_proj, *atlas_index as f32),
            _ => (Mat4::IDENTITY, -1.0),
        };
        // The cube layer and far plane of a point light.
        let (cube_layer, cube_far_plane) = match shadow {
            Some(ShadowEntry::Cube {
                cube_array_index,
                far_plane,
                ..
            }) => (*cube_array_index as f32, *far_plane),
            _ => (-1.0, 0.0),
        };

        match light.light_type {
            LightType::Directional(ref d) => {
                let at = uniforms.num_directional_lights as usize;
                if at < MAX_DIRECTIONAL_LIGHTS {
                    uniforms.directional_lights[at] = DirectionalLightUniform {
                        direction: [light.direction.x, light.direction.y, light.direction.z, 0.0],
                        color: d.color.with_alpha(d.intensity),
                        shadow_view_proj: atlas_view_proj.to_cols_array_2d(),
                        shadow_params: [atlas_index, d.shadow_bias, d.shadow_normal_bias, 0.0],
                    };
                    uniforms.num_directional_lights += 1;
                }
            }
            LightType::Point(ref p) => {
                let at = uniforms.num_point_lights as usize;
                if at < MAX_POINT_LIGHTS {
                    // `shadow_params`: the cube layer (-1 for none), the
                    // depth and normal biases, and the far plane the shadow
                    // pass projected with.
                    uniforms.point_lights[at] = PointLightUniform {
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
                    uniforms.num_point_lights += 1;
                }
            }
            LightType::Spot(ref s) => {
                let at = uniforms.num_spot_lights as usize;
                if at < MAX_SPOT_LIGHTS {
                    uniforms.spot_lights[at] = SpotLightUniform {
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
                        shadow_view_proj: atlas_view_proj.to_cols_array_2d(),
                        shadow_params: [atlas_index, s.shadow_bias, s.shadow_normal_bias, 0.0],
                    };
                    uniforms.num_spot_lights += 1;
                }
            }
        }
    }
    uniforms
}
