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

use super::LitForwardLane;
use khora_core::renderer::api::pipeline::{LayoutKey, LayoutSpec, PipelineSpec, ShaderVariantKey};
use khora_core::renderer::api::util::dynamic_uniform_buffer::DynamicUniformRingBuffer;
use khora_core::renderer::api::util::uniform_ring_buffer::UniformRingBuffer;
use khora_core::renderer::api::{
    pipeline::enums::PrimitiveTopology,
    scene::{LightingUniforms, ModelUniforms},
};

// ─── Free functions (CLAD: declarative pipeline spec) ───

/// The declarative pipeline spec for LitForward under a given material
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
        label: "LitForward Pipeline",
        shader: "khora::pipelines::lit_forward",
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
            // Transparent surfaces depth-test but never depth-write; see
            // `StandardPbrLane::pipeline_spec` for the rationale.
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

impl LitForwardLane {
    pub(super) fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use khora_core::lane::lock::mutex_lock_render;

        log::info!("LitForwardLane: Initializing GPU resources...");

        // Canonical layouts come from the backend's LayoutCache (deduped across
        // lit lanes + shared with the material projection's group-2 bind group).
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
        let model_layout = pipeline_system.layout(device, LayoutKey::Model, &variant)?;
        let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
        let light_layout = pipeline_system.layout(device, LayoutKey::Lighting, &variant)?;
        let lighting_buffer_layout =
            pipeline_system.layout(device, LayoutKey::LightingBuffer, &variant)?;

        // Warm the empty-variant pipeline (untextured materials). Textured
        // variants are compiled lazily in the render path on first use, keyed
        // by `GpuMaterial::variant`.
        let pipeline_id = pipeline_system.pipeline(
            device,
            &pipeline_spec(device, ShaderVariantKey::empty(), false, false),
        )?;

        // Init-once writes — `set` is lock-free; second call returns Err
        // which we ignore (re-init is a logic bug, not a runtime fault).
        let _ = self.camera_layout.set(camera_layout);
        let _ = self.model_layout.set(model_layout);
        let _ = self.material_layout.set(material_layout);
        let _ = self.light_layout.set(light_layout);
        let _ = self.lighting_buffer_layout.set(lighting_buffer_layout);
        let _ = self.pipeline.set(pipeline_id);

        // Persistent ring buffers (per-lane GPU buffers) built against the
        // shared layouts. This eliminates per-frame buffer allocation in the
        // render hot path.
        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>() as u64,
            "Camera Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let lighting_ring = UniformRingBuffer::new(
            device,
            lighting_buffer_layout,
            0,
            std::mem::size_of::<LightingUniforms>() as u64,
            "Lighting Uniform Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_ring = DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
            khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
            "LitForward Model Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        *mutex_lock_render(&self.camera_ring, "LitForward init.camera_ring")? = Some(camera_ring);
        *mutex_lock_render(&self.lighting_ring, "LitForward init.lighting_ring")? =
            Some(lighting_ring);
        *mutex_lock_render(&self.model_ring, "LitForward init.model_ring")? = Some(model_ring);

        log::info!(
            "LitForwardLane: Persistent ring buffers created (camera: {} bytes, lighting: {} bytes, {} slots each)",
            std::mem::size_of::<khora_core::renderer::api::resource::CameraUniformData>(),
            std::mem::size_of::<LightingUniforms>(),
            khora_core::renderer::api::core::MAX_FRAMES_IN_FLIGHT,
        );

        Ok(())
    }

    pub(super) fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        // Destroy ring buffers first (they own buffers + bind groups).
        // `.lock().ok().and_then(|mut g| g.take())` gracefully degrades
        // to a no-op on poisoning rather than panicking.
        //
        // Bind-group layouts and the pipeline are owned + cached by the
        // `PipelineSystem` backend (shared across lit lanes), so the lane
        // must not destroy them here.
        if let Some(ring) = self.camera_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
        if let Some(ring) = self.lighting_ring.lock().ok().and_then(|mut g| g.take()) {
            ring.destroy(device);
        }
    }
}
