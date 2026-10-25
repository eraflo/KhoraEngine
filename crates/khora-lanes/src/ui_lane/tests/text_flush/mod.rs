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

//! The UI lane hands its frame's text to the text renderer, with the size of
//! the colour target it draws into, whether or not the frame has UI quads.

use crate::ui_lane::UiRenderLane;
use khora_core::lane::{ColorTarget, ColorTargetSize, Lane, LaneContext};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, Vec2, Vec4};
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
use khora_core::renderer::api::text::{FontFace, TextLayout, TextRenderer};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_data::ui::{ExtractedUiText, UiAtlasMap, UiScene};
use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// --- Test doubles ---

/// A device that hands out unique ids and accepts every call.
#[derive(Debug, Default)]
struct QuietDevice {
    next_id: AtomicUsize,
}

impl QuietDevice {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
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

impl GraphicsDevice for QuietDevice {
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
    fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        Ok(BufferId(self.next()))
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
            name: "QuietDevice".to_string(),
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

/// A laid-out text of a fixed size.
struct FixedLayout;

impl TextLayout for FixedLayout {
    fn size(&self) -> Vec2 {
        Vec2::new(40.0, 16.0)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A text renderer that records what the lane queues and flushes.
#[derive(Default)]
struct RecordingTextRenderer {
    /// Every queued text, by position.
    queued: Mutex<Vec<Vec2>>,
    /// Every flush: the colour target and its size.
    flushes: Mutex<Vec<(TextureViewId, Extent2D)>>,
}

impl TextRenderer for RecordingTextRenderer {
    fn layout_text(
        &self,
        _text: &str,
        _face: FontFace<'_>,
        _font_size: f32,
        _max_width: Option<f32>,
    ) -> Box<dyn TextLayout> {
        Box::new(FixedLayout)
    }

    fn queue_text(&self, _layout: &dyn TextLayout, pos: Vec2, _color: Vec4, _z_index: i32) {
        self.queued.lock().expect("queued lock").push(pos);
    }

    fn flush(
        &self,
        _device: &dyn GraphicsDevice,
        _encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        target: Extent2D,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.flushes
            .lock()
            .expect("flushes lock")
            .push((*color_target, target));
        Ok(())
    }
}

// --- Helpers ---

const COLOR_TARGET: TextureViewId = TextureViewId(1);

fn extent(width: u32, height: u32) -> Extent2D {
    Extent2D { width, height }
}

/// An initialised UI lane, and the device it allocated from.
fn initialised_lane() -> (UiRenderLane, Arc<QuietDevice>) {
    let lane = UiRenderLane::new();
    let device = Arc::new(QuietDevice::default());
    let pipelines: Arc<dyn PipelineSystem> = Arc::new(FakePipelines::default());
    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert(pipelines);
    lane.on_initialize(&mut ctx)
        .expect("the UI lane initialises on a quiet device");
    (lane, device)
}

/// A text the flow laid out, at `pos`.
fn text_at(pos: Vec2) -> ExtractedUiText {
    ExtractedUiText {
        pos,
        layout: Arc::new(FixedLayout),
        color: Vec4::ONE,
        z_index: 0,
    }
}

/// Runs one UI frame of `scene` on a `target`-sized colour target, with
/// `text` as the frame's text renderer.
fn run_frame(
    lane: &UiRenderLane,
    device: &Arc<QuietDevice>,
    scene: &UiScene,
    text: &Arc<RecordingTextRenderer>,
    target: Extent2D,
) {
    let atlas_map = UiAtlasMap::default();
    let mut encoder: Box<dyn CommandEncoder> = Box::new(MockCommandEncoder);

    let mut ctx = LaneContext::new();
    ctx.insert(device.clone() as Arc<dyn GraphicsDevice>);
    ctx.insert(text.clone() as Arc<dyn TextRenderer>);
    ctx.insert_ref(scene);
    ctx.insert_ref(&atlas_map);
    ctx.insert_slot(encoder.as_mut());
    ctx.insert(ColorTarget(COLOR_TARGET));
    ctx.insert(ColorTargetSize(target));
    lane.execute(&mut ctx)
        .expect("a UI frame on a sized target succeeds");
}

// --- Tests ---

#[test]
fn a_frame_with_text_and_no_ui_node_still_flushes_its_text() {
    let (lane, device) = initialised_lane();
    let text = Arc::new(RecordingTextRenderer::default());
    let scene = UiScene {
        nodes: Vec::new(),
        texts: vec![text_at(Vec2::new(12.0, 34.0))],
    };
    let target = extent(1384, 561);

    run_frame(&lane, &device, &scene, &text, target);

    assert_eq!(
        *text.queued.lock().expect("queued lock"),
        [Vec2::new(12.0, 34.0)],
        "the frame's text is queued"
    );
    assert_eq!(
        *text.flushes.lock().expect("flushes lock"),
        [(COLOR_TARGET, target)],
        "the text is flushed into the frame's colour target, at its size"
    );
}

#[test]
fn an_uninitialised_lane_still_flushes_a_text_only_frame() {
    let lane = UiRenderLane::new();
    let device = Arc::new(QuietDevice::default());
    let text = Arc::new(RecordingTextRenderer::default());
    let scene = UiScene {
        nodes: Vec::new(),
        texts: vec![text_at(Vec2::new(1.0, 2.0))],
    };
    let target = extent(640, 480);

    run_frame(&lane, &device, &scene, &text, target);

    assert_eq!(
        *text.flushes.lock().expect("flushes lock"),
        [(COLOR_TARGET, target)],
        "text does not wait for the quad pipeline"
    );
}

mod text_with_quads;
