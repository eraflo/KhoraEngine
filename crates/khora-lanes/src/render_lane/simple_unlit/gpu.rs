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

use super::SimpleUnlitLane;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::{LayoutKey, LayoutSpec, PipelineSpec};
use khora_core::renderer::api::shader::ShaderVariantKey;

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

/// The declarative pipeline spec for SimpleUnlit — built each call, deduped by
/// the `PipelineSystem`. Camera is the canonical layout; model + material are
/// bespoke inline layouts shared with the lane's ring buffers.
fn pipeline_spec(device: &dyn khora_core::renderer::GraphicsDevice) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::enums::{
        CompareFunction, VertexFormat, VertexStepMode,
    };
    use khora_core::renderer::api::pipeline::state::{
        ColorWrites, DepthBiasState, StencilFaceState,
    };
    use khora_core::renderer::api::pipeline::{
        ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
        PrimitiveStateDescriptor, VertexAttributeDescriptor, VertexBufferLayoutDescriptor,
    };
    use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
    use std::borrow::Cow;

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
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
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
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

impl SimpleUnlitLane {
    pub(super) fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use crate::render_lane::util::UniformRingBuffer;
        use khora_core::renderer::api::gpu_scene::ModelUniforms;
        use khora_core::renderer::api::resource::CameraUniformData;

        log::info!("SimpleUnlitLane: Initializing GPU resources...");

        // The camera layout is the canonical engine layout (shared with the
        // lit lanes); the model + material layouts are bespoke to this unlit
        // strategy (single dynamic-offset uniforms, no PBR textures), resolved
        // as inline layouts so the ring buffers + pipeline share one id.
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
        let model_layout = pipeline_system.inline_layout(
            device,
            UNLIT_MODEL_LAYOUT_LABEL,
            &unlit_model_layout_entries(),
        )?;
        let material_layout = pipeline_system.inline_layout(
            device,
            UNLIT_MATERIAL_LAYOUT_LABEL,
            &unlit_material_layout_entries(),
        )?;

        // Pipeline (compiled + cached by the backend).
        let pipeline_id = pipeline_system.pipeline(device, &pipeline_spec(device))?;

        // Init-once writes — `set` returns Err if already initialized,
        // which we ignore: a second `on_initialize` is a logic bug
        // upstream, not a runtime failure.
        let _ = self.camera_layout.set(camera_layout);
        let _ = self.model_layout.set(model_layout);
        let _ = self.material_layout.set(material_layout);
        let _ = self.pipeline.set(pipeline_id);

        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "Camera Uniform Ring Runlit",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *khora_core::lane::lock::mutex_lock_render(
            &self.camera_ring,
            "SimpleUnlit init.camera_ring",
        )? = Some(camera_ring);

        let model_ring = crate::render_lane::util::DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            crate::render_lane::util::dynamic_uniform_ring_buffer::DEFAULT_MAX_ELEMENTS,
            crate::render_lane::util::dynamic_uniform_ring_buffer::MIN_UNIFORM_ALIGNMENT,
            "Model Dynamic Ring Runlit",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *khora_core::lane::lock::mutex_lock_render(
            &self.model_ring,
            "SimpleUnlit init.model_ring",
        )? = Some(model_ring);

        let material_ring = crate::render_lane::util::DynamicUniformRingBuffer::new(
            device,
            material_layout,
            0, // Binding size
            std::mem::size_of::<khora_core::renderer::api::material::MaterialUniforms>() as u32,
            crate::render_lane::util::dynamic_uniform_ring_buffer::DEFAULT_MAX_ELEMENTS,
            crate::render_lane::util::dynamic_uniform_ring_buffer::MIN_UNIFORM_ALIGNMENT,
            "Material Dynamic Ring Runlit",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *khora_core::lane::lock::mutex_lock_render(
            &self.material_ring,
            "SimpleUnlit init.material_ring",
        )? = Some(material_ring);

        Ok(())
    }

    pub(super) fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        // Ring buffers own their GPU buffers + bind groups; the pipeline and
        // bind-group layouts are owned + cached by the `PipelineSystem`
        // backend, so the lane must not destroy them here.
        if let Some(ring) = self.camera_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.model_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.material_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
    }
}
