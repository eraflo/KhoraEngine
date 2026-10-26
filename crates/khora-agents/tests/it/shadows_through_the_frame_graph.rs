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

//! The shadow pass is submitted by the engine, in the frame graph's order,
//! like every other pass: the shadow agent writes it into its pass slot and
//! never hands a command buffer to the device itself.

use std::any::TypeId;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_agents::shadow_agent::ShadowAgent;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Agent;
use khora_core::lane::{LaneBus, OutputDeck};
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
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::render::{RenderWorld, ShadowPassSlot};
use khora_data::AssetStore;

/// The command buffer every encoder of [`SubmitLog`] finishes into.
const SHADOW_BUFFER: CommandBufferId = CommandBufferId(41);

/// A device that creates whatever it is asked to and records what reaches
/// the queue: the buffers finished by its encoders and the buffers submitted.
#[derive(Debug, Default)]
struct SubmitLog {
    next_id: AtomicUsize,
    finished: Arc<Mutex<Vec<CommandBufferId>>>,
    submitted: Mutex<Vec<CommandBufferId>>,
}

impl SubmitLog {
    fn id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }
}

struct Finishing {
    finished: Arc<Mutex<Vec<CommandBufferId>>>,
}

struct Ignored;

impl RenderPass<'_> for Ignored {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {}
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

impl ComputePass<'_> for Ignored {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for Finishing {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        Box::new(Ignored)
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(Ignored)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(Ignored)
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
        self.finished
            .lock()
            .expect("the finish log")
            .push(SHADOW_BUFFER);
        Some(SHADOW_BUFFER)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for SubmitLog {
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
        Box::new(Finishing {
            finished: Arc::clone(&self.finished),
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        self.submitted
            .lock()
            .expect("the submit log")
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

/// **Shadows go through the frame graph.** Run over a frame, the shadow agent
/// submits nothing to the device; every command buffer it finished is in its
/// `ShadowPassSlot`, which it declares, for the engine to submit first, in the
/// registered pass order.
#[test]
fn shadows_go_through_the_frame_graph() {
    let mut agent = ShadowAgent::default();
    let contention = agent.contention();
    let device = Arc::new(SubmitLog::default());
    let mut runtime = Runtime::new();
    runtime.resources.insert(AssetStore::new());
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
    let runtime = Arc::new(runtime);
    let mut bus = LaneBus::new();
    bus.publish(RenderWorld::new());
    let mut deck = OutputDeck::new();

    {
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(&runtime),
            &bus,
            &mut deck,
            &contention,
            Some(AgentId::ShadowRenderer),
        );
        agent.execute(&mut ctx);
    }

    assert!(
        device.submitted.lock().expect("the submit log").is_empty(),
        "the shadow agent submitted {:?} itself; the engine submits passes",
        device.submitted.lock().expect("the submit log")
    );
    let in_slot = deck
        .get::<ShadowPassSlot>()
        .and_then(|slot| slot.0.as_ref())
        .map(|pass| pass.command_buffer);
    for finished in device.finished.lock().expect("the finish log").iter() {
        assert_eq!(
            in_slot,
            Some(*finished),
            "a shadow command buffer was finished but is not in the ShadowPassSlot"
        );
    }
    assert!(
        contention.deck.contains(&TypeId::of::<ShadowPassSlot>()),
        "the shadow agent declares the ShadowPassSlot it writes; it declares {:?}",
        contention.deck
    );
}
