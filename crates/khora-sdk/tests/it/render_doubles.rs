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

//! A renderer and a device that run a frame headless and record what the
//! engine asked of them: the views it prepared the frame with, and the
//! command buffers it submitted, in order.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::math::dimension::{Extent3D, Origin3D};
use khora_sdk::khora_core::math::Extent2D;
use khora_sdk::khora_core::platform::window::KhoraWindow;
use khora_sdk::khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId,
    CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_sdk::khora_core::renderer::api::device::{
    GraphicsAdapterInfo, RenderSettings, RenderStats,
};
use khora_sdk::khora_core::renderer::api::gpu_scene::RenderObject;
use khora_sdk::khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_sdk::khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewId, ViewInfo,
};
use khora_sdk::khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_sdk::khora_core::renderer::error::{RenderError, ResourceError};
use khora_sdk::khora_core::renderer::traits::{
    CommandEncoder, ComputePass, FrameTargets, GpuProfiler, RenderPass,
};
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::khora_core::telemetry::ResourceMonitor;
use khora_sdk::khora_core::Runtime;

/// A renderer drawing into a `size` colour target, recording every view the
/// engine prepares a frame with.
#[derive(Debug)]
pub struct RecordingRenderer {
    size: Extent2D,
    prepared: Arc<Mutex<Vec<ViewInfo>>>,
    stats: RenderStats,
}

impl RecordingRenderer {
    /// A renderer into a `width`×`height` target, and the views it will be
    /// prepared with.
    pub fn into_target(width: u32, height: u32) -> (Self, Arc<Mutex<Vec<ViewInfo>>>) {
        let prepared = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                size: Extent2D { width, height },
                prepared: Arc::clone(&prepared),
                stats: RenderStats::default(),
            },
            prepared,
        )
    }
}

impl khora_sdk::RenderSystem for RecordingRenderer {
    fn init(
        &mut self,
        _window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError> {
        Ok(Vec::new())
    }
    fn resize(&mut self, width: u32, height: u32) {
        self.size = Extent2D { width, height };
    }
    fn prepare_frame(&mut self, view_info: &ViewInfo) {
        self.prepared
            .lock()
            .expect("the prepared views")
            .push(view_info.clone());
    }
    fn render(
        &mut self,
        _renderables: &[RenderObject],
        _view_info: &ViewInfo,
        _settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError> {
        Ok(RenderStats::default())
    }
    fn get_last_frame_stats(&self) -> &RenderStats {
        &self.stats
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo> {
        None
    }
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice> {
        unimplemented!("the engine reaches the device through the backends")
    }
    fn begin_frame(&mut self) -> Result<FrameTargets, RenderError> {
        Ok(FrameTargets {
            color: TextureViewId(1),
            depth: None,
            size: self.size,
        })
    }
    fn end_frame(&mut self) -> Result<RenderStats, RenderError> {
        Ok(RenderStats::default())
    }
    fn shutdown(&mut self) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Installs `renderer` the way the engine reaches it: an
/// `Arc<Mutex<Box<dyn RenderSystem>>>` among the backends.
pub fn install_renderer(runtime: &mut Runtime, renderer: RecordingRenderer) {
    let renderer: Arc<Mutex<Box<dyn khora_sdk::RenderSystem>>> =
        Arc::new(Mutex::new(Box::new(renderer)));
    runtime.backends.insert(renderer);
}

/// A device that creates whatever it is asked to and records every command
/// buffer submitted to it, in order.
#[derive(Debug, Default)]
pub struct QueueLog {
    next_id: AtomicUsize,
    submitted: Mutex<Vec<CommandBufferId>>,
}

impl QueueLog {
    /// The command buffers submitted so far, in submission order.
    pub fn submitted(&self) -> Vec<CommandBufferId> {
        self.submitted.lock().expect("the queue log").clone()
    }

    fn id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }
}

struct NothingRecorded;

impl RenderPass<'_> for NothingRecorded {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {}
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

impl ComputePass<'_> for NothingRecorded {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for NothingRecorded {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        Box::new(NothingRecorded)
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(NothingRecorded)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(NothingRecorded)
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
        Some(CommandBufferId(0))
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for QueueLog {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.id()))
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(self.id()))
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.id()))
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(self.id() as u64))
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(self.id()))
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        Ok(BindGroupId(self.id()))
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        Ok(BufferId(self.id()))
    }
    fn create_buffer_with_data(
        &self,
        _d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        Ok(BufferId(self.id()))
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        Box::new(async { Ok(()) })
    }
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        Ok(TextureId(self.id()))
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: Origin3D,
        _size: Extent3D,
    ) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_texture_view(
        &self,
        _texture_id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        Ok(TextureViewId(self.id()))
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.id()))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        Box::new(NothingRecorded)
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        self.submitted
            .lock()
            .expect("the queue log")
            .push(command_buffer);
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(TextureFormat::Rgba8UnormSrgb)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (800, 600)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo::default()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}

/// A frame context among the runtime's resources, as the windowing driver
/// inserts it, with the tokio runtime it was made with kept alive beside it.
pub fn insert_frame_context(runtime: &mut Runtime) -> tokio::runtime::Runtime {
    let tokio = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a tokio runtime");
    let context = Arc::new(
        khora_sdk::khora_core::renderer::api::frame::FrameContext::new(tokio.handle().clone()),
    );
    runtime.resources.insert(context);
    tokio
}
