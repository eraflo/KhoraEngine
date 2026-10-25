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

//! The Forward+ light-list buffers follow the colour target's tile grid, and
//! the lane reads that target's size from its context.

use super::super::tile_grid::TileGrid;
use super::super::{ForwardPlusGpuResources, ForwardPlusLane};
use khora_core::lane::{ClearColor, ColorTarget, ColorTargetSize, DepthTarget, Lane, LaneContext};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, LinearRgba};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId,
    BindingResource, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::{
    GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
};
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::texture::{
    SamplerDescriptor, SamplerId, TextureDescriptor, TextureId, TextureViewDescriptor,
    TextureViewId,
};
use khora_core::renderer::api::resource::{BufferDescriptor, BufferId, IndexFormat, TextureFormat};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};
use khora_core::renderer::{ForwardPlusTileConfig, GraphicsDevice, TileSize};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

// --- A device that records the resources it is asked for ---

/// One call the recording device saw, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    CreateBuffer {
        id: BufferId,
        size: u64,
    },
    CreateBindGroup {
        id: BindGroupId,
        layout: BindGroupLayoutId,
        /// `(binding, buffer)` for every buffer entry.
        buffers: Vec<(u32, BufferId)>,
    },
    DestroyBuffer(BufferId),
    DestroyBindGroup(BindGroupId),
}

/// A bind group the device created: its id, layout and `(binding, buffer)` entries.
type CreatedBindGroup = (BindGroupId, BindGroupLayoutId, Vec<(u32, BufferId)>);

#[derive(Debug, Default)]
struct RecordingDevice {
    next_id: AtomicUsize,
    events: Mutex<Vec<Event>>,
    /// When set, every buffer and bind-group creation fails.
    refuse_creation: AtomicBool,
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

    fn forget_events(&self) {
        self.events.lock().expect("events lock").clear();
    }

    fn created_buffers(&self) -> Vec<(BufferId, u64)> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::CreateBuffer { id, size } => Some((id, size)),
                _ => None,
            })
            .collect()
    }

    fn created_bind_groups(&self) -> Vec<CreatedBindGroup> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::CreateBindGroup {
                    id,
                    layout,
                    buffers,
                } => Some((id, layout, buffers)),
                _ => None,
            })
            .collect()
    }

    fn destroyed_buffers(&self) -> Vec<BufferId> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::DestroyBuffer(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn destroyed_bind_groups(&self) -> Vec<BindGroupId> {
        self.events()
            .into_iter()
            .filter_map(|event| match event {
                Event::DestroyBindGroup(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn size_of(&self, id: BufferId) -> u64 {
        self.created_buffers()
            .into_iter()
            .find(|(buffer, _)| *buffer == id)
            .map(|(_, size)| size)
            .expect("buffer created by this device")
    }

    fn refused(&self) -> Result<(), ResourceError> {
        if self.refuse_creation.load(Ordering::Relaxed) {
            Err(ResourceError::BackendError("out of memory (test)".into()))
        } else {
            Ok(())
        }
    }
}

struct MockCommandEncoder;
struct MockRenderPass;
struct MockComputePass;

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

impl ComputePass<'_> for MockComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for MockCommandEncoder {
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
        Box::new(MockComputePass)
    }

    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(MockComputePass)
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
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.refused()?;
        let id = BindGroupId(self.next());
        let buffers = d
            .entries
            .iter()
            .filter_map(|entry| match &entry.resource {
                BindingResource::Buffer(binding) => Some((entry.binding, binding.buffer)),
                _ => None,
            })
            .collect();
        self.record(Event::CreateBindGroup {
            id,
            layout: d.layout,
            buffers,
        });
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
        self.refused()?;
        let id = BufferId(self.next());
        self.record(Event::CreateBuffer { id, size: d.size });
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
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
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
        Box::new(MockCommandEncoder)
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

// --- Fixtures ---

/// Ids far above anything the recording device hands out.
const CULLING_LAYOUT: BindGroupLayoutId = BindGroupLayoutId(1_000_001);
const CULLING_UNIFORMS: BufferId = BufferId(1_000_002);
const LIGHTS: BufferId = BufferId(1_000_003);

/// The resources `on_gpu_init` leaves behind: everything the culling bind
/// group needs except the light lists, whose size depends on the target.
fn initialised_resources() -> ForwardPlusGpuResources {
    ForwardPlusGpuResources {
        culling_layout: Some(CULLING_LAYOUT),
        culling_uniforms_buffer: Some(CULLING_UNIFORMS),
        light_buffer: Some(LIGHTS),
        ..Default::default()
    }
}

fn x16() -> ForwardPlusTileConfig {
    ForwardPlusTileConfig {
        tile_size: TileSize::X16,
        max_lights_per_tile: 128,
        use_depth_prepass: false,
    }
}

fn grid(config: &ForwardPlusTileConfig, width: u32, height: u32) -> TileGrid {
    TileGrid::new(config, Extent2D { width, height })
}

/// Bytes of a light-index buffer for `tiles` tiles: one `u32` per light slot.
fn index_bytes(tiles: u64, max_lights_per_tile: u64) -> u64 {
    tiles * max_lights_per_tile * 4
}

/// Bytes of a light-grid buffer for `tiles` tiles: an `(offset, count)` `u32` pair.
fn grid_bytes(tiles: u64) -> u64 {
    tiles * 2 * 4
}

/// The light-list buffers and bind group the resources hold, as one value.
fn held(
    resources: &ForwardPlusGpuResources,
) -> (Option<BufferId>, Option<BufferId>, Option<BindGroupId>, u32) {
    (
        resources.light_index_buffer,
        resources.light_grid_buffer,
        resources.culling_bind_group,
        resources.tile_capacity,
    )
}

// --- ensure_tile_buffers ---

#[test]
fn the_first_frame_creates_light_lists_sized_for_its_grid() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080))
        .expect("first frame allocates its light lists");

    let tiles = 120 * 68;
    assert_eq!(device.created_buffers().len(), 2, "{:?}", device.events());
    assert_eq!(
        device.created_bind_groups().len(),
        1,
        "{:?}",
        device.events()
    );
    assert!(device.destroyed_buffers().is_empty());
    assert!(device.destroyed_bind_groups().is_empty());

    let index = resources.light_index_buffer.expect("light index buffer");
    let light_grid = resources.light_grid_buffer.expect("light grid buffer");
    assert_eq!(device.size_of(index), index_bytes(tiles, 128));
    assert_eq!(device.size_of(light_grid), grid_bytes(tiles));
    assert_eq!(resources.tile_capacity, tiles as u32);
    assert_eq!(
        resources.culling_bind_group,
        Some(device.created_bind_groups()[0].0)
    );
}

#[test]
fn the_culling_bind_group_binds_uniforms_lights_index_and_grid() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("light lists allocated");

    let (_, layout, mut buffers) = device.created_bind_groups().remove(0);
    buffers.sort_by_key(|(binding, _)| *binding);
    assert_eq!(layout, CULLING_LAYOUT);
    assert_eq!(
        buffers,
        vec![
            (0, CULLING_UNIFORMS),
            (1, LIGHTS),
            (2, resources.light_index_buffer.expect("index")),
            (3, resources.light_grid_buffer.expect("grid")),
        ]
    );
}

#[test]
fn light_list_sizes_follow_the_tile_config() {
    let device = RecordingDevice::default();
    let config = ForwardPlusTileConfig {
        tile_size: TileSize::X32,
        max_lights_per_tile: 256,
        use_depth_prepass: false,
    };
    let mut resources = initialised_resources();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1384, 561))
        .expect("light lists allocated");

    let tiles = 44 * 18;
    let index = resources.light_index_buffer.expect("index");
    let light_grid = resources.light_grid_buffer.expect("grid");
    assert_eq!(device.size_of(index), index_bytes(tiles, 256));
    assert_eq!(device.size_of(light_grid), grid_bytes(tiles));
    assert_eq!(resources.tile_capacity, tiles as u32);
}

#[test]
fn a_grid_the_light_lists_already_hold_creates_nothing() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();
    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080))
        .expect("first frame");
    let before = held(&resources);
    device.forget_events();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080))
        .expect("same grid");
    assert_eq!(device.events(), vec![], "the same grid reallocates nothing");
    assert_eq!(held(&resources), before);

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("smaller grid");
    assert_eq!(
        device.events(),
        vec![],
        "a shrinking target keeps its buffers"
    );
    assert_eq!(held(&resources), before);
    assert_eq!(resources.tile_capacity, 120 * 68);
}

#[test]
fn a_larger_grid_replaces_the_light_lists_and_frees_the_old_ones() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();
    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("first frame");
    let (old_index, old_grid, old_group, _) = held(&resources);
    let (old_index, old_grid, old_group) = (
        old_index.expect("index"),
        old_grid.expect("grid"),
        old_group.expect("bind group"),
    );
    device.forget_events();

    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080))
        .expect("larger grid");

    let tiles = 120 * 68;
    let new_index = resources.light_index_buffer.expect("index");
    let new_grid = resources.light_grid_buffer.expect("grid");
    let new_group = resources.culling_bind_group.expect("bind group");
    assert_eq!(device.created_buffers().len(), 2, "{:?}", device.events());
    assert_eq!(device.size_of(new_index), index_bytes(tiles, 128));
    assert_eq!(device.size_of(new_grid), grid_bytes(tiles));
    assert_eq!(device.created_bind_groups().len(), 1);
    assert_eq!(device.created_bind_groups()[0].0, new_group);
    assert_ne!(new_group, old_group);
    assert_eq!(resources.tile_capacity, tiles as u32);

    let mut destroyed = device.destroyed_buffers();
    destroyed.sort_by_key(|id| id.0);
    let mut expected = vec![old_index, old_grid];
    expected.sort_by_key(|id| id.0);
    assert_eq!(destroyed, expected, "exactly the old light lists are freed");
    assert_eq!(device.destroyed_bind_groups(), vec![old_group]);

    // The old resources are freed only once their replacements exist.
    let events = device.events();
    let last_create = events
        .iter()
        .rposition(|e| {
            matches!(
                e,
                Event::CreateBuffer { .. } | Event::CreateBindGroup { .. }
            )
        })
        .expect("creations");
    let first_destroy = events
        .iter()
        .position(|e| matches!(e, Event::DestroyBuffer(_) | Event::DestroyBindGroup(_)))
        .expect("destructions");
    assert!(last_create < first_destroy, "{events:?}");
}

#[test]
fn a_grid_without_tiles_creates_nothing() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();

    for (width, height) in [(0, 1080), (1920, 0), (0, 0)] {
        resources
            .ensure_tile_buffers(&device, &config, &grid(&config, width, height))
            .expect("a zero-sized target is not an error");
    }

    assert_eq!(device.events(), vec![], "never a zero-sized buffer");
    assert_eq!(held(&resources), (None, None, None, 0));
}

#[test]
fn a_failed_regrow_keeps_the_previous_light_lists() {
    let device = RecordingDevice::default();
    let config = x16();
    let mut resources = initialised_resources();
    resources
        .ensure_tile_buffers(&device, &config, &grid(&config, 1024, 768))
        .expect("first frame");
    let before = held(&resources);
    device.forget_events();
    device.refuse_creation.store(true, Ordering::Relaxed);

    let result = resources.ensure_tile_buffers(&device, &config, &grid(&config, 1920, 1080));

    assert!(result.is_err(), "a refused allocation is reported");
    assert_eq!(
        held(&resources),
        before,
        "the previous light lists stay bound"
    );
    let (old_index, old_grid, old_group, _) = before;
    let destroyed = device.destroyed_buffers();
    assert!(
        !destroyed.contains(&old_index.expect("index")),
        "{destroyed:?}"
    );
    assert!(
        !destroyed.contains(&old_grid.expect("grid")),
        "{destroyed:?}"
    );
    assert!(!device
        .destroyed_bind_groups()
        .contains(&old_group.expect("bind group")));
}

// --- The lane reads the target size from its context ---

#[test]
fn execute_without_the_colour_target_size_fails_naming_it() {
    let lane = ForwardPlusLane::new();
    let device: Arc<dyn GraphicsDevice> = Arc::new(RecordingDevice::default());
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
    let render_world = RenderWorld::default();
    let mut encoder: Box<dyn CommandEncoder> = Box::new(MockCommandEncoder);

    let mut ctx = LaneContext::new();
    ctx.insert(device);
    ctx.insert(gpu_meshes);
    ctx.insert_slot(encoder.as_mut());
    ctx.insert_ref(&render_world);
    ctx.insert(ColorTarget(TextureViewId(1)));
    ctx.insert(DepthTarget(TextureViewId(2)));
    ctx.insert(ClearColor(LinearRgba::BLACK));

    let err = lane
        .execute(&mut ctx)
        .expect_err("a frame without a ColorTargetSize has no tile grid");
    assert!(err.to_string().contains("ColorTargetSize"), "{err}");
}

/// The lane's cost estimate for an empty scene on a target of `target`
/// (`None`: no `ColorTargetSize` in the context).
fn cost_on(lane: &ForwardPlusLane, target: Option<Extent2D>) -> f32 {
    let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
    let render_world = RenderWorld::default();
    let mut ctx = LaneContext::new();
    ctx.insert(gpu_meshes);
    ctx.insert_ref(&render_world);
    if let Some(target) = target {
        ctx.insert(ColorTargetSize(target));
    }
    lane.estimate_cost(&ctx)
}

#[test]
fn the_cost_estimate_counts_the_tiles_of_the_colour_target() {
    let lane = ForwardPlusLane::new();
    let large = cost_on(
        &lane,
        Some(Extent2D {
            width: 3840,
            height: 2160,
        }),
    );
    let small = cost_on(
        &lane,
        Some(Extent2D {
            width: 640,
            height: 480,
        }),
    );
    assert!(
        large > small,
        "a 4K target has more tiles to cull than a 640x480 one ({large} vs {small})"
    );

    // Without a frame (the negotiation thread), the cost model assumes 1080p.
    let reference = cost_on(
        &lane,
        Some(Extent2D {
            width: 1920,
            height: 1080,
        }),
    );
    assert_eq!(cost_on(&lane, None), reference);
}
