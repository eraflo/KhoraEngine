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

use super::ForwardPlusLane;
use khora_core::renderer::api::util::{
    dynamic_uniform_buffer::DynamicUniformRingBuffer, uniform_ring_buffer::UniformRingBuffer,
};
use khora_core::renderer::api::{
    pipeline::enums::PrimitiveTopology,
    pipeline::{ComputePipelineSpec, LayoutKey, LayoutSpec, PipelineSpec, ShaderVariantKey},
};

// ─── Free functions (CLAD: declarative specs + bespoke layouts) ───

/// Stable cache label for the Forward+ per-draw model layout.
const FP_MODEL_LAYOUT_LABEL: &str = "forward_plus_model_layout";

/// Stable cache label for the Forward+ group-3 lighting layout.
const FP_LIGHTING_LAYOUT_LABEL: &str = "forward_plus_lighting_layout";

/// Stable cache label for the Forward+ compute culling layout.
const FP_CULLING_LAYOUT_LABEL: &str = "forward_plus_culling_layout";

/// Bespoke group-1 (model) layout: a single dynamic-offset uniform buffer.
fn fp_model_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    use khora_core::renderer::api::command::{
        BindGroupLayoutEntry, BindingType, BufferBindingType,
    };
    use khora_core::renderer::api::scene::ModelUniforms;
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

/// The declarative render pipeline spec for Forward+ under a given material
/// variant — built each call, deduped by the `PipelineSystem`. Canonical
/// Camera/Material; bespoke Model + Lighting. The group-2 (Material) layout
/// resolves to the variant's texture-binding set.
pub(super) fn render_pipeline_spec(
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
        label: "ForwardPlus Pipeline",
        shader: "khora::pipelines::forward_plus",
        variant,
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

impl ForwardPlusLane {
    pub(super) fn on_gpu_init(
        &self,
        device: &dyn khora_core::renderer::GraphicsDevice,
        pipeline_system: &dyn khora_core::renderer::traits::PipelineSystem,
    ) -> Result<(), khora_core::renderer::error::RenderError> {
        use khora_core::renderer::api::{
            command::{BindGroupDescriptor, BindGroupEntry},
            resource::CameraUniformData,
            scene::ModelUniforms,
        };
        use std::borrow::Cow;

        log::info!("ForwardPlusLane: Initializing GPU resources...");

        // Layouts — canonical Camera (group 0) + Material (group 2) come from
        // the backend's cache (shared with the lit lanes); the per-draw Model
        // (group 1), Forward+ lighting (group 3) and the compute culling layout
        // are bespoke inline layouts shared with the lane's pipelines + buffers.
        let variant = ShaderVariantKey::empty();
        let camera_layout = pipeline_system.layout(device, LayoutKey::Camera, &variant)?;
        let material_layout = pipeline_system.layout(device, LayoutKey::Material, &variant)?;
        let model_layout = pipeline_system.inline_layout(
            device,
            FP_MODEL_LAYOUT_LABEL,
            &fp_model_layout_entries(),
        )?;
        let lighting_layout = pipeline_system.inline_layout(
            device,
            FP_LIGHTING_LAYOUT_LABEL,
            &fp_lighting_layout_entries(),
        )?;
        let culling_layout = pipeline_system.inline_layout(
            device,
            FP_CULLING_LAYOUT_LABEL,
            &fp_culling_layout_entries(),
        )?;

        // Pipelines (compiled + cached by the backend). Warm the empty-variant
        // render pipeline (untextured materials); textured variants compile
        // lazily in the render path keyed by `GpuMaterial::variant`.
        let pipeline_id = pipeline_system.pipeline(
            device,
            &render_pipeline_spec(device, ShaderVariantKey::empty(), false, false),
        )?;
        let culling_pipeline =
            pipeline_system.compute_pipeline(device, &culling_pipeline_spec())?;

        // 3. Create Buffers and Rings

        // Light Data Buffer
        let light_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Buffer")),
                size: 64 * 1024,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Light Index List
        let light_index_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Index Buffer")),
                size: 120 * 68 * 256 * 4,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Light Grid
        let light_grid_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Light Grid Buffer")),
                size: 120 * 68 * 2 * 4,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Tile Info Buffer
        let tile_info_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Tile Info")),
                size: 256,
                usage: khora_core::renderer::api::resource::BufferUsage::UNIFORM
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Shadow view-projection matrices (one mat4 per light, indexed
        // identically to `light_buffer`). 64 KB matches the light buffer
        // sizing — ~1024 matrices, well above any realistic scene.
        let shadow_view_projs_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Shadow ViewProj Buffer")),
                size: 64 * 1024,
                usage: khora_core::renderer::api::resource::BufferUsage::STORAGE
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Culling Uniforms
        let culling_uniforms_buffer = device
            .create_buffer(&khora_core::renderer::api::resource::BufferDescriptor {
                label: Some(Cow::Borrowed("Forward+ Culling Uniforms")),
                size: 256,
                usage: khora_core::renderer::api::resource::BufferUsage::UNIFORM
                    | khora_core::renderer::api::resource::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Ring Buffers
        let camera_ring = UniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u64,
            "Forward+ Camera Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        let model_ring = DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            khora_core::renderer::api::util::dynamic_uniform_buffer::DEFAULT_MAX_ELEMENTS,
            khora_core::renderer::api::util::dynamic_uniform_buffer::MIN_UNIFORM_ALIGNMENT,
            "Forward+ Model Ring",
        )
        .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // Materials are no longer per-frame ring uniforms: each material is a
        // cached `GpuMaterial` (uniforms + textures + group-2 bind group)
        // produced by the data-layer projection. The lane just binds it.

        // 4. Bind Groups

        let culling_bg = device
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Forward+ Culling Bind Group"),
                layout: culling_layout,
                entries: &[
                    BindGroupEntry::buffer(0, culling_uniforms_buffer, 0, None),
                    BindGroupEntry::buffer(1, light_buffer, 0, None),
                    BindGroupEntry::buffer(2, light_index_buffer, 0, None),
                    BindGroupEntry::buffer(3, light_grid_buffer, 0, None),
                ],
            })
            .map_err(khora_core::renderer::error::RenderError::ResourceError)?;

        // The group-3 (lighting) bind group is rebuilt every frame in
        // `render` because it combines the persistent light buffers with
        // the per-frame shadow atlas views — it cannot be cached here.

        // 5. Store all resources
        let mut res = self.gpu_resources.lock().map_err(|_| {
            khora_core::renderer::error::RenderError::ResourceError(
                khora_core::renderer::error::ResourceError::BackendError(
                    "ForwardPlusLane gpu_resources mutex poisoned".into(),
                ),
            )
        })?;
        res.light_buffer = Some(light_buffer);
        res.light_index_buffer = Some(light_index_buffer);
        res.light_grid_buffer = Some(light_grid_buffer);
        res.tile_info_buffer = Some(tile_info_buffer);
        res.culling_uniforms_buffer = Some(culling_uniforms_buffer);
        res.shadow_view_projs_buffer = Some(shadow_view_projs_buffer);
        res.camera_layout = Some(camera_layout);
        res.model_layout = Some(model_layout);
        res.material_layout = Some(material_layout);
        res.lighting_layout = Some(lighting_layout);
        res.culling_layout = Some(culling_layout);
        res.camera_ring = Some(camera_ring);
        res.model_ring = Some(model_ring);
        res.culling_bind_group = Some(culling_bg);
        res.culling_pipeline = Some(culling_pipeline);
        res.render_pipeline = Some(pipeline_id);

        Ok(())
    }

    pub(super) fn on_gpu_shutdown(&self, device: &dyn khora_core::renderer::GraphicsDevice) {
        let mut resources = crate::lock_or_log!(
            self.gpu_resources.lock(),
            "ForwardPlusLane::on_gpu_shutdown"
        );

        if let Some(ring) = resources.camera_ring.take() {
            ring.destroy(device);
        }
        if let Some(ring) = resources.model_ring.take() {
            ring.destroy(device);
        }

        if let Some(id) = resources.light_buffer.take() {
            device.destroy_buffer(id).ok();
        }
        if let Some(id) = resources.light_index_buffer.take() {
            device.destroy_buffer(id).ok();
        }
        if let Some(id) = resources.light_grid_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
        if let Some(id) = resources.culling_uniforms_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
        if let Some(id) = resources.shadow_view_projs_buffer.take() {
            let _ = device.destroy_buffer(id);
        }
    }
}
