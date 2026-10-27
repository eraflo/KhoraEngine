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

//! A lane builds its pipelines per `TargetKey`; the `PipelineSystem` dedups
//! the pipelines it is asked for by `PipelineSpec::key()`. Two keys a lane
//! tells apart must therefore be two cache entries, or the second target is
//! handed the pipeline built for the first.

use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
use khora_core::renderer::api::pipeline::state::{ColorWrites, DepthBiasState, StencilFaceState};
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
    PipelineSpec, PrimitiveStateDescriptor,
};
use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
use khora_core::renderer::api::shader::ShaderVariantKey;

/// A depth-tested pipeline spec for a target of `key`'s formats, the way a
/// lane fills one from the key.
fn spec_for(key: TargetKey) -> PipelineSpec {
    PipelineSpec {
        label: "Keyed Pipeline",
        shader: "khora::pipelines::skybox",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: key.depth.map(|format| DepthStencilStateDescriptor {
            format,
            depth_write_enabled: false,
            depth_compare: CompareFunction::LessEqual,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState::default(),
        }),
        color_targets: vec![ColorTargetStateDescriptor {
            format: key.color,
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

/// **Two depth formats are two pipelines.** A target on a `Depth32Float`
/// buffer and one on a `Depth24Plus` buffer, same colour format: the depth
/// format is baked into the pipeline, so the cache must not hand the second
/// the first one's pipeline.
#[test]
fn two_depth_formats_are_two_cached_pipelines() {
    let depth32 = TargetKey {
        color: TextureFormat::Rgba8UnormSrgb,
        depth: Some(TextureFormat::Depth32Float),
        samples: 1,
    };
    let depth24 = TargetKey {
        depth: Some(TextureFormat::Depth24Plus),
        ..depth32
    };

    assert_ne!(
        spec_for(depth32).key(),
        spec_for(depth24).key(),
        "the pipeline cache tells the two depth formats apart"
    );
}
