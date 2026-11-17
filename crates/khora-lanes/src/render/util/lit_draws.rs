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

//! The draws a lit lane records a frame: each mesh's model pushed, its
//! pipeline found, the opaque ones apart from the blended; and the key of its
//! lighting bind group.

use khora_core::lane::io::TargetKey;
use khora_core::math::Vec3;
use khora_core::renderer::api::command::DrawCommand;
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::{PipelineSpec, RenderPipelineId};
use khora_core::renderer::api::resource::BufferId;
use khora_core::renderer::api::shadow::ShadowGpuBindings;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;

use super::variant_pipelines::{Surface, VariantPipelines};
use super::DynamicUniformRingBuffer;
use crate::render::camera_distance_sq;

/// A lit lane's draws for one frame, each list in the order it is recorded.
#[derive(Debug, Default)]
pub(crate) struct LitDraws {
    /// The opaque draws, grouped by pipeline.
    pub opaque: Vec<DrawCommand>,
    /// The blended draws with their squared distance to the eye, farthest first.
    pub blended: Vec<(f32, DrawCommand)>,
}

/// Where a lit lane finds the pipeline of each surface it draws.
pub(crate) struct LitPipelines<'a> {
    /// The pipelines the lane kept on its target.
    pub kept: &'a mut VariantPipelines,
    /// The pipeline system lent this frame, to build one not kept.
    pub lent: Option<&'a dyn PipelineSystem>,
    /// The lane's pipeline for a surface, on a target key.
    pub spec: fn(&Surface<'_>, TargetKey) -> PipelineSpec,
    /// The pipeline of a surface none is found for; without it, the mesh is not drawn.
    pub plain: Option<RenderPipelineId>,
    /// The lane's name, in the log.
    pub label: &'static str,
}

/// Returns the frame's draws of every mesh with a GPU mesh and a GPU material.
///
/// Each mesh's model uniforms are pushed into `model_ring`; a mesh whose
/// transform does not invert is not drawn, nor one no pipeline is found for.
///
/// # Errors
///
/// Returns a [`RenderError`] if `model_ring` refuses a push.
pub(crate) fn collect_lit_draws(
    world: &RenderWorld,
    gpu_meshes: &Assets<GpuMesh>,
    eye: Vec3,
    device: &dyn GraphicsDevice,
    pipelines: LitPipelines<'_>,
    model_ring: &mut DynamicUniformRingBuffer,
) -> Result<LitDraws, RenderError> {
    let LitPipelines {
        kept,
        lent,
        spec,
        plain,
        label,
    } = pipelines;
    let mut draws = LitDraws {
        opaque: Vec::with_capacity(world.meshes.len()),
        blended: Vec::new(),
    };
    for mesh in &world.meshes {
        let Some(gpu_mesh) = gpu_meshes.get(&mesh.cpu_mesh_uuid) else {
            continue;
        };
        // The material projection tags every rendered entity with a
        // `GpuMaterial` before the render flow runs.
        let Some(material) = &mesh.gpu_material else {
            continue;
        };
        let surface = Surface {
            variant: &material.variant,
            double_sided: material.double_sided,
            mirrored: mesh.mirrored,
            blend: material.blend,
        };
        let pipeline = match kept.get(&surface, device, lent, spec) {
            Ok(Some(pipeline)) => pipeline,
            Ok(None) => match plain {
                Some(pipeline) => pipeline,
                None => continue,
            },
            Err(error) => {
                log::error!("{label}: no pipeline for {surface:?}: {error:?}");
                continue;
            }
        };
        let model = mesh.transform.to_matrix();
        // A degenerate transform has no normal matrix: nothing to draw.
        let Some(model_uniforms) = ModelUniforms::from_model(model) else {
            continue;
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
            draws.blended.push((camera_distance_sq(&model, eye), draw));
        } else {
            draws.opaque.push(draw);
        }
    }
    // One `set_pipeline` per variant across the pass; blending is
    // order-dependent, so the blended draws go farthest first instead.
    draws.opaque.sort_by_key(|draw| draw.pipeline.0);
    draws
        .blended
        .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(draws)
}

/// What a lit lane's group-3 bind group binds that can change: one buffer of
/// the lane's, the shadow atlases and the environment.
pub(crate) type LightingKey = (BufferId, ShadowGpuBindings, IblGpuBindings);
