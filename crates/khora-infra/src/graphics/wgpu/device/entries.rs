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

//! The records the device keeps for each GPU resource it created.

use std::sync::Arc;
use wgpu;

pub(crate) use super::wgpu_bind_group_entry::WgpuBindGroupEntry;
pub(crate) use super::wgpu_bind_group_layout_entry::WgpuBindGroupLayoutEntry;
pub(crate) use super::wgpu_buffer_entry::WgpuBufferEntry;
pub(crate) use super::wgpu_compute_pipeline_entry::WgpuComputePipelineEntry;
pub(crate) use super::wgpu_pipeline_layout_entry::WgpuPipelineLayoutEntry;
pub(crate) use super::wgpu_render_pipeline_entry::WgpuRenderPipelineEntry;
pub(crate) use super::wgpu_sampler_entry::WgpuSamplerEntry;
pub(crate) use super::wgpu_texture_entry::WgpuTextureEntry;
pub(crate) use super::wgpu_texture_view_entry::WgpuTextureViewEntry;

#[derive(Debug)]
pub(crate) struct WgpuShaderModuleEntry {
    pub(super) wgpu_module: Arc<wgpu::ShaderModule>,
}
