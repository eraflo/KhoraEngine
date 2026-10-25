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

//! The UI projection follows the colour target the lane draws into.

use super::super::{ui_projection, UiRenderLane};
use khora_core::lane::{ColorTarget, ColorTargetSize, Lane, LaneContext};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, Mat4, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::{
    GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
};
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
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_data::ui::{UiAtlasMap, UiScene};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// --- Test doubles ---

/// A device that hands out unique ids and records buffers and writes.
#[derive(Debug, Default)]
struct RecordingDevice {
    next_id: AtomicUsize,
    /// Every buffer created, with its label.
    buffers: Mutex<Vec<(BufferId, String)>>,
    /// Every write: target buffer and bytes.
    writes: Mutex<Vec<(BufferId, Vec<u8>)>>,
}

impl RecordingDevice {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn projection_buffer(&self) -> BufferId {
        self.buffers
            .lock()
            .expect("buffers lock")
            .iter()
            .find(|(_, label)| label.contains("Projection"))
            .map(|(id, _)| *id)
            .expect("the UI lane creates its projection buffer at initialisation")
    }

    fn writes_to(&self, buffer: BufferId) -> Vec<Vec<u8>> {
        self.writes
            .lock()
            .expect("writes lock")
            .iter()
            .filter(|(id, _)| *id == buffer)
            .map(|(_, bytes)| bytes.clone())
            .collect()
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
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        Ok(BindGroupId(self.next()))
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        let id = BufferId(self.next());
        let label = d.label.as_deref().unwrap_or_default().to_string();
        self.buffers.lock().expect("buffers lock").push((id, label));
        Ok(id)
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.create_buffer(d)
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
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

/// A pipeline system that answers every request with a fresh id.
#[derive(Debug, Default)]
struct FakePipelines {
    next_id: AtomicUsize,
}

impl FakePipelines {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 10_000
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

// --- Helpers ---

fn extent(width: u32, height: u32) -> Extent2D {
    Extent2D { width, height }
}

/// Where `projection` sends the UI pixel `(x, y)`, in normalised device coordinates.
fn ndc(projection: &Mat4, x: f32, y: f32) -> (f32, f32) {
    let clip = *projection * Vec4::new(x, y, 0.0, 1.0);
    (clip.x / clip.w, clip.y / clip.w)
}

fn assert_near(actual: (f32, f32), expected: (f32, f32), what: &str) {
    let close = (actual.0 - expected.0).abs() < 1e-5 && (actual.1 - expected.1).abs() < 1e-5;
    assert!(close, "{what}: got {actual:?}, expected {expected:?}");
}

/// Asserts `projection` maps the corners of a `target` UI to the NDC corners:
/// top-left pixel to (-1, 1), bottom-right to (1, -1), centre to the origin.
fn assert_maps_target_to_ndc(projection: &Mat4, target: Extent2D) {
    let (w, h) = (target.width as f32, target.height as f32);
    assert_near(ndc(projection, 0.0, 0.0), (-1.0, 1.0), "top-left pixel");
    assert_near(ndc(projection, w, h), (1.0, -1.0), "bottom-right pixel");
    assert_near(ndc(projection, w, 0.0), (1.0, 1.0), "top-right pixel");
    assert_near(
        ndc(projection, w * 0.5, h * 0.5),
        (0.0, 0.0),
        "centre pixel",
    );
}

/// An initialised UI lane, and the device it allocated from.
fn initialised_lane() -> (UiRenderLane, Arc<RecordingDevice>) {
    let lane = UiRenderLane::new();
    let device = Arc::new(RecordingDevice::default());
    let pipelines: Arc<dyn PipelineSystem> = Arc::new(FakePipelines::default());
    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert(pipelines);
    lane.on_initialize(&mut ctx)
        .expect("the UI lane initialises on a recording device");
    (lane, device)
}

/// Runs one UI frame of an empty scene; `target` is the `ColorTargetSize`
/// handed to the lane (`None`: the context has none).
fn run_frame(
    lane: &UiRenderLane,
    device: &Arc<RecordingDevice>,
    target: Option<Extent2D>,
) -> Result<(), khora_core::lane::LaneError> {
    let scene = UiScene::default();
    let atlas_map = UiAtlasMap::default();
    let mut encoder: Box<dyn CommandEncoder> = Box::new(MockCommandEncoder);

    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert_ref(&scene);
    ctx.insert_ref(&atlas_map);
    ctx.insert_slot(encoder.as_mut());
    ctx.insert(ColorTarget(TextureViewId(1)));
    if let Some(target) = target {
        ctx.insert(ColorTargetSize(target));
    }
    lane.execute(&mut ctx)
}

// --- ui_projection ---

#[test]
fn the_ui_projection_maps_a_1024_by_768_target_onto_clip_space() {
    let target = extent(1024, 768);
    let projection = ui_projection(target).expect("a target with area has a projection");
    assert_maps_target_to_ndc(&projection, target);
}

#[test]
fn the_ui_projection_maps_a_1384_by_561_target_onto_clip_space() {
    let target = extent(1384, 561);
    let projection = ui_projection(target).expect("a target with area has a projection");
    assert_maps_target_to_ndc(&projection, target);
}

#[test]
fn a_target_with_a_zero_side_has_no_ui_projection() {
    for target in [extent(0, 768), extent(1024, 0), extent(0, 0)] {
        assert!(
            ui_projection(target).is_none(),
            "{target:?} has no area to project onto"
        );
    }
}

// --- UiRenderLane::execute ---

#[test]
fn execute_without_the_colour_target_size_fails_naming_it() {
    let lane = UiRenderLane::new();
    let device = Arc::new(RecordingDevice::default());

    let err = run_frame(&lane, &device, None)
        .expect_err("a frame without a ColorTargetSize has nothing to project onto");
    assert!(err.to_string().contains("ColorTargetSize"), "{err}");
}

#[test]
fn execute_uploads_the_projection_of_the_colour_target() {
    let (lane, device) = initialised_lane();
    let target = extent(1384, 561);

    run_frame(&lane, &device, Some(target)).expect("a UI frame on a sized target");

    let writes = device.writes_to(device.projection_buffer());
    let bytes = writes.last().expect("the projection is uploaded");
    assert_eq!(bytes.len(), std::mem::size_of::<Mat4>());
    let projection: Mat4 = bytemuck::pod_read_unaligned(bytes);
    assert_maps_target_to_ndc(&projection, target);
}

#[test]
fn execute_on_a_target_with_a_zero_side_uploads_nothing() {
    let (lane, device) = initialised_lane();

    run_frame(&lane, &device, Some(extent(0, 561))).expect("a minimised target is not an error");

    assert!(
        device.writes_to(device.projection_buffer()).is_empty(),
        "no projection exists for a target without area"
    );
}
