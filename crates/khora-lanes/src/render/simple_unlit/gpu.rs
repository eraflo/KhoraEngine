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

use std::borrow::Cow;

use crate::render::util::dynamic_uniform_ring_buffer::{
    DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
};
use crate::render::util::target_state::{
    scene_color_targets, scene_depth_state, scene_multisample,
};
use crate::render::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::gpu_scene::{Mesh, ModelUniforms};
use khora_core::renderer::api::material::MaterialUniforms;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::{
    LayoutKey, LayoutSpec, PipelineSpec, PrimitiveStateDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::CameraUniformData;
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

// ─── Free functions (CLAD: declarative pipeline spec + bespoke layouts) ───

/// Stable cache label for the unlit per-draw model layout.
const UNLIT_MODEL_LAYOUT_LABEL: &str = "simple_unlit_model_layout";

/// Stable cache label for the unlit per-draw material layout.
const UNLIT_MATERIAL_LAYOUT_LABEL: &str = "simple_unlit_material_layout";

/// Bespoke group-1 (model) layout: a single dynamic-offset uniform buffer.
fn unlit_model_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::gpu_scene::ModelUniforms;
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<ModelUniforms>() as u64),
        },
    }]
}

/// Bespoke group-2 (material) layout: a single dynamic-offset uniform buffer
/// (unlit has no PBR textures, so it does not use the canonical material
/// layout).
fn unlit_material_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry>
{
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::material::MaterialUniforms;
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: std::num::NonZeroU64::new(
                std::mem::size_of::<MaterialUniforms>() as u64
            ),
        },
    }]
}

/// The pipeline spec of the unlit lane for a target of `key`: the canonical
/// camera layout, and the lane's own model and material layouts shared with
/// its rings.
fn pipeline_spec(key: TargetKey) -> PipelineSpec {
    PipelineSpec {
        label: "SimpleUnlit Pipeline",
        shader: "khora::pipelines::unlit",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Inline {
                label: UNLIT_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(unlit_model_layout_entries()),
            },
            LayoutSpec::Inline {
                label: UNLIT_MATERIAL_LAYOUT_LABEL,
                entries: Cow::Owned(unlit_material_layout_entries()),
            },
        ],
        vertex_buffers: vec![Mesh::vertex_buffer_layout()],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: scene_depth_state(key, false),
        color_targets: scene_color_targets(key, false),
        multisample: scene_multisample(key),
    }
}

/// What the unlit lane keeps on the device: its uniform rings, and the
/// pipeline of the target it draws into.
pub(super) struct UnlitGpu {
    pub(super) camera_ring: UniformRingBuffer,
    pub(super) model_ring: DynamicUniformRingBuffer,
    pub(super) material_ring: DynamicUniformRingBuffer,
    /// The pipeline and the key it was built for; `None` until a target is
    /// known.
    pub(super) pipeline: Option<(TargetKey, RenderPipelineId)>,
}

impl UnlitGpu {
    /// Builds the rings against the layouts the pipeline system holds; a
    /// ring the device refuses gives back the ones already built.
    pub(super) fn build(
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
    ) -> Result<Self, RenderError> {
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipelines.layout(device, LayoutKey::Camera, &variant)?;
        let model_layout = pipelines.inline_layout(
            device,
            UNLIT_MODEL_LAYOUT_LABEL,
            &unlit_model_layout_entries(),
        )?;
        let material_layout = pipelines.inline_layout(
            device,
            UNLIT_MATERIAL_LAYOUT_LABEL,
            &unlit_material_layout_entries(),
        )?;

        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "SimpleUnlit Camera Ring",
        )
        .map_err(RenderError::ResourceError)?;
        let model_ring = match DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            DEFAULT_MAX_ELEMENTS,
            MIN_UNIFORM_ALIGNMENT,
            "SimpleUnlit Model Ring",
        ) {
            Ok(ring) => ring,
            Err(error) => {
                camera_ring.destroy(device);
                return Err(RenderError::ResourceError(error));
            }
        };
        let material_ring = match DynamicUniformRingBuffer::new(
            device,
            material_layout,
            0,
            std::mem::size_of::<MaterialUniforms>() as u32,
            DEFAULT_MAX_ELEMENTS,
            MIN_UNIFORM_ALIGNMENT,
            "SimpleUnlit Material Ring",
        ) {
            Ok(ring) => ring,
            Err(error) => {
                camera_ring.destroy(device);
                model_ring.destroy(device);
                return Err(RenderError::ResourceError(error));
            }
        };
        Ok(Self {
            camera_ring,
            model_ring,
            material_ring,
            pipeline: None,
        })
    }

    /// The pipeline for a target of `key`: the one built while the key
    /// holds, else a new one.
    pub(super) fn retarget(
        &mut self,
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
        key: TargetKey,
    ) -> Result<(), RenderError> {
        if self.pipeline.map(|(built_for, _)| built_for) != Some(key) {
            let pipeline = pipelines.pipeline(device, &pipeline_spec(key))?;
            self.pipeline = Some((key, pipeline));
        }
        Ok(())
    }

    /// Gives the rings back to the device. The layouts and the pipeline
    /// belong to the pipeline system.
    pub(super) fn destroy(self, device: &dyn GraphicsDevice) {
        self.camera_ring.destroy(device);
        self.model_ring.destroy(device);
        self.material_ring.destroy(device);
    }
}
