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

//! `StandardTextRenderer` draws its queued text into the colour target it is
//! handed, at that target's size, through a GPU-free recording device.

use crate::graphics::shader::TEXT_WGSL;
use crate::graphics::text::standard::{text_projection, StandardTextRenderer};
use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, Mat4, Vec2, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, BindingType,
    BufferBindingType, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::{
    GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
};
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::texture::{
    SamplerDescriptor, SamplerId, TextureDescriptor, TextureId, TextureViewDescriptor,
    TextureViewId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, BufferUsage, IndexFormat, TextureFormat,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::api::text::{FontFace, TextRenderer};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};
use khora_core::renderer::GraphicsDevice;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// --- Test doubles ---

/// What a bind-group layout entry binds, as far as the shader can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Uniform,
    Storage,
    Texture,
    Sampler,
}

/// A bind-group layout and its `(binding, slot)` entries.
type RecordedLayout = (BindGroupLayoutId, Vec<(u32, Slot)>);

/// A buffer the device created.
#[derive(Debug, Clone)]
struct CreatedBuffer {
    id: BufferId,
    size: u64,
    usage: BufferUsage,
}

/// A device that hands out unique ids and records what it is asked to do.
#[derive(Debug, Default)]
struct RecordingDevice {
    next_id: AtomicUsize,
    buffers: Mutex<Vec<CreatedBuffer>>,
    destroyed_buffers: Mutex<Vec<BufferId>>,
    /// Every buffer write: target, offset, bytes.
    writes: Mutex<Vec<(BufferId, u64, Vec<u8>)>>,
    /// Every bind-group layout, with its `(binding, slot)` entries.
    layouts: Mutex<Vec<RecordedLayout>>,
    /// Every pipeline layout, with its bind-group layouts by set index.
    pipeline_layouts: Mutex<Vec<(PipelineLayoutId, Vec<BindGroupLayoutId>)>>,
    shader_modules: AtomicUsize,
    render_pipelines: AtomicUsize,
    textures: AtomicUsize,
    bind_groups: AtomicUsize,
    samplers: AtomicUsize,
}

impl RecordingDevice {
    fn next(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn buffers(&self) -> Vec<CreatedBuffer> {
        self.buffers.lock().expect("buffers lock").clone()
    }

    fn buffer(&self, id: BufferId) -> CreatedBuffer {
        self.buffers()
            .into_iter()
            .find(|b| b.id == id)
            .expect("a buffer this device created")
    }

    fn destroyed(&self) -> Vec<BufferId> {
        self.destroyed_buffers
            .lock()
            .expect("destroyed lock")
            .clone()
    }

    fn writes(&self) -> Vec<(BufferId, u64, Vec<u8>)> {
        self.writes.lock().expect("writes lock").clone()
    }

    fn writes_to(&self, id: BufferId) -> Vec<Vec<u8>> {
        self.writes()
            .into_iter()
            .filter(|(buffer, _, _)| *buffer == id)
            .map(|(_, _, bytes)| bytes)
            .collect()
    }

    /// The renderer's uniform buffer: the one buffer created for uniforms.
    fn uniform_buffer(&self) -> BufferId {
        let uniforms: Vec<CreatedBuffer> = self
            .buffers()
            .into_iter()
            .filter(|b| b.usage.contains(BufferUsage::UNIFORM))
            .collect();
        assert_eq!(
            uniforms.len(),
            1,
            "the text renderer owns exactly one uniform buffer: {uniforms:?}"
        );
        uniforms[0].id
    }

    /// The number of GPU objects of any kind this device created.
    fn creations(&self) -> usize {
        self.buffers().len()
            + self.layouts.lock().expect("layouts lock").len()
            + self
                .pipeline_layouts
                .lock()
                .expect("pipeline layouts lock")
                .len()
            + self.shader_modules.load(Ordering::Relaxed)
            + self.render_pipelines.load(Ordering::Relaxed)
            + self.textures.load(Ordering::Relaxed)
            + self.bind_groups.load(Ordering::Relaxed)
            + self.samplers.load(Ordering::Relaxed)
    }

    /// Every `(group, binding, slot)` the pipeline layout declares.
    fn declared_bindings(&self) -> BTreeSet<(u32, u32, Slot)> {
        let pipeline_layouts = self
            .pipeline_layouts
            .lock()
            .expect("pipeline layouts lock")
            .clone();
        assert_eq!(
            pipeline_layouts.len(),
            1,
            "the text renderer builds one pipeline layout"
        );
        let layouts = self.layouts.lock().expect("layouts lock").clone();
        let mut declared = BTreeSet::new();
        for (group, layout) in pipeline_layouts[0].1.iter().enumerate() {
            let (_, entries) = layouts
                .iter()
                .find(|(id, _)| id == layout)
                .expect("the pipeline layout names layouts of this device");
            for (binding, slot) in entries {
                declared.insert((group as u32, *binding, *slot));
            }
        }
        declared
    }
}

/// One render pass the encoder began, and what was recorded into it.
#[derive(Debug, Clone, Default)]
struct PassRecord {
    vertex_buffer: Option<BufferId>,
    index_buffer: Option<(BufferId, IndexFormat)>,
    draws_indexed: Vec<Range<u32>>,
}

type Passes = Arc<Mutex<Vec<PassRecord>>>;

struct RecordingEncoder {
    passes: Passes,
}

struct RecordingRenderPass {
    passes: Passes,
    index: usize,
}

impl RecordingRenderPass {
    fn record(&self, f: impl FnOnce(&mut PassRecord)) {
        f(&mut self.passes.lock().expect("passes lock")[self.index]);
    }
}

struct MockComputePass;

impl RenderPass<'_> for RecordingRenderPass {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, b: &BufferId, _o: u64) {
        let b = *b;
        self.record(|pass| pass.vertex_buffer = Some(b));
    }
    fn set_index_buffer(&mut self, b: &BufferId, _o: u64, f: IndexFormat) {
        let b = *b;
        self.record(|pass| pass.index_buffer = Some((b, f)));
    }
    fn draw(&mut self, _v: Range<u32>, _i: Range<u32>) {}
    fn draw_indexed(&mut self, idx: Range<u32>, _bv: i32, _i: Range<u32>) {
        self.record(|pass| pass.draws_indexed.push(idx));
    }
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

impl ComputePass<'_> for MockComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for RecordingEncoder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        let index = {
            let mut passes = self.passes.lock().expect("passes lock");
            passes.push(PassRecord::default());
            passes.len() - 1
        };
        Box::new(RecordingRenderPass {
            passes: self.passes.clone(),
            index,
        })
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
        self.shader_modules.fetch_add(1, Ordering::Relaxed);
        Ok(ShaderModuleId(self.next()))
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_render_pipeline(
        &self,
        _d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        self.render_pipelines.fetch_add(1, Ordering::Relaxed);
        Ok(RenderPipelineId(self.next()))
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        let id = PipelineLayoutId(self.next());
        self.pipeline_layouts
            .lock()
            .expect("pipeline layouts lock")
            .push((id, d.bind_group_layouts.to_vec()));
        Ok(id)
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
        d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        let id = BindGroupLayoutId(self.next());
        let entries = d
            .entries
            .iter()
            .map(|entry| {
                let slot = match &entry.ty {
                    BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        ..
                    } => Slot::Uniform,
                    BindingType::Buffer { .. } => Slot::Storage,
                    BindingType::Texture { .. } => Slot::Texture,
                    BindingType::Sampler(_) => Slot::Sampler,
                };
                (entry.binding, slot)
            })
            .collect();
        self.layouts
            .lock()
            .expect("layouts lock")
            .push((id, entries));
        Ok(id)
    }
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.bind_groups.fetch_add(1, Ordering::Relaxed);
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
        self.buffers
            .lock()
            .expect("buffers lock")
            .push(CreatedBuffer {
                id,
                size: d.size,
                usage: d.usage,
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
        self.destroyed_buffers
            .lock()
            .expect("destroyed lock")
            .push(id);
        Ok(())
    }
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        self.writes
            .lock()
            .expect("writes lock")
            .push((id, offset, data.to_vec()));
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
        self.textures.fetch_add(1, Ordering::Relaxed);
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
        self.samplers.fetch_add(1, Ordering::Relaxed);
        Ok(SamplerId(self.next()))
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        Box::new(RecordingEncoder {
            passes: Passes::default(),
        })
    }
    fn submit_command_buffer(&self, _cb: CommandBufferId) {}
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(TextureFormat::Rgba8UnormSrgb)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        // Deliberately unlike every target below: the renderer must not read it.
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

// --- Helpers ---

/// The colour target every flush below draws into.
const COLOR_TARGET: TextureViewId = TextureViewId(7);

fn extent(width: u32, height: u32) -> Extent2D {
    Extent2D { width, height }
}

/// Where `projection` sends the text pixel `(x, y)`, in normalised device coordinates.
fn ndc(projection: &Mat4, x: f32, y: f32) -> (f32, f32) {
    let clip = *projection * Vec4::new(x, y, 0.0, 1.0);
    (clip.x / clip.w, clip.y / clip.w)
}

fn assert_near(actual: (f32, f32), expected: (f32, f32), what: &str) {
    let close = (actual.0 - expected.0).abs() < 1e-5 && (actual.1 - expected.1).abs() < 1e-5;
    assert!(close, "{what}: got {actual:?}, expected {expected:?}");
}

/// Asserts `projection` maps the pixels of `target` onto clip space: the
/// top-left pixel to (-1, 1), the bottom-right to (1, -1), the centre to the
/// origin.
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

fn renderer() -> StandardTextRenderer {
    StandardTextRenderer::new(TEXT_WGSL.to_owned())
}

/// A loaded font asset the tests lay text out with.
fn font() -> (AssetUUID, AssetHandle<Font>) {
    (
        AssetUUID::new(),
        AssetHandle::new(Font {
            name: "Test".to_owned(),
            data: Vec::new(),
        }),
    )
}

/// Lays `text` out and queues it at `pos`.
fn queue(renderer: &StandardTextRenderer, text: &str, pos: Vec2) {
    let (uuid, font) = font();
    let layout = renderer.layout_text(text, FontFace::Asset { uuid, font: &font }, 16.0, None);
    renderer.queue_text(layout.as_ref(), pos, Vec4::ONE, 0);
}

/// Flushes `renderer` into [`COLOR_TARGET`] at `target` and returns the
/// passes the encoder recorded.
fn flush(
    renderer: &StandardTextRenderer,
    device: &RecordingDevice,
    target: Extent2D,
) -> Vec<PassRecord> {
    let passes = Passes::default();
    let mut encoder = RecordingEncoder {
        passes: passes.clone(),
    };
    renderer
        .flush(device, &mut encoder, &COLOR_TARGET, target)
        .expect("a flush on a recording device succeeds");
    let recorded = passes.lock().expect("passes lock").clone();
    recorded
}

/// The number of indices the recorded passes draw.
fn indices_drawn(passes: &[PassRecord]) -> u32 {
    passes
        .iter()
        .flat_map(|pass| pass.draws_indexed.iter())
        .map(|range| range.end - range.start)
        .sum()
}

/// The last projection written to the renderer's uniform buffer.
fn last_projection(device: &RecordingDevice) -> Mat4 {
    let writes = device.writes_to(device.uniform_buffer());
    let bytes = writes.last().expect("the projection is uploaded");
    assert_eq!(bytes.len(), std::mem::size_of::<Mat4>());
    bytemuck::pod_read_unaligned(bytes)
}

/// A string of `n` glyphs, none of them blank.
fn glyphs(n: usize) -> String {
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        .chars()
        .cycle()
        .take(n)
        .collect()
}

/// Every `(group, binding, slot)` `TEXT_WGSL` declares.
fn shader_bindings() -> BTreeSet<(u32, u32, Slot)> {
    let module = naga::front::wgsl::parse_str(TEXT_WGSL)
        .unwrap_or_else(|e| panic!("TEXT_WGSL does not parse: {e}"));
    module
        .global_variables
        .iter()
        .filter_map(|(_, var)| {
            let binding = var.binding.as_ref()?;
            let slot = match module.types[var.ty].inner {
                naga::TypeInner::Image { .. } => Slot::Texture,
                naga::TypeInner::Sampler { .. } => Slot::Sampler,
                _ => match var.space {
                    naga::AddressSpace::Uniform => Slot::Uniform,
                    _ => Slot::Storage,
                },
            };
            Some((binding.group, binding.binding, slot))
        })
        .collect()
}

// --- text_projection ---

#[test]
fn the_text_projection_maps_a_1024_by_768_target_onto_clip_space() {
    let target = extent(1024, 768);
    let projection = text_projection(target).expect("a target with area has a projection");
    assert_maps_target_to_ndc(&projection, target);
}

#[test]
fn the_text_projection_maps_a_1384_by_561_target_onto_clip_space() {
    let target = extent(1384, 561);
    let projection = text_projection(target).expect("a target with area has a projection");
    assert_maps_target_to_ndc(&projection, target);
}

#[test]
fn a_target_with_a_zero_side_has_no_text_projection() {
    for target in [extent(0, 768), extent(1024, 0), extent(0, 0)] {
        assert!(
            text_projection(target).is_none(),
            "{target:?} has no area to project onto"
        );
    }
}

#[test]
fn the_text_projection_changes_with_the_target_size() {
    let small = text_projection(extent(800, 600)).expect("a target with area");
    let large = text_projection(extent(1920, 1080)).expect("a target with area");
    assert_ne!(
        small, large,
        "two target sizes put the same pixel at two clip positions"
    );
}

// --- the shader and the layout ---

#[test]
fn the_text_shader_binds_its_uniform_texture_and_sampler_in_group_zero() {
    let expected: BTreeSet<(u32, u32, Slot)> = [
        (0, 0, Slot::Uniform),
        (0, 1, Slot::Texture),
        (0, 2, Slot::Sampler),
    ]
    .into_iter()
    .collect();
    assert_eq!(shader_bindings(), expected);
}

#[test]
fn the_text_shader_declares_exactly_what_the_pipeline_layout_binds() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, "Hi", Vec2::new(4.0, 4.0));

    flush(&renderer, &device, extent(800, 600));

    assert_eq!(
        shader_bindings(),
        device.declared_bindings(),
        "every binding of the shader is in the layout at the same group and slot, and nothing else"
    );
}

// --- flush ---

#[test]
fn flush_writes_the_projection_of_its_target_to_the_text_uniforms() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    let target = extent(1384, 561);
    queue(&renderer, "Hello", Vec2::new(10.0, 20.0));

    flush(&renderer, &device, target);

    assert_maps_target_to_ndc(&last_projection(&device), target);
}

#[test]
fn each_flush_writes_the_projection_of_its_own_target() {
    let renderer = renderer();
    let device = RecordingDevice::default();

    let first = extent(800, 600);
    queue(&renderer, "Before", Vec2::new(10.0, 20.0));
    flush(&renderer, &device, first);
    let first_projection = last_projection(&device);

    let second = extent(1920, 1080);
    queue(&renderer, "After", Vec2::new(10.0, 20.0));
    flush(&renderer, &device, second);
    let second_projection = last_projection(&device);

    assert_maps_target_to_ndc(&first_projection, first);
    assert_maps_target_to_ndc(&second_projection, second);
}

#[test]
fn flush_draws_u32_indices_with_the_uint32_format() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, "Hi", Vec2::new(4.0, 4.0));

    let passes = flush(&renderer, &device, extent(800, 600));

    let (index_buffer, format) = passes
        .iter()
        .find_map(|pass| pass.index_buffer)
        .expect("the text pass binds an index buffer");
    assert_eq!(format, IndexFormat::Uint32);
    assert_eq!(indices_drawn(&passes), 12, "two glyphs, two triangles each");

    let bytes = device
        .writes_to(index_buffer)
        .last()
        .cloned()
        .expect("the indices are uploaded");
    assert_eq!(bytes.len(), 12 * 4, "twelve indices of four bytes");
    let indices: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
}

#[test]
fn flush_never_writes_past_the_end_of_a_buffer() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    // Far more glyphs than a 64 KiB vertex buffer holds (128 bytes a glyph).
    let count = 2_000;
    queue(&renderer, &glyphs(count), Vec2::new(0.0, 0.0));

    let passes = flush(&renderer, &device, extent(1920, 1080));

    for (buffer, offset, bytes) in device.writes() {
        let size = device.buffer(buffer).size;
        assert!(
            offset + bytes.len() as u64 <= size,
            "a write of {} bytes at {offset} into a buffer of {size} bytes",
            bytes.len()
        );
    }
    assert_eq!(
        indices_drawn(&passes),
        (count * 6) as u32,
        "every queued glyph is drawn"
    );
}

#[test]
fn a_flush_that_outgrows_its_buffers_binds_bigger_ones_and_destroys_the_old() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    flush(&renderer, &device, extent(1920, 1080));

    let count = 2_000;
    queue(&renderer, &glyphs(count), Vec2::new(0.0, 0.0));
    let passes = flush(&renderer, &device, extent(1920, 1080));

    let pass = passes.last().expect("the large flush draws");
    let vertices = pass.vertex_buffer.expect("the pass binds a vertex buffer");
    let (indices, _) = pass.index_buffer.expect("the pass binds an index buffer");
    let vertex_bytes = device
        .writes_to(vertices)
        .last()
        .map(Vec::len)
        .expect("the vertices are uploaded");
    let index_bytes = device
        .writes_to(indices)
        .last()
        .map(Vec::len)
        .expect("the indices are uploaded");
    assert!(device.buffer(vertices).size >= vertex_bytes as u64);
    assert!(device.buffer(indices).size >= index_bytes as u64);

    let destroyed = device.destroyed();
    for buffer in device.buffers() {
        let replaced = (buffer.usage.contains(BufferUsage::VERTEX) && buffer.id != vertices)
            || (buffer.usage.contains(BufferUsage::INDEX) && buffer.id != indices);
        if replaced {
            assert!(
                destroyed.contains(&buffer.id),
                "the outgrown buffer {:?} is destroyed",
                buffer.id
            );
        }
    }
}

#[test]
fn a_flush_with_nothing_queued_creates_nothing_and_begins_no_pass() {
    let renderer = renderer();
    let device = RecordingDevice::default();

    let passes = flush(&renderer, &device, extent(800, 600));

    assert_eq!(device.creations(), 0, "no GPU object for an empty queue");
    assert!(device.writes().is_empty(), "no upload for an empty queue");
    assert!(passes.is_empty(), "no render pass for an empty queue");
}

#[test]
fn a_flush_on_a_target_without_area_draws_nothing_and_drops_the_queue() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    queue(&renderer, "Stale", Vec2::new(0.0, 0.0));

    let passes = flush(&renderer, &device, extent(0, 600));
    assert!(passes.is_empty(), "a target without area gets no pass");

    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    let passes = flush(&renderer, &device, extent(800, 600));
    assert_eq!(
        indices_drawn(&passes),
        12,
        "only the text queued after the empty target is drawn"
    );
}

// --- layout_text ---

#[test]
fn layout_text_lays_out_with_the_built_in_face_and_with_a_font_asset() {
    let renderer = renderer();
    let (uuid, font) = font();

    let built_in = renderer.layout_text("Hello", FontFace::BuiltIn, 16.0, None);
    let asset = renderer.layout_text("Hello", FontFace::Asset { uuid, font: &font }, 16.0, None);

    for (face, layout) in [("built-in", &built_in), ("asset", &asset)] {
        let size = layout.size();
        assert!(
            size.x > 0.0 && size.y > 0.0,
            "the {face} face lays five glyphs out on a non-empty box: {size:?}"
        );
    }
}

mod failed_flush;
mod glyph_atlas;
mod glyph_buffers;
