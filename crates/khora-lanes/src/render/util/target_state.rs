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

//! What a scene pipeline takes from the target it draws into: the colour
//! format, the depth format and the samples, all from the target's key.

use khora_core::lane::io::TargetKey;
use khora_core::renderer::api::pipeline::enums::CompareFunction;
use khora_core::renderer::api::pipeline::state::{
    BlendStateDescriptor, ColorWrites, DepthBiasState, StencilFaceState,
};
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
};

use super::sample_count;

/// The depth test of a scene draw into a target of `key`: `Less`, writing
/// depth unless the draw blends — two blended surfaces composite only if
/// neither writes depth. `None` for a target without depth.
pub(crate) fn scene_depth_state(
    key: TargetKey,
    blend: bool,
) -> Option<DepthStencilStateDescriptor> {
    key.depth.map(|format| DepthStencilStateDescriptor {
        format,
        depth_write_enabled: !blend,
        depth_compare: CompareFunction::Less,
        stencil_front: StencilFaceState::default(),
        stencil_back: StencilFaceState::default(),
        stencil_read_mask: 0,
        stencil_write_mask: 0,
        bias: DepthBiasState::default(),
    })
}

/// The one colour target of a scene draw into a target of `key`, alpha
/// blended when the draw blends.
pub(crate) fn scene_color_targets(key: TargetKey, blend: bool) -> Vec<ColorTargetStateDescriptor> {
    vec![ColorTargetStateDescriptor {
        format: key.color,
        blend: blend.then(BlendStateDescriptor::alpha_blending),
        write_mask: ColorWrites::ALL,
    }]
}

/// The multisampling of a draw into a target of `key`.
pub(crate) fn scene_multisample(key: TargetKey) -> MultisampleStateDescriptor {
    MultisampleStateDescriptor {
        count: sample_count(key.samples),
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}
