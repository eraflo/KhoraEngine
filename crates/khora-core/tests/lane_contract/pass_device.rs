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

//! A device that only opens command encoders, and keeps a ledger of them:
//! every encoder opened, every draw recorded through one, every encoder
//! finished, every buffer submitted. The pass tests need nothing else from a
//! device, so every other call is a test failure.

use std::future::Future;
use std::sync::{Arc, Mutex};

use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId,
    CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};
use khora_core::renderer::GraphicsDevice;

const ENCODERS_ONLY: &str = "a pass output only opens an encoder on the device";

/// What the encoders of one [`EncoderLedger`] did.
#[derive(Debug, Default)]
struct Log {
    /// Encoders opened, by the id they finish into.
    opened: Vec<CommandBufferId>,
    /// Draws recorded, by the encoder they were recorded through.
    draws: Vec<CommandBufferId>,
    /// Encoders finished.
    finished: Vec<CommandBufferId>,
    /// Buffers submitted to the queue.
    submitted: Vec<CommandBufferId>,
}

/// A device opening encoders and keeping a ledger of what they did.
#[derive(Debug, Default)]
pub struct EncoderLedger {
    log: Arc<Mutex<Log>>,
    /// Whether its encoders fail to finish, as a backend with a poisoned
    /// buffer registry does.
    unfinishable: bool,
}

impl EncoderLedger {
    /// A device whose encoders finish.
    pub fn new() -> Self {
        Self::default()
    }

    /// A device whose encoders never finish: `finish` gives back no buffer.
    pub fn unfinishable() -> Self {
        Self {
            unfinishable: true,
            ..Self::default()
        }
    }

    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The buffers of every encoder opened, in order.
    pub fn opened(&self) -> Vec<CommandBufferId> {
        self.log().opened.clone()
    }

    /// The buffer each draw was recorded into, in order.
    pub fn draws(&self) -> Vec<CommandBufferId> {
        self.log().draws.clone()
    }

    /// The buffers of every encoder finished, in order.
    pub fn finished(&self) -> Vec<CommandBufferId> {
        self.log().finished.clone()
    }

    /// The buffers submitted to the queue.
    pub fn submitted(&self) -> Vec<CommandBufferId> {
        self.log().submitted.clone()
    }
}

/// One encoder of an [`EncoderLedger`]: it finishes into `buffer`.
struct LedgerEncoder {
    buffer: CommandBufferId,
    log: Arc<Mutex<Log>>,
    unfinishable: bool,
}

/// A render pass of a [`LedgerEncoder`]: it logs its draws.
struct LedgerPass {
    buffer: CommandBufferId,
    log: Arc<Mutex<Log>>,
}

impl LedgerPass {
    fn draw_logged(&self) {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .draws
            .push(self.buffer);
    }
}

impl RenderPass<'_> for LedgerPass {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {
        self.draw_logged();
    }
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {
        self.draw_logged();
    }
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

struct NoCompute;

impl ComputePass<'_> for NoCompute {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for LedgerEncoder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        Box::new(LedgerPass {
            buffer: self.buffer,
            log: Arc::clone(&self.log),
        })
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(NoCompute)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(NoCompute)
    }
    fn copy_buffer_to_buffer(
        &mut self,
        _src: &BufferId,
        _src_off: u64,
        _dst: &BufferId,
        _dst_off: u64,
        _size: u64,
    ) {
    }
    fn finish(self: Box<Self>) -> Option<CommandBufferId> {
        if self.unfinishable {
            return None;
        }
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finished
            .push(self.buffer);
        Some(self.buffer)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for EncoderLedger {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        _size: Extent3D,
    ) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_texture_view(
        &self,
        _texture_id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        unreachable!("{ENCODERS_ONLY}")
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        let mut log = self.log();
        // Buffer ids start at 100, so a zeroed id is never mistaken for one.
        let buffer = CommandBufferId(100 + log.opened.len() as u64);
        log.opened.push(buffer);
        Box::new(LedgerEncoder {
            buffer,
            log: Arc::clone(&self.log),
            unfinishable: self.unfinishable,
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        self.log().submitted.push(command_buffer);
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        None
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (1, 1)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo::default()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}
