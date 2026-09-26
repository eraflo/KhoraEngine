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

use khora_core::renderer::api::resource::texture::{self as api_tex};

#[allow(dead_code)]
#[derive(Debug)]
pub(super) struct WgpuShaderModuleEntry {
    pub(super) wgpu_module: Arc<wgpu::ShaderModule>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuRenderPipelineEntry {
    pub(crate) wgpu_pipeline: Arc<wgpu::RenderPipeline>,
}

#[derive(Debug)]
pub(crate) struct WgpuComputePipelineEntry {
    pub(crate) wgpu_pipeline: Arc<wgpu::ComputePipeline>,
}

#[derive(Debug)]
pub(crate) struct WgpuBufferEntry {
    pub(crate) wgpu_buffer: Arc<wgpu::Buffer>,
    pub(crate) size: u64, // To track VRAM accurately on destruction
}

#[derive(Debug)]
pub(crate) struct WgpuTextureEntry {
    pub(crate) wgpu_texture: Arc<wgpu::Texture>,
    pub(crate) size: u64, // To track VRAM accurately on destruction
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct WgpuTextureViewEntry {
    pub(crate) wgpu_view: Arc<wgpu::TextureView>,
    pub(crate) source_texture_id: Option<api_tex::TextureId>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuSamplerEntry {
    pub(crate) wgpu_sampler: Arc<wgpu::Sampler>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuBindGroupLayoutEntry {
    pub(crate) wgpu_layout: Arc<wgpu::BindGroupLayout>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuBindGroupEntry {
    pub(crate) wgpu_bind_group: Arc<wgpu::BindGroup>,
}

#[derive(Debug)]
pub(crate) struct WgpuPipelineLayoutEntry {
    pub(crate) wgpu_layout: Arc<wgpu::PipelineLayout>,
}
