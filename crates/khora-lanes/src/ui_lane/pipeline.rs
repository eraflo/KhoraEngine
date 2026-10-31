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

//! What the UI lane asks the device and the pipeline system for: its
//! bind-group layouts, its two pipelines for a target's formats — the quads'
//! and the glyphs' — and its sampler.

use std::borrow::Cow;

use khora_core::renderer::api::command::{BindGroupLayoutEntry, BindingType, BufferBindingType};
use khora_core::renderer::api::pipeline::{
    BlendStateDescriptor, ColorTargetStateDescriptor, ColorWrites, MultisampleStateDescriptor,
    PipelineSpec, PrimitiveStateDescriptor, PrimitiveTopology,
};
use khora_core::renderer::api::resource::{TextureFormat, TextureViewDimension};
use khora_core::renderer::api::util::ShaderStageFlags;

/// Stable cache label for the UI global uniform layout (set 0).
pub(super) const UI_GLOBAL_LAYOUT_LABEL: &str = "ui_global_layout";
/// Stable cache label for the UI instance storage layout (set 1).
pub(super) const UI_INSTANCE_LAYOUT_LABEL: &str = "ui_instance_layout";
/// Stable cache label for the UI atlas texture + sampler layout (set 2).
pub(super) const UI_ATLAS_LAYOUT_LABEL: &str = "ui_atlas_layout";
/// Stable cache label for the glyph atlases + sampler layout (set 2 of the
/// glyph pipeline).
pub(super) const UI_GLYPH_ATLAS_LAYOUT_LABEL: &str = "ui_glyph_atlas_layout";

/// The UI's filtering sampler, clamped at the atlas's edges.
pub(super) fn ui_sampler_descriptor(
) -> khora_core::renderer::api::resource::SamplerDescriptor<'static> {
    use khora_core::renderer::api::resource::{
        AddressMode, FilterMode, MipmapFilterMode, SamplerDescriptor,
    };
    SamplerDescriptor {
        label: Some(Cow::Borrowed("ui_sampler")),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Linear,
        lod_min_clamp: 0.0,
        lod_max_clamp: 100.0,
        compare: None,
        anisotropy_clamp: 1,
        border_color: None,
    }
}

/// Set-0 layout: the projection-matrix uniform buffer.
pub(super) fn ui_global_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Set-1 layout: the read-only instance storage buffer.
pub(super) fn ui_instance_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// Set-2 layout: the atlas texture + filtering sampler.
pub(super) fn ui_atlas_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: khora_core::renderer::api::command::TextureSampleType::Float {
                    filterable: true,
                },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(
                khora_core::renderer::api::command::SamplerBindingType::Filtering,
            ),
        },
    ]
}

/// Set-2 layout of the glyph pipeline: the mask atlas, the colour atlas,
/// and the filtering sampler both are read through.
pub(super) fn ui_glyph_atlas_layout_entries() -> Vec<BindGroupLayoutEntry> {
    let texture = |binding| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: khora_core::renderer::api::command::TextureSampleType::Float {
                filterable: true,
            },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
    };
    vec![
        texture(0),
        texture(1),
        BindGroupLayoutEntry {
            binding: 2,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(
                khora_core::renderer::api::command::SamplerBindingType::Filtering,
            ),
        },
    ]
}

/// The declarative pipeline spec for the UI's quads on a target of `color`
/// with `samples` — instanced, no vertex buffer, no depth.
pub(super) fn ui_pipeline_spec(formats: (TextureFormat, u32)) -> PipelineSpec {
    instanced_spec(
        "UI Render Pipeline",
        "khora::pipelines::ui",
        UI_ATLAS_LAYOUT_LABEL,
        ui_atlas_layout_entries(),
        formats,
    )
}

/// The declarative pipeline spec for the UI's glyphs on a target of `color`
/// with `samples` — the quads' shape, over the glyph atlases.
pub(super) fn ui_text_pipeline_spec(formats: (TextureFormat, u32)) -> PipelineSpec {
    instanced_spec(
        "UI Text Pipeline",
        "khora::pipelines::ui_text",
        UI_GLYPH_ATLAS_LAYOUT_LABEL,
        ui_glyph_atlas_layout_entries(),
        formats,
    )
}

/// An instanced pipeline of `shader` over the projection (set 0), an
/// instance storage (set 1) and the textures `textures` lays out (set 2):
/// four vertices a quad from no vertex buffer, no depth, alpha-blended.
fn instanced_spec(
    label: &'static str,
    shader: &'static str,
    textures_label: &'static str,
    textures: Vec<BindGroupLayoutEntry>,
    (color, samples): (TextureFormat, u32),
) -> PipelineSpec {
    use khora_core::renderer::api::pipeline::LayoutSpec;
    use khora_core::renderer::api::shader::ShaderVariantKey;
    PipelineSpec {
        label,
        shader,
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: UI_GLOBAL_LAYOUT_LABEL,
                entries: Cow::Owned(ui_global_layout_entries()),
            },
            LayoutSpec::Inline {
                label: UI_INSTANCE_LAYOUT_LABEL,
                entries: Cow::Owned(ui_instance_layout_entries()),
            },
            LayoutSpec::Inline {
                label: textures_label,
                entries: Cow::Owned(textures),
            },
        ],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        // Four corners per instance, drawn as a strip: two triangles per quad.
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: color,
            // Both shaders return straight (non-premultiplied) alpha: the
            // quads for their rounded corners and borders, the glyphs for
            // their coverage.
            blend: Some(BlendStateDescriptor::alpha_blending()),
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: crate::render_lane::util::sample_count(samples),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}
