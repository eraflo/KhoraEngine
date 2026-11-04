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

use super::g3;
use super::ForwardPlusGpuResources;
use crate::render_lane::culling::mesh_cull_mode;
use crate::render_lane::util::target_state::{
    scene_color_targets, scene_depth_state, scene_multisample,
};
use crate::render_lane::util::variant_pipelines::Surface;
use crate::render_lane::util::{DynamicUniformRingBuffer, UniformRingBuffer};
use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::{
    ComputePipelineSpec, LayoutKey, LayoutSpec, PipelineSpec,
};
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

// ─── Free functions (CLAD: declarative specs + bespoke layouts) ───

/// Stable cache label for the Forward+ per-draw model layout.
const FP_MODEL_LAYOUT_LABEL: &str = "forward_plus_model_layout";

/// Stable cache label for the Forward+ group-3 lighting layout.
const FP_LIGHTING_LAYOUT_LABEL: &str = "forward_plus_lighting_layout";

/// Stable cache label for the Forward+ compute culling layout.
const FP_CULLING_LAYOUT_LABEL: &str = "forward_plus_culling_layout";

/// The most lights one frame uploads: the light buffer and its parallel
/// shadow view-projection buffer hold this many entries each.
pub(super) const LIGHT_BUFFER_CAPACITY: usize = 1024;

/// Bespoke group-1 (model) layout: a single dynamic-offset uniform buffer.
fn fp_model_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
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

/// Bespoke group-3 (lighting) layout — the canonical 4-group render
/// convention's lighting domain. One bind group holds every lighting input:
/// the light list (0), the shadow atlases (1/2/3, shared
/// `khora::shadow::bindings` contract), the per-tile culling results (4/5/6)
/// and the per-light shadow view-projections (7).
fn fp_lighting_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{BindGroupLayoutEntry, BufferBindingType};
    use khora_core::renderer::api::util::ShaderStageFlags;
    let mut entries: Vec<BindGroupLayoutEntry> = vec![BindGroupLayoutEntry::buffer(
        g3::LIGHTS,
        ShaderStageFlags::FRAGMENT,
        BufferBindingType::Storage { read_only: true },
        false,
        None,
    )];
    entries.extend(khora_core::renderer::api::shadow::bindings::shadow_bind_group_layout_entries());
    entries.extend([
        BindGroupLayoutEntry::buffer(
            g3::LIGHT_INDICES,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::LIGHT_GRID,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::TILE_INFO,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Uniform,
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            g3::SHADOW_VIEW_PROJS,
            ShaderStageFlags::FRAGMENT,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
    ]);
    // IBL at 8..12 (irradiance cube, prefiltered cube, BRDF LUT, sampler).
    entries.extend(khora_core::renderer::api::ibl::ibl_bind_group_layout_entries(8));
    entries
}

/// Bespoke compute culling layout (compute pass side).
fn fp_culling_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{BindGroupLayoutEntry, BufferBindingType};
    use khora_core::renderer::api::util::ShaderStageFlags;
    vec![
        BindGroupLayoutEntry::buffer(
            0,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Uniform,
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            1,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: true },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            2,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: false },
            false,
            None,
        ),
        BindGroupLayoutEntry::buffer(
            3,
            ShaderStageFlags::COMPUTE,
            BufferBindingType::Storage { read_only: false },
            false,
            None,
        ),
    ]
}

/// The render pipeline spec of Forward+ for `surface` drawn into a target
/// of `key`: canonical camera and material layouts, its own model and
/// lighting ones; the group-2 layout resolves to the material variant's
/// textures.
pub(super) fn render_pipeline_spec(surface: &Surface, key: TargetKey) -> PipelineSpec {
    use khora_core::renderer::api::gpu_scene::Mesh;
    use khora_core::renderer::api::pipeline::PrimitiveStateDescriptor;
    use std::borrow::Cow;

    PipelineSpec {
        label: "ForwardPlus Pipeline",
        shader: "khora::pipelines::forward_plus",
        variant: surface.variant.clone(),
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Inline {
                label: FP_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(fp_model_layout_entries()),
            },
            LayoutSpec::Named(LayoutKey::Material),
            LayoutSpec::Inline {
                label: FP_LIGHTING_LAYOUT_LABEL,
                entries: Cow::Owned(fp_lighting_layout_entries()),
            },
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

/// The declarative compute pipeline spec for Forward+ light culling.
fn culling_pipeline_spec() -> ComputePipelineSpec {
    use std::borrow::Cow;
    ComputePipelineSpec {
        label: "Forward+ Culling Pipeline",
        shader: "khora::pipelines::light_culling",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![LayoutSpec::Inline {
            label: FP_CULLING_LAYOUT_LABEL,
            entries: Cow::Owned(fp_culling_layout_entries()),
        }],
        entry_point: "cs_main",
    }
}

impl ForwardPlusGpuResources {
    /// Builds the layouts, the culling pipeline, the light buffers and the
    /// rings. The light lists are sized from the target, so the first frame
    /// builds them ([`ensure_tile_buffers`](Self::ensure_tile_buffers)). On a
    /// failure, gives back what was already built.
    pub(super) fn build(
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
    ) -> Result<Self, RenderError> {
        let mut resources = Self::default();
        match resources.build_into(device, pipelines) {
            Ok(()) => Ok(resources),
            Err(error) => {
                resources.destroy(device);
                Err(error)
            }
        }
    }

    fn build_into(
        &mut self,
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
    ) -> Result<(), RenderError> {
        use khora_core::renderer::api::gpu_scene::ModelUniforms;
        use khora_core::renderer::api::resource::{
            BufferDescriptor, BufferUsage, CameraUniformData,
        };
        use std::borrow::Cow;

        // The canonical camera and material layouts are shared with the
        // other lit lanes; the model, lighting and culling ones are the lane's.
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipelines.layout(device, LayoutKey::Camera, &variant)?;
        self.camera_layout = Some(camera_layout);
        self.material_layout = Some(pipelines.layout(device, LayoutKey::Material, &variant)?);
        let model_layout =
            pipelines.inline_layout(device, FP_MODEL_LAYOUT_LABEL, &fp_model_layout_entries())?;
        self.model_layout = Some(model_layout);
        self.lighting_layout = Some(pipelines.inline_layout(
            device,
            FP_LIGHTING_LAYOUT_LABEL,
            &fp_lighting_layout_entries(),
        )?);
        self.culling_layout = Some(pipelines.inline_layout(
            device,
            FP_CULLING_LAYOUT_LABEL,
            &fp_culling_layout_entries(),
        )?);
        self.culling_pipeline = Some(pipelines.compute_pipeline(device, &culling_pipeline_spec())?);

        let buffer = |label: &'static str, size: u64, usage: BufferUsage| {
            device
                .create_buffer(&BufferDescriptor {
                    label: Some(Cow::Borrowed(label)),
                    size,
                    usage,
                    mapped_at_creation: false,
                })
                .map_err(RenderError::ResourceError)
        };
        let storage = BufferUsage::STORAGE | BufferUsage::COPY_DST;
        let uniform = BufferUsage::UNIFORM | BufferUsage::COPY_DST;
        self.light_buffer = Some(buffer(
            "Forward+ Light Buffer",
            (LIGHT_BUFFER_CAPACITY * std::mem::size_of::<khora_core::renderer::GpuLight>()) as u64,
            storage,
        )?);
        self.tile_info_buffer = Some(buffer("Forward+ Tile Info", 256, uniform)?);
        // One matrix per light, indexed as the light buffer is.
        self.shadow_view_projs_buffer = Some(buffer(
            "Forward+ Shadow ViewProj Buffer",
            (LIGHT_BUFFER_CAPACITY * std::mem::size_of::<[[f32; 4]; 4]>()) as u64,
            storage,
        )?);
        self.culling_uniforms_buffer = Some(buffer("Forward+ Culling Uniforms", 256, uniform)?);

        self.camera_ring = Some(
            UniformRingBuffer::new(
                device,
                camera_layout,
                0,
                std::mem::size_of::<CameraUniformData>() as u64,
                "Forward+ Camera Ring",
            )
            .map_err(RenderError::ResourceError)?,
        );
        self.model_ring = Some(
            DynamicUniformRingBuffer::new(
                device,
                model_layout,
                0,
                std::mem::size_of::<ModelUniforms>() as u32,
                crate::render_lane::util::dynamic_uniform_ring_buffer::DEFAULT_MAX_ELEMENTS,
                crate::render_lane::util::dynamic_uniform_ring_buffer::MIN_UNIFORM_ALIGNMENT,
                "Forward+ Model Ring",
            )
            .map_err(RenderError::ResourceError)?,
        );
        Ok(())
    }

    /// Gives every buffer, ring and bind group back to the device. The
    /// layouts and pipelines belong to the pipeline system.
    pub(super) fn destroy(&mut self, device: &dyn GraphicsDevice) {
        if let Some(ring) = self.camera_ring.take() {
            ring.destroy(device);
        }
        if let Some(ring) = self.model_ring.take() {
            ring.destroy(device);
        }
        if let Some(group) = self.culling_bind_group.take() {
            if let Err(error) = device.destroy_bind_group(group) {
                log::warn!("Forward+: the culling bind group could not be destroyed: {error:?}");
            }
        }
        for buffer in [
            self.light_buffer.take(),
            self.light_index_buffer.take(),
            self.light_grid_buffer.take(),
            self.culling_uniforms_buffer.take(),
            self.shadow_view_projs_buffer.take(),
            self.tile_info_buffer.take(),
        ]
        .into_iter()
        .flatten()
        {
            if let Err(error) = device.destroy_buffer(buffer) {
                log::warn!("Forward+: a buffer could not be destroyed: {error:?}");
            }
        }
        self.tile_capacity = 0;
        self.render_pipeline = None;
    }
}
