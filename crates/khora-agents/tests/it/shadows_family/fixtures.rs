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

//! The shadows family's registration and agent as the engine boots them, the
//! runtime and bus they run against, and a device that records what reaches
//! it: the depth textures it made, the encoders it opened and finished, and
//! the buffers submitted to its queue.

use std::any::TypeId;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_agents::registered_agent::registered_agents;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::light::{DirectionalLight, LightType};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::flow::{ShadowMatrices, ShadowView};
use khora_data::render::{ExtractedLight, RenderWorld};
use khora_data::AssetStore;
use khora_lanes::shadow_lane::ShadowsFamily;

const MIB: u64 = 1024 * 1024;

/// The graphics memory each shadow level holds, cheapest first.
pub const LEVEL_MEMORY: [u64; 3] = [
    4 * MIB + 3 * MIB / 2,
    16 * MIB + 6 * MIB,
    64 * MIB + 24 * MIB,
];

// ─── The registration ───────────────────────────────────────────────────────

/// The shadows family's registration, as the engine collects it.
pub fn shadows_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<ShadowsFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the shadows family is not registered: no registration of `ShadowsFamily` was submitted"
        ),
        many => panic!("the shadows family is registered {} times", many.len()),
    }
}

/// The shadows family's agent, as the engine builds it from its registration.
pub fn shadow_agent() -> LaneAgent {
    lane_agent(shadows_registration(), &DeviceCaps::default())
}

// ─── The device ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Log {
    textures: Vec<Extent3D>,
    opened: Vec<CommandBufferId>,
    finished: Vec<CommandBufferId>,
    submitted: Vec<CommandBufferId>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A device that makes whatever it is asked to, and records the depth
/// textures it made, the encoders it opened and finished, and what was
/// submitted to its queue.
#[derive(Debug, Default)]
pub struct SubmitLog {
    next_id: AtomicUsize,
    log: Arc<Mutex<Log>>,
    /// Whether its encoders fail to finish.
    unfinishable: bool,
}

impl SubmitLog {
    /// A device whose encoders finish.
    pub fn new() -> Self {
        Self::default()
    }

    /// A device whose encoders never finish.
    pub fn unfinishable() -> Self {
        Self {
            unfinishable: true,
            ..Self::default()
        }
    }

    fn id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The sides of the square depth textures made, sorted: the atlases.
    pub fn atlas_sides(&self) -> Vec<u32> {
        let mut sides: Vec<u32> = locked(&self.log)
            .textures
            .iter()
            .map(|size| size.width)
            .collect();
        sides.sort_unstable();
        sides
    }

    /// The buffers of every encoder opened.
    pub fn opened(&self) -> Vec<CommandBufferId> {
        locked(&self.log).opened.clone()
    }

    /// The buffers of every encoder finished.
    pub fn finished(&self) -> Vec<CommandBufferId> {
        locked(&self.log).finished.clone()
    }

    /// The buffers submitted to the queue.
    pub fn submitted(&self) -> Vec<CommandBufferId> {
        locked(&self.log).submitted.clone()
    }
}

struct Finishing {
    buffer: CommandBufferId,
    log: Arc<Mutex<Log>>,
    unfinishable: bool,
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
        if self.unfinishable {
            return None;
        }
        locked(&self.log).finished.push(self.buffer);
        Some(self.buffer)
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
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        if d.format == TextureFormat::Depth32Float {
            locked(&self.log).textures.push(d.size);
        }
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
        let mut log = locked(&self.log);
        let buffer = CommandBufferId(100 + log.opened.len() as u64);
        log.opened.push(buffer);
        Box::new(Finishing {
            buffer,
            log: Arc::clone(&self.log),
            unfinishable: self.unfinishable,
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        locked(&self.log).submitted.push(command_buffer);
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

/// A `PipelineSystem` answering every request with a fresh id.
#[derive(Debug, Default)]
pub struct Pipelines {
    next_id: AtomicUsize,
}

impl Pipelines {
    fn next(&self) -> usize {
        1_000_000 + self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl PipelineSystem for Pipelines {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: LayoutKey,
        _variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        Ok(BindGroupLayoutId(self.next()))
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        Ok(BindGroupLayoutId(self.next()))
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        Ok(RenderPipelineId(self.next()))
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        Ok(ComputePipelineId(self.next() as u64))
    }
}

// ─── The runtime and the frame ──────────────────────────────────────────────

/// A runtime as the engine fills it for shadows: the device among the
/// backends, the pipeline system, and the asset store.
pub fn shadows_runtime(device: &Arc<SubmitLog>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(device) as Arc<dyn GraphicsDevice>);
    runtime
        .resources
        .insert(Arc::new(Pipelines::default()) as Arc<dyn PipelineSystem>);
    runtime.resources.insert(AssetStore::new());
    runtime
}

/// A bus for frame 1, holding what the render and shadow Flows publish for a
/// scene with one shadowed sun.
pub fn bus_with_a_sun() -> LaneBus {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    let mut render_world = RenderWorld::new();
    render_world.lights.push(ExtractedLight {
        light_type: LightType::Directional(DirectionalLight {
            shadow_enabled: true,
            ..Default::default()
        }),
        position: Vec3::new(0.0, 20.0, 0.0),
        direction: Vec3::new(0.0, -1.0, 0.0),
        shadow_view_proj: Mat4::IDENTITY,
        shadow_atlas_index: None,
    });
    let mut shadows = ShadowView {
        light_count: 1,
        ..ShadowView::default()
    };
    shadows.matrices.insert(
        0,
        ShadowMatrices::Single(Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0))),
    );
    bus.publish(render_world);
    bus.publish(shadows);
    bus
}

/// Runs `agent` once over `bus` into `deck`, as the scheduler runs it: with
/// the agent's own declaration and no world.
pub fn run_once(
    agent: &mut LaneAgent,
    runtime: &Arc<Runtime>,
    bus: &LaneBus,
    deck: &mut OutputDeck,
) {
    let permit = agent.contention();
    let id = agent.id();
    let mut context = EngineContext::for_agent(
        WorldAccess::None,
        Arc::clone(runtime),
        bus,
        deck,
        &permit,
        Some(id),
    );
    agent.execute(&mut context);
}
