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

//! GPU resources of the lane and the pipeline spec they are built from.

use super::StandardPbrLane;
use khora_core::renderer::api::pipeline::{LayoutKey, LayoutSpec, PipelineSpec, ShaderVariantKey};
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer;
use khora_core::renderer::api::{
    pipeline::enums::PrimitiveTopology,
    scene::{LightingUniforms, ModelUniforms},
};

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// The declarative pipeline spec for StandardPbr under a given material
/// variant — built each call, deduped by the `PipelineSystem`. Layouts are
/// the canonical 4-group budget; the backend owns + caches them per variant
/// (shared across lit lanes). The group-2 (Material) layout resolves to the
/// variant's texture-binding set.
pub(super) fn pipeline_spec(
    device: &dyn khora_core::renderer::GraphicsDevice,
    variant: ShaderVariantKey,
    double_sided: bool,
    blend: bool,
) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        CompareFunction, CullMode, VertexFormat, VertexStepMode,
    };
    use khora_core::renderer::api::pipeline::state::{
        BlendStateDescriptor, ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
        PrimitiveStateDescriptor, VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    };
    use khora_core::renderer::api::util::{SampleCount, TextureFormat};
    use std::borrow::Cow;

    PipelineSpec {
        label: "StandardPbr Pipeline",
        shader: "khora::pipelines::standard_pbr",
        variant,
        bind_group_layouts: vec![
            LayoutSpec::Named(LayoutKey::Camera),
            LayoutSpec::Named(LayoutKey::Model),
            LayoutSpec::Named(LayoutKey::Material),
            LayoutSpec::Named(LayoutKey::Lighting),
        ],
        vertex_buffers: vec![VertexBufferLayoutDescriptor {
            array_stride: 32,
            step_mode: VertexStepMode::Vertex,
            attributes: Cow::Owned(vec![
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
                VertexAttributeDescriptor {
                    format: VertexFormat::Float32x2,
                    offset: 24,
                    shader_location: 2,
                },
            ]),
        }],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            // Single-sided materials cull back faces; double-sided disable
            // culling. The cull mode is part of the pipeline cache key.
            cull_mode: if double_sided {
                None
            } else {
                Some(CullMode::Back)
            },
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            // Transparent surfaces still depth-*test* against the opaque scene
            // but must not depth-*write*: writing would let a nearer
            // transparent fragment reject a farther one that should still show
            // through it.
            depth_write_enabled: !blend,
            depth_compare: CompareFunction::Less,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState::default(),
        }),
        color_targets: vec![ColorTargetStateDescriptor {
            format: device
                .get_surface_format()
                .unwrap_or(TextureFormat::Rgba8UnormSrgb),
            blend: blend.then(BlendStateDescriptor::alpha_blending),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

pub(super) fn init_gpu_resources(
    lane: &StandardPbrLane,
    device: &dyn khora_core::renderer::GraphicsDevice,
    pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
) -> Result<(), khora_core::renderer::error::RenderError> {
    use khora_core::renderer::api::resource::CameraUniformData;

    let variant = ShaderVariantKey::empty();
    // Canonical layouts come from the backend's LayoutCache (deduped across
    // lit lanes + shared with the material projection's group-2 bind group).
    let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
    let model_layout = pipeline_system.layout(device, LayoutKey::Model, &variant)?;
    let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
    let light_layout = pipeline_system.layout(device, LayoutKey::Lighting, &variant)?;
    let lighting_buffer_layout =
        pipeline_system.layout(device, LayoutKey::LightingBuffer, &variant)?;

    // Warm the empty-variant pipeline (untextured materials). Textured
    // variants are compiled lazily in the render path on first use, keyed by
    // `GpuMaterial::variant`.
    let pipeline_id = pipeline_system.pipeline(
        device,
        &pipeline_spec(device, ShaderVariantKey::empty(), false, false),
    )?;

    let _ = lane.camera_layout.set(camera_layout);
    let _ = lane.model_layout.set(model_layout);
    let _ = lane.material_layout.set(material_layout);
    let _ = lane.light_layout.set(light_layout);
    let _ = lane.pipeline.set(pipeline_id);

    // Ring buffers (per-lane GPU buffers) built against the shared layouts.
    let camera_ring = UniformRingBuffer::new(
        device,
        camera_layout,
        0,
        std::mem::size_of::<CameraUniformData>() as u64,
        "StandardPbr Camera Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let _ = lane.lighting_buffer_layout.set(lighting_buffer_layout);

    let lighting_ring = UniformRingBuffer::new(
        device,
        lighting_buffer_layout,
        0,
        std::mem::size_of::<LightingUniforms>() as u64,
        "StandardPbr Lighting Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    let model_ring = DynamicUniformRingBuffer::new(
        device,
        model_layout,
        0,
        std::mem::size_of::<ModelUniforms>() as u32,
        khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
        khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
        "StandardPbr Model Ring",
    )
    .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

    use khora_core::lane::lock::mutex_lock_render;
    *mutex_lock_render(&lane.camera_ring, "StandardPbr init.camera_ring")? = Some(camera_ring);
    *mutex_lock_render(&lane.lighting_ring, "StandardPbr init.lighting_ring")? =
        Some(lighting_ring);
    *mutex_lock_render(&lane.model_ring, "StandardPbr init.model_ring")? = Some(model_ring);
    Ok(())
}
