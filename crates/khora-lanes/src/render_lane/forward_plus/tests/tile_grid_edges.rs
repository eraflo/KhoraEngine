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

//! The Forward+ light lists at the edges: what the culling shader indexes,
//! allocations that fail half-way, a lane torn down after a regrow, and the
//! frame path that sizes and dispatches from the colour target.

use super::super::tile_grid::TileGrid;
use super::super::{ForwardPlusGpuResources, ForwardPlusLane};
use khora_core::lane::{ClearColor, ColorTarget, ColorTargetSize, DepthTarget, Lane, LaneContext};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, LinearRgba, Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::{
    GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::texture::{
    SamplerDescriptor, SamplerId, TextureDescriptor, TextureId, TextureViewDescriptor,
    TextureViewId,
};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferId, IndexFormat, TextureFormat};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::light::CullingUniformsData;
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::{ForwardPlusTileConfig, GraphicsDevice, TileSize};
use khora_data::assets::Assets;
use khora_data::render::{ExtractedView, RenderWorld};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// The light-culling compute shader the light lists are bound to.
const LIGHT_CULLING_WGSL: &str = include_str!(
    "../../../../../khora-infra/src/graphics/shader/shaders/pipelines/light_culling.wgsl"
);

/// The per-tile stride the culling shader writes the light-index list with
/// (`offset = tile_index * MAX_LIGHTS_PER_TILE`).
fn shader_lights_per_tile() -> u64 {
    LIGHT_CULLING_WGSL
        .lines()
        .find_map(|line| {
            let rest = line
                .trim()
                .strip_prefix("const MAX_LIGHTS_PER_TILE: u32 =")?;
            rest.trim()
                .trim_end_matches(';')
                .trim_end_matches('u')
                .parse()
                .ok()
        })
        .expect("light_culling.wgsl declares MAX_LIGHTS_PER_TILE")
}

#[test]
fn the_lanes_light_stride_is_the_culling_shaders() {
    assert_eq!(
        u64::from(super::super::gpu_resources::CULLING_SHADER_LIGHTS_PER_TILE),
        shader_lights_per_tile(),
        "the light-index list is laid out by light_culling.wgsl's MAX_LIGHTS_PER_TILE"
    );
}

// --- A device that records what it is asked for, and can refuse one creation ---

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    CreateBuffer {
        id: BufferId,
        size: u64,
        label: String,
    },
    CreateBindGroup(BindGroupId),
    DestroyBuffer(BufferId),
    DestroyBindGroup(BindGroupId),
}

#[derive(Debug, Default)]
struct RecordingDevice {
    next_id: AtomicUsize,
    events: Mutex<Vec<Event>>,
    writes: Mutex<Vec<(BufferId, Vec<u8>)>>,
    /// Buffer and bind-group creations seen so far.
    creations: AtomicUsize,
    /// The creation (1-based, counted by `creations`) that fails; 0 = none.
    fail_creation: AtomicUsize,
}

impl RecordingDevice {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn record(&self, event: Event) {
        self.events.lock().expect("events lock").push(event);
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("events lock").clone()
    }

    fn forget(&self) {
        self.events.lock().expect("events lock").clear();
        self.writes.lock().expect("writes lock").clear();
    }

    /// Fails the `n`-th creation from now on (1 = the next one).
    fn fail_the_nth_creation_from_now(&self, n: usize) {
        self.fail_creation.store(
            self.creations.load(Ordering::Relaxed) + n,
            Ordering::Relaxed,
        );
    }

    fn creation(&self) -> Result<(), ResourceError> {
        let n = self.creations.fetch_add(1, Ordering::Relaxed) + 1;
        if n == self.fail_creation.load(Ordering::Relaxed) {
            Err(ResourceError::BackendError("out of memory (test)".into()))
        } else {
            Ok(())
        }
    }

    fn created_buffers(&self) -> Vec<(BufferId, u64, String)> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::CreateBuffer { id, size, label } => Some((id, size, label)),
                _ => None,
            })
            .collect()
    }

    fn created_bind_groups(&self) -> Vec<BindGroupId> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::CreateBindGroup(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn destroyed_buffers(&self) -> Vec<BufferId> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::DestroyBuffer(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn destroyed_bind_groups(&self) -> Vec<BindGroupId> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                Event::DestroyBindGroup(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn size_of(&self, id: BufferId) -> u64 {
        self.created_buffers()
            .into_iter()
            .find(|(buffer, _, _)| *buffer == id)
            .map(|(_, size, _)| size)
            .expect("buffer created by this device")
    }

    fn last_write_to(&self, id: BufferId) -> Option<Vec<u8>> {
        self.writes
            .lock()
            .expect("writes lock")
            .iter()
            .rev()
            .find(|(buffer, _)| *buffer == id)
            .map(|(_, bytes)| bytes.clone())
    }
}

/// The compute dispatches every encoder of a test recorded.
type Dispatches = Arc<Mutex<Vec<(u32, u32, u32)>>>;

struct RecordingEncoder {
    dispatches: Dispatches,
}
struct MockRenderPass;
struct RecordingComputePass {
    dispatches: Dispatches,
}

impl RenderPass<'_> for MockRenderPass {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {}
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

impl ComputePass<'_> for RecordingComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        self.dispatches
            .lock()
            .expect("dispatches lock")
            .push((x, y, z));
    }
}

impl CommandEncoder for RecordingEncoder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        Box::new(MockRenderPass)
    }

    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(RecordingComputePass {
            dispatches: self.dispatches.clone(),
        })
    }

    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(RecordingComputePass {
            dispatches: self.dispatches.clone(),
        })
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

impl GraphicsDevice for RecordingDevice {
    fn create_shader_module(
        &self,
        _d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.next()))
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(self.next()))
    }
    fn create_pipeline_layout(
        &self,
        _d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.next()))
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_compute_pipeline(
        &self,
        _d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(self.next() as u64))
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_bind_group_layout(
        &self,
        _d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(self.next()))
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.creation()?;
        let id = BindGroupId(self.next());
        self.record(Event::CreateBindGroup(id));
        Ok(id)
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.record(Event::DestroyBindGroup(id));
        Ok(())
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        self.creation()?;
        let id = BufferId(self.next());
        let label = d.label.as_deref().unwrap_or_default().to_string();
        self.record(Event::CreateBuffer {
            id,
            size: d.size,
            label,
        });
        Ok(id)
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.create_buffer(d)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.record(Event::DestroyBuffer(id));
        Ok(())
    }
    fn write_buffer(&self, id: BufferId, _offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        self.writes
            .lock()
            .expect("writes lock")
            .push((id, data.to_vec()));
        Ok(())
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        Box::new(async { Ok(()) })
    }
    fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        Ok(TextureId(self.next()))
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn write_texture(
        &self,
        _id: TextureId,
        _data: &[u8],
        _bpr: Option<u32>,
        _offset: Origin3D,
        _size: Extent3D,
    ) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_texture_view(
        &self,
        _id: TextureId,
        _d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        Ok(TextureViewId(self.next()))
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.next()))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        Box::new(RecordingEncoder {
            dispatches: Dispatches::default(),
        })
    }
    fn submit_command_buffer(&self, _cb: CommandBufferId) {}
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(TextureFormat::Rgba8UnormSrgb)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        // Deliberately unlike every target below: the lane must not read it.
        (3, 3)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo {
            name: "RecordingDevice".to_string(),
            backend_type: GraphicsBackendType::Unknown,
            device_type: RendererDeviceType::Unknown,
        }
    }
    fn supports_feature(&self, _feature: &str) -> bool {
        true
    }
}

/// A pipeline system that answers every request with a fresh id.
#[derive(Debug, Default)]
struct FakePipelines {
    next_id: AtomicUsize,
}

impl FakePipelines {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 10_000_000
    }
}

impl PipelineSystem for FakePipelines {
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

// --- Fixtures ---

const CULLING_LAYOUT: BindGroupLayoutId = BindGroupLayoutId(1_000_001);
const CULLING_UNIFORMS: BufferId = BufferId(1_000_002);
const LIGHTS: BufferId = BufferId(1_000_003);

fn initialised_resources() -> ForwardPlusGpuResources {
    ForwardPlusGpuResources {
        culling_layout: Some(CULLING_LAYOUT),
        culling_uniforms_buffer: Some(CULLING_UNIFORMS),
        light_buffer: Some(LIGHTS),
        ..Default::default()
    }
}

fn config(tile_size: TileSize, max_lights_per_tile: u32) -> ForwardPlusTileConfig {
    ForwardPlusTileConfig {
        tile_size,
        max_lights_per_tile,
        use_depth_prepass: false,
    }
}

fn extent(width: u32, height: u32) -> Extent2D {
    Extent2D { width, height }
}

fn grid(config: &ForwardPlusTileConfig, width: u32, height: u32) -> TileGrid {
    TileGrid::new(config, extent(width, height))
}

fn held(
    r: &ForwardPlusGpuResources,
) -> (Option<BufferId>, Option<BufferId>, Option<BindGroupId>, u32) {
    (
        r.light_index_buffer,
        r.light_grid_buffer,
        r.culling_bind_group,
        r.tile_capacity,
    )
}

/// A lane whose GPU initialisation ran on `device`.
fn initialised_lane(lane: &ForwardPlusLane, device: &Arc<RecordingDevice>) {
    let pipelines: Arc<dyn PipelineSystem> = Arc::new(FakePipelines::default());
    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert(pipelines);
    lane.on_initialize(&mut ctx)
        .expect("Forward+ initialises on a recording device");
}

/// Runs one Forward+ frame of a scene with one camera on a target of
/// `target`, and returns the culling dispatches it recorded.
fn run_frame(
    lane: &ForwardPlusLane,
    device: &Arc<RecordingDevice>,
    target: Extent2D,
) -> Vec<(u32, u32, u32)> {
    let dispatches = Dispatches::default();
    let mut encoder: Box<dyn CommandEncoder> = Box::new(RecordingEncoder {
        dispatches: dispatches.clone(),
    });
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
    let render_world = RenderWorld {
        views: vec![ExtractedView {
            view_proj: Mat4::IDENTITY,
            position: Vec3::ZERO,
        }],
        ..Default::default()
    };

    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert(gpu_meshes);
    ctx.insert_slot(encoder.as_mut());
    ctx.insert_ref(&render_world);
    ctx.insert(ColorTarget(TextureViewId(1)));
    ctx.insert(DepthTarget(TextureViewId(2)));
    ctx.insert(ClearColor(LinearRgba::BLACK));
    ctx.insert(ColorTargetSize(target));
    lane.execute(&mut ctx)
        .expect("a Forward+ frame on a sized target");
    drop(ctx);

    let recorded = dispatches.lock().expect("dispatches lock").clone();
    recorded
}

// --- What the culling shader indexes ---

#[test]
fn the_light_index_list_holds_the_culling_shaders_stride_for_every_tile() {
    // The culling shader writes tile `t`'s lights at `t * MAX_LIGHTS_PER_TILE`
    // (a WGSL constant), whatever the config's budget: the buffer must reach
    // the last tile's last slot, or the writes for the upper tiles fall off it.
    let device = RecordingDevice::default();
    let config = config(TileSize::X16, 64);
    let mut resources = initialised_resources();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080))
        .expect("light lists allocated");

    let tiles = 120 * 68;
    let index = resources.light_index_buffer.expect("index");
    let needed = tiles * shader_lights_per_tile() * 4;
    assert!(
        device.size_of(index) >= needed,
        "light index list is {} bytes; the culling shader writes up to {needed}",
        device.size_of(index)
    );
}

#[test]
fn a_zero_light_budget_never_creates_a_zero_sized_light_list() {
    let device = RecordingDevice::default();
    let config = config(TileSize::X16, 0);
    let mut resources = initialised_resources();

    let _ = resources.ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080));

    let empty: Vec<_> = device
        .created_buffers()
        .into_iter()
        .filter(|(_, size, _)| *size == 0)
        .collect();
    assert!(empty.is_empty(), "zero-sized buffers created: {empty:?}");
}

// --- Allocations that fail half-way ---

#[test]
fn a_light_grid_that_fails_to_allocate_frees_the_new_index_list() {
    let device = RecordingDevice::default();
    let config = config(TileSize::X16, 128);
    let mut resources = initialised_resources();
    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("first frame");
    let before = held(&resources);
    device.forget();
    // The index list is created, the grid is refused.
    device.fail_the_nth_creation_from_now(2);

    let result = resources.ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080));

    assert!(result.is_err(), "the refused grid is reported");
    assert_eq!(held(&resources), before, "the previous light lists stay");
    let created = device.created_buffers();
    assert_eq!(created.len(), 1, "{:?}", device.events());
    assert_eq!(
        device.destroyed_buffers(),
        vec![created[0].0],
        "the orphaned new index list is freed, nothing else"
    );
    assert!(device.destroyed_bind_groups().is_empty());
}

#[test]
fn a_culling_bind_group_that_fails_frees_both_new_light_lists() {
    let device = RecordingDevice::default();
    let config = config(TileSize::X16, 128);
    let mut resources = initialised_resources();
    // First frame: index and grid are created, the bind group is refused.
    device.fail_the_nth_creation_from_now(3);

    let result = resources.ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768));

    assert!(result.is_err());
    assert_eq!(
        held(&resources),
        (None, None, None, 0),
        "nothing is half-held"
    );
    let mut created: Vec<_> = device
        .created_buffers()
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let mut destroyed = device.destroyed_buffers();
    created.sort_by_key(|id| id.0);
    destroyed.sort_by_key(|id| id.0);
    assert_eq!(created.len(), 2);
    assert_eq!(destroyed, created, "both new buffers are freed");

    // The next frame, with a healthy device, allocates normally.
    device.forget();
    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("the retry allocates");
    assert_eq!(resources.tile_capacity, 64 * 48);
}

#[test]
fn light_lists_requested_before_gpu_initialisation_fail_and_create_nothing() {
    let device = RecordingDevice::default();
    let config = config(TileSize::X16, 128);
    let mut resources = ForwardPlusGpuResources::default();

    let result = resources.ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080));

    assert!(result.is_err());
    assert_eq!(device.events(), vec![]);
    assert_eq!(held(&resources), (None, None, None, 0));
}

// --- The frame path ---

#[test]
fn each_frame_sizes_the_light_lists_and_the_dispatch_from_its_colour_target() {
    let lane = ForwardPlusLane::new();
    let device = Arc::new(RecordingDevice::default());
    initialised_lane(&lane, &device);
    let (tile_info, culling_uniforms) = {
        let r = lane.gpu_resources.lock().expect("resources");
        (
            r.tile_info_buffer.expect("tile info buffer"),
            r.culling_uniforms_buffer.expect("culling uniforms"),
        )
    };

    // 1384 x 561: 87 x 36 tiles.
    let dispatches = run_frame(&lane, &device, extent(1384, 561));
    assert_eq!(dispatches, vec![(87, 36, 1)]);
    let tile_info_bytes = device.last_write_to(tile_info).expect("tile info written");
    let tile_info_words: Vec<u32> = tile_info_bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_eq!(tile_info_words, vec![87, 36, 16, 128]);
    let culling: CullingUniformsData = bytemuck::pod_read_unaligned(
        &device
            .last_write_to(culling_uniforms)
            .expect("culling uniforms written"),
    );
    assert_eq!(culling.screen_dimensions, [1384.0, 561.0]);
    assert_eq!(culling.tile_count, [87, 36]);
    {
        let r = lane.gpu_resources.lock().expect("resources");
        assert_eq!(r.tile_capacity, 87 * 36);
        assert_eq!(
            device.size_of(r.light_index_buffer.expect("index")),
            87 * 36 * 128 * 4
        );
    }

    // Shrinking: same buffers, a smaller dispatch.
    let before = held(&lane.gpu_resources.lock().expect("resources"));
    device.forget();
    let dispatches = run_frame(&lane, &device, extent(640, 480));
    assert_eq!(dispatches, vec![(40, 30, 1)]);
    assert_eq!(held(&lane.gpu_resources.lock().expect("resources")), before);
    assert!(device
        .created_buffers()
        .iter()
        .all(|(_, _, label)| !label.contains("Light Index") && !label.contains("Light Grid")));

    // Growing past the first size: regrown to exactly the new grid.
    device.forget();
    let dispatches = run_frame(&lane, &device, extent(2560, 1440));
    assert_eq!(dispatches, vec![(160, 90, 1)]);
    let r = lane.gpu_resources.lock().expect("resources");
    assert_eq!(r.tile_capacity, 160 * 90);
    assert_eq!(
        device.size_of(r.light_index_buffer.expect("index")),
        160 * 90 * 128 * 4
    );
    assert_eq!(
        device.size_of(r.light_grid_buffer.expect("grid")),
        160 * 90 * 2 * 4
    );
    let mut destroyed = device.destroyed_buffers();
    destroyed.sort_by_key(|id| id.0);
    let mut expected = vec![before.0.expect("old index"), before.1.expect("old grid")];
    expected.sort_by_key(|id| id.0);
    assert_eq!(destroyed, expected);
    assert_eq!(
        device.destroyed_bind_groups(),
        vec![before.2.expect("old group")]
    );
}

#[test]
fn a_frame_on_a_target_without_area_dispatches_no_culling() {
    let lane = ForwardPlusLane::new();
    let device = Arc::new(RecordingDevice::default());
    initialised_lane(&lane, &device);
    run_frame(&lane, &device, extent(1024, 768));
    let before = held(&lane.gpu_resources.lock().expect("resources"));
    device.forget();

    for target in [extent(0, 768), extent(1024, 0), extent(0, 0)] {
        let dispatches = run_frame(&lane, &device, target);
        assert_eq!(dispatches, vec![], "{target:?} has no tile to cull");
    }
    assert_eq!(held(&lane.gpu_resources.lock().expect("resources")), before);
    assert!(device.destroyed_buffers().is_empty());
}

#[test]
fn shutdown_after_a_regrow_frees_the_light_lists_and_a_reinitialised_lane_regrows_them() {
    let lane = ForwardPlusLane::new();
    let device = Arc::new(RecordingDevice::default());
    initialised_lane(&lane, &device);
    run_frame(&lane, &device, extent(1024, 768));
    run_frame(&lane, &device, extent(1920, 1080));
    let (index, light_grid, group, capacity) = held(&lane.gpu_resources.lock().expect("resources"));
    assert_eq!(capacity, 120 * 68);
    device.forget();

    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    lane.on_shutdown(&mut ctx);

    {
        let r = lane.gpu_resources.lock().expect("resources");
        assert_eq!(
            held(&r),
            (None, None, None, 0),
            "nothing is left after shutdown"
        );
        assert!(!r.is_initialized());
    }
    let destroyed = device.destroyed_buffers();
    assert!(destroyed.contains(&index.expect("index")), "{destroyed:?}");
    assert!(
        destroyed.contains(&light_grid.expect("grid")),
        "{destroyed:?}"
    );
    // The camera and model rings free their own bind groups too.
    let destroyed_groups = device.destroyed_bind_groups();
    assert!(
        destroyed_groups.contains(&group.expect("group")),
        "{destroyed_groups:?}"
    );

    // Initialised again, the first frame allocates for its own target.
    initialised_lane(&lane, &device);
    device.forget();
    let dispatches = run_frame(&lane, &device, extent(1024, 768));
    assert_eq!(dispatches, vec![(64, 48, 1)]);
    let r = lane.gpu_resources.lock().expect("resources");
    assert_eq!(r.tile_capacity, 64 * 48);
    assert_eq!(
        device.size_of(r.light_index_buffer.expect("index")),
        64 * 48 * 128 * 4
    );
    assert_eq!(
        device.created_bind_groups().len(),
        1,
        "{:?}",
        device.events()
    );
}
