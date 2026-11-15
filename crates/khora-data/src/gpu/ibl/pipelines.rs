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

//! Pipeline specs and bind-group layouts of the bake passes.

use std::borrow::Cow;

use super::resources::uniform_entry;
use super::{
    BRDF_SHADER, EQUIRECT_LAYOUT, EQUIRECT_SHADER, FACE_BASIS_LAYOUT, IBL_FORMAT,
    IRRADIANCE_LAYOUT, IRRADIANCE_SHADER, PREFILTER_LAYOUT, PREFILTER_SHADER, SKY_SHADER,
};
use khora_core::renderer::api::command::{
    BindGroupLayoutEntry, BindingType, SamplerBindingType, TextureSampleType, TextureViewDimension,
};
use khora_core::renderer::api::pipeline::state::ColorWrites;
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, LayoutSpec, MultisampleStateDescriptor, PipelineSpec,
    PrimitiveStateDescriptor,
};
use khora_core::renderer::api::resource::SampleCount;
use khora_core::renderer::api::shader::ShaderVariantKey;
use khora_core::renderer::api::util::ShaderStageFlags;

/// Returns the procedural-sky bake pipeline's spec.
pub(super) fn sky_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Sky Bake",
        SKY_SHADER,
        vec![LayoutSpec::Inline {
            label: FACE_BASIS_LAYOUT,
            entries: Cow::Owned(vec![uniform_entry(0)]),
        }],
    )
}

/// Returns the specular prefilter pipeline's spec; it binds like the
/// irradiance convolution.
pub(super) fn prefilter_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Prefilter Bake",
        PREFILTER_SHADER,
        vec![LayoutSpec::Inline {
            label: PREFILTER_LAYOUT,
            entries: Cow::Owned(irradiance_layout_entries()),
        }],
    )
}

/// Returns the split-sum BRDF table pipeline's spec, which binds nothing.
pub(super) fn brdf_pipeline_spec() -> PipelineSpec {
    let mut spec = bake_pipeline_spec("IBL BRDF LUT Bake", BRDF_SHADER, vec![]);
    // The LUT stores (scale, bias) in RG; the shared IBL_FORMAT (Rgba16Float)
    // carries them fine.
    spec.label = "IBL BRDF LUT Bake";
    spec
}

/// Returns the irradiance convolution's layout: the env cube at 0, the sampler
/// at 1, the per-face basis at 2.
pub(super) fn irradiance_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::Cube,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
        uniform_entry(2),
    ]
}

/// Returns the equirectangular projection's layout: the 2D lat-long map at 0,
/// its sampler at 1, the per-face basis at 2.
pub(super) fn equirect_layout_entries() -> Vec<BindGroupLayoutEntry> {
    vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStageFlags::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
        },
        uniform_entry(2),
    ]
}

/// Returns the equirectangular-to-cube projection pipeline's spec.
pub(super) fn equirect_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Equirect Bake",
        EQUIRECT_SHADER,
        vec![LayoutSpec::Inline {
            label: EQUIRECT_LAYOUT,
            entries: Cow::Owned(equirect_layout_entries()),
        }],
    )
}

/// Returns the irradiance convolution pipeline's spec.
pub(super) fn irradiance_pipeline_spec() -> PipelineSpec {
    bake_pipeline_spec(
        "IBL Irradiance Bake",
        IRRADIANCE_SHADER,
        vec![LayoutSpec::Inline {
            label: IRRADIANCE_LAYOUT,
            entries: Cow::Owned(irradiance_layout_entries()),
        }],
    )
}

/// Returns the shape the bake pipelines share: a fullscreen triangle, no depth,
/// writing linear HDR into one target.
fn bake_pipeline_spec(
    label: &'static str,
    shader: &'static str,
    bind_group_layouts: Vec<LayoutSpec>,
) -> PipelineSpec {
    PipelineSpec {
        label,
        shader,
        variant: ShaderVariantKey::empty(),
        bind_group_layouts,
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: IBL_FORMAT,
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
