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

//! GPU resources of the lane: creation, release, and the layouts and
//! pipeline specs they are built from.

use crate::render_lane::culling::mesh_cull_mode;
use crate::render_lane::neutral::NeutralBindings;
use crate::render_lane::util::dynamic_uniform_ring_buffer::{
    DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
};
use crate::render_lane::util::kept_bind_groups::KeptBindGroups;
use crate::render_lane::util::target_state::{
    scene_color_targets, scene_depth_state, scene_multisample,
};
use crate::render_lane::util::variant_pipelines::{Surface, VariantPipelines};
use crate::render_lane::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::command::BindGroupLayoutId;
use khora_core::renderer::api::frame::MAX_FRAMES_IN_FLIGHT;
use khora_core::renderer::api::gpu_scene::{Mesh, ModelUniforms};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::{
    LayoutKey, LayoutSpec, PipelineSpec, PrimitiveStateDescriptor,
};
use khora_core::renderer::api::resource::{BufferId, CameraUniformData};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::api::shadow::ShadowGpuBindings;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::light::LightingUniforms;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

/// The label the lane's resources are named by.
pub(super) const LABEL: &str = "LitForward";

/// The first group-3 binding of the image-based lighting, after the lighting
/// uniforms and the shadow atlases.
pub(super) const IBL_BASE: u32 = 4;

/// What a group-3 bind group binds: the lighting buffer of the ring slot,
/// the shadow atlases and the environment.
pub(super) type LightingKey = (BufferId, ShadowGpuBindings, IblGpuBindings);

/// The pipeline spec of the forward lane for `surface` drawn into a target
/// of `key`: the canonical four layouts, the group-2 one resolving to the
/// material variant's textures.
pub(super) fn pipeline_spec(surface: &Surface, key: TargetKey) -> PipelineSpec {
    PipelineSpec {
        label: "LitForward Pipeline",
        shader: "khora::pipelines::lit_forward",
        variant: surface.variant.clone(),
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Named(LayoutKey::Model),
            LayoutSpec::Named(LayoutKey::Material),
            LayoutSpec::Named(LayoutKey::Lighting),
        ],
        vertex_buffers: vec![Mesh::vertex_buffer_layout()],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            cull_mode: mesh_cull_mode(surface.double_sided, surface.mirrored),
            ..Default::default()
        },
        depth_stencil: scene_depth_state(key, surface.blend),
        color_targets: scene_color_targets(key, surface.blend),
        multisample: scene_multisample(key),
    }
}

/// What the forward lane keeps on the device.
pub(super) struct LitGpu {
    pub(super) camera_ring: UniformRingBuffer,
    pub(super) lighting_ring: UniformRingBuffer,
    pub(super) model_ring: DynamicUniformRingBuffer,
    /// The group-3 layout the lighting bind groups are made with.
    pub(super) light_layout: BindGroupLayoutId,
    /// What group 3 binds when the frame has no shadows or no bake.
    pub(super) neutral: NeutralBindings,
    /// The pipelines asked for on the current target.
    pub(super) pipelines: VariantPipelines,
    /// One lighting bind group per buffer of the lighting ring.
    pub(super) lighting_groups: KeptBindGroups<LightingKey>,
}

impl LitGpu {
    /// Builds the rings and the neutral bindings; on a failure, gives back
    /// what was already built.
    pub(super) fn build(
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
    ) -> Result<Self, RenderError> {
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipelines.layout(device, LayoutKey::Camera, &variant)?;
        let model_layout = pipelines.layout(device, LayoutKey::Model, &variant)?;
        let light_layout = pipelines.layout(device, LayoutKey::Lighting, &variant)?;
        let lighting_buffer_layout =
            pipelines.layout(device, LayoutKey::LightingBuffer, &variant)?;

        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "LitForward Camera Ring",
        )
        .map_err(RenderError::ResourceError)?;
        let lighting_ring = match UniformRingBuffer::new(
            device,
            lighting_buffer_layout,
            0,
            std::mem::size_of::<LightingUniforms>() as u64,
            "LitForward Lighting Ring",
        ) {
            Ok(ring) => ring,
            Err(error) => {
                camera_ring.destroy(device);
                return Err(RenderError::ResourceError(error));
            }
        };
        let model_ring = match DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            DEFAULT_MAX_ELEMENTS,
            MIN_UNIFORM_ALIGNMENT,
            "LitForward Model Ring",
        ) {
            Ok(ring) => ring,
            Err(error) => {
                camera_ring.destroy(device);
                lighting_ring.destroy(device);
                return Err(RenderError::ResourceError(error));
            }
        };
        let neutral = match NeutralBindings::build(device, LABEL) {
            Ok(neutral) => neutral,
            Err(error) => {
                camera_ring.destroy(device);
                lighting_ring.destroy(device);
                model_ring.destroy(device);
                return Err(error);
            }
        };
        Ok(Self {
            camera_ring,
            lighting_ring,
            model_ring,
            light_layout,
            neutral,
            pipelines: VariantPipelines::default(),
            lighting_groups: KeptBindGroups::new(MAX_FRAMES_IN_FLIGHT),
        })
    }

    /// Draws into a target of `key` from now on, warming the plain
    /// surface's pipeline for it.
    pub(super) fn retarget(
        &mut self,
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
        key: TargetKey,
    ) -> Result<(), RenderError> {
        self.pipelines.retarget(key);
        self.pipelines
            .get(&Surface::plain(), device, Some(pipelines), pipeline_spec)?;
        Ok(())
    }

    /// Gives back the rings, the neutral bindings and the kept bind groups.
    /// The layouts and the pipelines belong to the pipeline system.
    pub(super) fn destroy(mut self, device: &dyn GraphicsDevice) {
        self.lighting_groups.destroy(device);
        self.camera_ring.destroy(device);
        self.lighting_ring.destroy(device);
        self.model_ring.destroy(device);
        self.neutral.destroy(device);
        self.pipelines.clear();
    }
}
