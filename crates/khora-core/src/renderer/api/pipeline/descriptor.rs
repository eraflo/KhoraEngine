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

//! Main pipeline descriptors.

use super::layout::PipelineLayoutId;
use super::state::*;
use crate::renderer::api::{core::ShaderModuleId, util::SampleCount};
use std::borrow::Cow;

/// A complete descriptor for a render pipeline.
///
/// This struct aggregates all the state needed by the GPU to render primitives.
#[derive(Debug, Clone)]
pub struct RenderPipelineDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The compiled vertex shader module.
    pub vertex_shader_module: ShaderModuleId,
    /// The name of the entry point function in the vertex shader.
    pub vertex_entry_point: Cow<'a, str>,
    /// The compiled fragment shader module, if any.
    pub fragment_shader_module: Option<ShaderModuleId>,
    /// The name of the entry point function in the fragment shader.
    pub fragment_entry_point: Option<Cow<'a, str>>,
    /// The layout of the vertex buffers.
    pub vertex_buffers_layout: Cow<'a, [VertexBufferLayoutDescriptor<'a>]>,
    /// The pipeline layout, if any.
    pub layout: Option<PipelineLayoutId>,
    /// The state for primitive assembly and rasterization.
    pub primitive_state: PrimitiveStateDescriptor,
    /// The state for depth and stencil testing. If `None`, these tests are disabled.
    pub depth_stencil_state: Option<DepthStencilStateDescriptor>,
    /// The states of all color targets this pipeline will render to.
    pub color_target_states: Cow<'a, [ColorTargetStateDescriptor]>,
    /// The multisampling state.
    pub multisample_state: MultisampleStateDescriptor,
}

/// Describes the multisampling state for a render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultisampleStateDescriptor {
    /// The number of samples per pixel.
    pub count: SampleCount,
    /// A bitmask where each bit corresponds to a sample. `!0` means all samples are affected.
    pub mask: u32,
    /// If `true`, enables alpha-to-coverage, using the fragment's alpha value to determine coverage.
    pub alpha_to_coverage_enabled: bool,
}

/// An opaque handle to a compiled render pipeline state object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderPipelineId(pub usize);
