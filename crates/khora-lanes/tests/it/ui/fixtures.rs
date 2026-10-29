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

//! The device, scenes and frames the UI lane's tests share.
//!
//! The device is a ledger: it hands out ids, and keeps every object it made
//! (with a buffer's size), what each bind group binds, every object it was
//! told to destroy, every buffer write with its offset, every encoder it
//! opened and finished, every render pass recorded through one, and every
//! buffer submitted. It can be told to refuse the n-th object it is asked to
//! make, so a test can stop a build at each of its steps. The
//! `PipelineSystem` hands out ids of its own, at or above
//! [`PIPELINE_SYSTEM_IDS`], keeps every pipeline spec it was asked for, and
//! can refuse every pipeline of one shader.
//!
//! The UI image atlas is made on a ledger of its own, counting from
//! [`ATLAS_IDS`], so the lane's ledger holds the lane's objects alone and two
//! atlases never share a view.
//!
//! The text renderer is a recorder: it keeps every text queued, and every
//! flush — the encoder it was handed (by the command buffer that encoder
//! finishes into), the colour target and its size, the texts queued since
//! the flush before, and how many render passes the lane's ledger had
//! recorded by then. It can be told to fail its flushes.
//!
//! A frame is what the engine hands the UI lane: the `UiScene` on the bus,
//! stamped for the frame, the frame's `Target`, a runtime holding the image
//! atlas among its resources and the text renderer among its backends (where
//! the engine installs it), and an `OutputSink` whose pass opens on the
//! ledger.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::AssetUUID;
use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::guard::InputRefusal;
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, Vec2, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, BindingResource, CommandBufferId, ComputePassDescriptor, LoadOp,
    RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::state::BlendStateDescriptor;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SampleCount, SamplerDescriptor, SamplerId,
    TextureDescriptor, TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::api::text::{FontFace, TextLayout, TextRenderer};
use khora_core::renderer::api::util::AtlasRect;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::types::UiRect;
use khora_core::Runtime;
use khora_data::render::{PassDescriptor, ResourceId, UiPassSlot};
use khora_data::ui::{
    ExtractedUiNode, ExtractedUiText, UiBorder, UiColor, UiImage, UiImageAtlas, UiScene,
};
use khora_lanes::ui_lane::UiLane;

/// The UI lane's one level.
pub const LEVEL: usize = 0;

/// The shader the UI pipeline is composed from.
pub const UI_SHADER: &str = "khora::pipelines::ui";

/// The format the ledger says its surface has: never the format of a test's
/// target, so a lane building for the surface instead of the target shows.
pub const SURFACE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

/// The frame's colour attachment, made by the renderer, not the lane.
pub const COLOR_VIEW: TextureViewId = TextureViewId(9_000_001);
/// The frame's depth attachment, made by the renderer, not the lane.
pub const DEPTH_VIEW: TextureViewId = TextureViewId(9_000_002);

/// First id the [`LedgerPipelines`] hands out. The device counts from 1, so
/// an id at or above this one belongs to the `PipelineSystem`.
pub const PIPELINE_SYSTEM_IDS: usize = 1_000_000;

/// First id of the image atlases' own ledgers: never an id the lane's
/// ledger or the `PipelineSystem` hands out.
pub const ATLAS_IDS: usize = 7_000_000;

/// The bytes one UI node takes in the instance storage: the shader's
/// `UiInstance` — position, size, colour, params, uv min, uv max — sixteen
/// floats.
pub const INSTANCE_BYTES: u64 = 64;

/// The nodes the instance storage held before 3i-a, when it stopped
/// silently past them.
pub const OLD_CAPACITY: usize = 1024;

// ─── The ledger ─────────────────────────────────────────────────────────────

/// The kind of device object the ledger hands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GpuKind {
    Buffer,
    Texture,
    TextureView,
    Sampler,
    BindGroup,
    BindGroupLayout,
    ShaderModule,
    RenderPipeline,
    PipelineLayout,
    ComputePipeline,
}

/// One object the device made.
#[derive(Debug, Clone)]
pub struct Made {
    pub kind: GpuKind,
    pub id: usize,
    pub label: String,
    /// A buffer's size in bytes; `None` for any other object.
    pub size: Option<u64>,
}

/// What one binding of a bind group binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Buffer(BufferId),
    View(TextureViewId),
    Sampler(SamplerId),
}

/// One `set_bind_group` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetBindGroup {
    pub group: u32,
    pub bind_group: BindGroupId,
    pub offsets: Vec<u32>,
}

/// One render pass recorded through an encoder of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPass {
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// Its colour attachments.
    pub colors: Vec<TextureViewId>,
    /// Whether every colour attachment loads what the target held.
    pub loads_color: bool,
    /// Its depth attachment, if it had one.
    pub depth: Option<TextureViewId>,
    /// The pipelines set, in order.
    pub pipelines: Vec<RenderPipelineId>,
    /// The bind groups set, in order.
    pub bind_groups: Vec<SetBindGroup>,
    /// The vertex buffers set, in order.
    pub vertex_buffers: Vec<BufferId>,
    /// The non-indexed draws, as `(vertices, instances)`.
    pub draws: Vec<(Range<u32>, Range<u32>)>,
    /// The indexed draws, as `(indices, instances)`.
    pub indexed_draws: Vec<(Range<u32>, Range<u32>)>,
}

impl RecordedPass {
    /// The bind group set last at `group`.
    pub fn bind_group(&self, group: u32) -> Option<BindGroupId> {
        self.bind_groups
            .iter()
            .rev()
            .find(|set| set.group == group)
            .map(|set| set.bind_group)
    }
}

/// One buffer write.
#[derive(Debug, Clone)]
pub struct Write {
    pub buffer: BufferId,
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct Log {
    made: Vec<Made>,
    bound: HashMap<usize, Vec<(u32, Bound)>>,
    destroyed: Vec<(GpuKind, usize)>,
    writes: Vec<Write>,
    opened: Vec<CommandBufferId>,
    finished: Vec<CommandBufferId>,
    passes: Vec<RecordedPass>,
    submitted: Vec<CommandBufferId>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A device keeping a ledger of what it made, bound, destroyed and recorded.
#[derive(Debug, Default)]
pub struct Ledger {
    next_id: AtomicUsize,
    /// How many objects it was asked to make, refused ones included.
    asked: AtomicUsize,
    log: Arc<Mutex<Log>>,
    /// The object it refuses to make, counted from 1 in the order it is
    /// asked; `None` makes everything.
    refused: Mutex<Option<usize>>,
}

impl Ledger {
    /// A device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::default()
    }

    /// A device handing out ids from `first` on.
    pub fn counting_from(first: usize) -> Self {
        let ledger = Self::default();
        ledger.next_id.store(first - 1, Ordering::Relaxed);
        ledger
    }

    /// A device refusing the `n`-th object it is asked to make (from 1):
    /// buffers, textures, views, samplers and bind groups count.
    pub fn refusing_the(n: usize) -> Self {
        let ledger = Self::default();
        *locked(&ledger.refused) = Some(n);
        ledger
    }

    /// Makes everything from now on.
    pub fn stop_refusing(&self) {
        *locked(&self.refused) = None;
    }

    fn refuses(&self) -> bool {
        let asked = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        *locked(&self.refused) == Some(asked)
    }

    fn make(&self, kind: GpuKind, label: Option<&str>, size: Option<u64>) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        locked(&self.log).made.push(Made {
            kind,
            id,
            label: label.unwrap_or_default().to_owned(),
            size,
        });
        id
    }

    fn refusal() -> ResourceError {
        ResourceError::BackendError("refused by the ledger".into())
    }

    fn destroy(&self, kind: GpuKind, id: usize) -> Result<(), ResourceError> {
        locked(&self.log).destroyed.push((kind, id));
        Ok(())
    }

    /// Every object the lane made, in order.
    pub fn made(&self) -> Vec<Made> {
        locked(&self.log).made.clone()
    }

    /// The objects of `kind` the lane made.
    pub fn made_of(&self, kind: GpuKind) -> Vec<Made> {
        self.made()
            .into_iter()
            .filter(|made| made.kind == kind)
            .collect()
    }

    /// The object `(kind, id)`, if the lane made it.
    pub fn find(&self, kind: GpuKind, id: usize) -> Option<Made> {
        self.made()
            .into_iter()
            .find(|made| made.kind == kind && made.id == id)
    }

    /// How many objects the device was asked to make so far, refused ones
    /// included.
    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// Every destroy received so far.
    pub fn destroyed(&self) -> Vec<(GpuKind, usize)> {
        locked(&self.log).destroyed.clone()
    }

    /// Whether the object `(kind, id)` was destroyed.
    pub fn is_destroyed(&self, kind: GpuKind, id: usize) -> bool {
        locked(&self.log).destroyed.contains(&(kind, id))
    }

    /// The buffers the lane made and has not destroyed.
    pub fn live_buffers(&self) -> Vec<Made> {
        self.made_of(GpuKind::Buffer)
            .into_iter()
            .filter(|made| !self.is_destroyed(GpuKind::Buffer, made.id))
            .collect()
    }

    /// The bytes the live buffers hold.
    pub fn live_bytes(&self) -> u64 {
        self.live_buffers()
            .iter()
            .map(|buffer| buffer.size.expect("a buffer has a size"))
            .sum()
    }

    /// What `bind_group` binds at `binding`.
    pub fn bound_at(&self, bind_group: BindGroupId, binding: u32) -> Option<Bound> {
        locked(&self.log)
            .bound
            .get(&bind_group.0)
            .and_then(|entries| entries.iter().find(|(at, _)| *at == binding))
            .map(|(_, bound)| *bound)
    }

    /// The buffer `bind_group` binds at binding 0; panics when it binds none.
    pub fn buffer_of(&self, bind_group: BindGroupId) -> BufferId {
        match self.bound_at(bind_group, 0) {
            Some(Bound::Buffer(buffer)) => buffer,
            other => panic!("{bind_group:?} binds a buffer at 0, not {other:?}"),
        }
    }

    /// Every write to `buffer`, in order.
    pub fn writes_to(&self, buffer: BufferId) -> Vec<Write> {
        locked(&self.log)
            .writes
            .iter()
            .filter(|write| write.buffer == buffer)
            .cloned()
            .collect()
    }

    /// The bytes last written to `buffer` at `offset`; panics when none were.
    pub fn last_write_at(&self, buffer: BufferId, offset: u64) -> Vec<u8> {
        self.writes_to(buffer)
            .into_iter()
            .rev()
            .find(|write| write.offset == offset)
            .unwrap_or_else(|| panic!("nothing was written to {buffer:?} at {offset}"))
            .data
    }

    /// Every write, in order.
    pub fn writes(&self) -> Vec<Write> {
        locked(&self.log).writes.clone()
    }

    /// The buffers of every encoder opened.
    pub fn opened(&self) -> Vec<CommandBufferId> {
        locked(&self.log).opened.clone()
    }

    /// The buffers of every encoder finished.
    pub fn finished(&self) -> Vec<CommandBufferId> {
        locked(&self.log).finished.clone()
    }

    /// Every render pass recorded.
    pub fn passes(&self) -> Vec<RecordedPass> {
        locked(&self.log).passes.clone()
    }

    /// The buffers submitted.
    pub fn submitted(&self) -> Vec<CommandBufferId> {
        locked(&self.log).submitted.clone()
    }
}

/// An encoder of the ledger. The command buffer it finishes into is known
/// from the start, so a text renderer handed it can say which it was.
pub struct LedgerEncoder {
    buffer: CommandBufferId,
    log: Arc<Mutex<Log>>,
}

struct LedgerRenderPass {
    pass: RecordedPass,
    log: Arc<Mutex<Log>>,
}

impl Drop for LedgerRenderPass {
    fn drop(&mut self) {
        locked(&self.log).passes.push(self.pass.clone());
    }
}

impl RenderPass<'_> for LedgerRenderPass {
    fn set_pipeline(&mut self, pipeline: &RenderPipelineId) {
        self.pass.pipelines.push(*pipeline);
    }
    fn set_bind_group(&mut self, index: u32, bind_group: &BindGroupId, offsets: &[u32]) {
        self.pass.bind_groups.push(SetBindGroup {
            group: index,
            bind_group: *bind_group,
            offsets: offsets.to_vec(),
        });
    }
    fn set_vertex_buffer(&mut self, _slot: u32, buffer: &BufferId, _offset: u64) {
        self.pass.vertex_buffers.push(*buffer);
    }
    fn set_index_buffer(&mut self, _buffer: &BufferId, _offset: u64, _format: IndexFormat) {}
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draws.push((vertices, instances));
    }
    fn draw_indexed(&mut self, indices: Range<u32>, _base_vertex: i32, instances: Range<u32>) {
        self.pass.indexed_draws.push((indices, instances));
    }
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

struct LedgerComputePass;

impl ComputePass<'_> for LedgerComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for LedgerEncoder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        desc: &RenderPassDescriptor<'enc>,
    ) -> Box<dyn RenderPass<'enc> + 'enc> {
        let colors = desc
            .color_attachments
            .iter()
            .map(|attachment| *attachment.view)
            .collect();
        let loads_color = desc
            .color_attachments
            .iter()
            .all(|attachment| matches!(attachment.ops.load, LoadOp::Load));
        let depth = desc
            .depth_stencil_attachment
            .as_ref()
            .map(|attachment| *attachment.view);
        Box::new(LedgerRenderPass {
            pass: RecordedPass {
                buffer: self.buffer,
                colors,
                loads_color,
                depth,
                pipelines: Vec::new(),
                bind_groups: Vec::new(),
                vertex_buffers: Vec::new(),
                draws: Vec::new(),
                indexed_draws: Vec::new(),
            },
            log: Arc::clone(&self.log),
        })
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &ComputePassDescriptor<'enc>,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(LedgerComputePass)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(LedgerComputePass)
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
        locked(&self.log).finished.push(self.buffer);
        Some(self.buffer)
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GraphicsDevice for Ledger {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.make(
            GpuKind::ShaderModule,
            d.label,
            None,
        )))
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ShaderModule, id.0)
    }
    fn create_render_pipeline(
        &self,
        d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(self.make(
            GpuKind::RenderPipeline,
            d.label.as_deref(),
            None,
        )))
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.make(
            GpuKind::PipelineLayout,
            d.label.as_deref(),
            None,
        )))
    }
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::RenderPipeline, id.0)
    }
    fn create_compute_pipeline(
        &self,
        d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(
            self.make(GpuKind::ComputePipeline, d.label.as_deref(), None) as u64,
        ))
    }
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ComputePipeline, id.0 as usize)
    }
    fn create_bind_group_layout(
        &self,
        d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(self.make(
            GpuKind::BindGroupLayout,
            d.label,
            None,
        )))
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        if self.refuses() {
            return Err(Self::refusal());
        }
        let id = self.make(GpuKind::BindGroup, d.label, None);
        let entries = d
            .entries
            .iter()
            .map(|entry| {
                let bound = match &entry.resource {
                    BindingResource::Buffer(binding) => Bound::Buffer(binding.buffer),
                    BindingResource::TextureView(view) => Bound::View(*view),
                    BindingResource::Sampler(sampler) => Bound::Sampler(*sampler),
                };
                (entry.binding, bound)
            })
            .collect();
        locked(&self.log).bound.insert(id, entries);
        Ok(BindGroupId(id))
    }
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroupLayout, id.0)
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroup, id.0)
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        if self.refuses() {
            return Err(Self::refusal());
        }
        Ok(BufferId(self.make(
            GpuKind::Buffer,
            d.label.as_deref(),
            Some(d.size),
        )))
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        let buffer = self.create_buffer(d)?;
        self.write_buffer(buffer, 0, data)?;
        Ok(buffer)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Buffer, id.0)
    }
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        locked(&self.log).writes.push(Write {
            buffer: id,
            offset,
            data: data.to_vec(),
        });
        Ok(())
    }
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        let written = self.write_buffer(id, offset, data);
        Box::new(async move { written })
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        if self.refuses() {
            return Err(Self::refusal());
        }
        Ok(TextureId(self.make(
            GpuKind::Texture,
            d.label.as_deref(),
            None,
        )))
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Texture, id.0)
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
        d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        if self.refuses() {
            return Err(Self::refusal());
        }
        Ok(TextureViewId(self.make(
            GpuKind::TextureView,
            d.label.as_deref(),
            None,
        )))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::TextureView, id.0)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        if self.refuses() {
            return Err(Self::refusal());
        }
        Ok(SamplerId(self.make(
            GpuKind::Sampler,
            d.label.as_deref(),
            None,
        )))
    }
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Sampler, id.0)
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        let mut log = locked(&self.log);
        // Buffer ids start at 100, so a zeroed id is never mistaken for one.
        let buffer = CommandBufferId(100 + log.opened.len() as u64);
        log.opened.push(buffer);
        Box::new(LedgerEncoder {
            buffer,
            log: Arc::clone(&self.log),
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        locked(&self.log).submitted.push(command_buffer);
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(SURFACE_FORMAT)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        // Deliberately unlike every target of the tests: the lane must not
        // read it.
        (3, 3)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo::default()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        true
    }
}

/// One render pipeline the `PipelineSystem` was asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Requested {
    /// The id it answered with.
    pub id: RenderPipelineId,
    /// The shader the spec names.
    pub shader: &'static str,
    /// The spec's colour target formats.
    pub colors: Vec<TextureFormat>,
    /// The blend of each colour target.
    pub blends: Vec<Option<BlendStateDescriptor>>,
    /// The spec's depth format, if it tests depth.
    pub depth: Option<TextureFormat>,
    /// The spec's samples per pixel.
    pub samples: SampleCount,
    /// The spec's primitive topology.
    pub topology: PrimitiveTopology,
    /// How many bind group layouts the spec names.
    pub layouts: usize,
    /// How many vertex buffers the spec reads.
    pub vertex_buffers: usize,
}

/// A `PipelineSystem` answering every request with a fresh id at or above
/// [`PIPELINE_SYSTEM_IDS`], keeping every render pipeline spec it was asked
/// for. It owns what it hands out.
#[derive(Debug, Default)]
pub struct LedgerPipelines {
    next_id: AtomicUsize,
    requested: Mutex<Vec<Requested>>,
    /// The shaders whose pipelines it refuses.
    refused: Mutex<Vec<&'static str>>,
}

impl LedgerPipelines {
    fn next(&self) -> usize {
        PIPELINE_SYSTEM_IDS + self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Refuses every pipeline of `shader` from now on.
    pub fn refuse(&self, shader: &'static str) {
        locked(&self.refused).push(shader);
    }

    /// Builds every pipeline from now on.
    pub fn stop_refusing(&self) {
        locked(&self.refused).clear();
    }

    /// Every pipeline of `shader` the lane asked for, in order.
    pub fn of(&self, shader: &str) -> Vec<Requested> {
        locked(&self.requested)
            .iter()
            .filter(|requested| requested.shader == shader)
            .cloned()
            .collect()
    }

    /// The pipeline answered with `id`; panics when none was.
    pub fn by_id(&self, id: RenderPipelineId) -> Requested {
        locked(&self.requested)
            .iter()
            .find(|requested| requested.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id:?} is not a pipeline the lane asked for"))
    }
}

impl PipelineSystem for LedgerPipelines {
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
        spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        if locked(&self.refused).contains(&spec.shader) {
            return Err(RenderError::Internal("pipeline refused".into()));
        }
        let id = RenderPipelineId(self.next());
        locked(&self.requested).push(Requested {
            id,
            shader: spec.shader,
            colors: spec
                .color_targets
                .iter()
                .map(|target| target.format)
                .collect(),
            blends: spec
                .color_targets
                .iter()
                .map(|target| target.blend)
                .collect(),
            depth: spec.depth_stencil.as_ref().map(|depth| depth.format),
            samples: spec.multisample.count,
            topology: spec.primitive.topology,
            layouts: spec.bind_group_layouts.len(),
            vertex_buffers: spec.vertex_buffers.len(),
        });
        Ok(id)
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        Ok(ComputePipelineId(self.next() as u64))
    }
}

// ─── The text renderer ──────────────────────────────────────────────────────

/// A laid-out text of a fixed size: the size names which text it is.
pub struct FixedLayout(pub Vec2);

impl TextLayout for FixedLayout {
    fn size(&self) -> Vec2 {
        self.0
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// One text queued, as the text renderer was handed it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Queued {
    /// The size of its layout.
    pub layout: Vec2,
    pub pos: Vec2,
    pub color: Vec4,
    pub z_index: i32,
}

/// One flush of the text renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct Flush {
    /// The command buffer the encoder it was handed finishes into; `None`
    /// when that encoder was not one of the ledger's.
    pub encoder: Option<CommandBufferId>,
    /// The colour target it was handed.
    pub color: TextureViewId,
    /// The size it was handed.
    pub size: Extent2D,
    /// The texts queued since the flush before.
    pub texts: Vec<Queued>,
    /// How many render passes the lane's ledger had recorded by then.
    pub passes_before: usize,
}

/// A text renderer recording what it was queued and flushed.
pub struct RecordingText {
    /// The lane's ledger, to tell which passes came before a flush.
    device: Arc<Ledger>,
    pending: Mutex<Vec<Queued>>,
    queued: Mutex<Vec<Queued>>,
    flushes: Mutex<Vec<Flush>>,
    failing: AtomicBool,
}

impl RecordingText {
    /// A text renderer beside the lane's `device`.
    pub fn beside(device: &Arc<Ledger>) -> Self {
        Self {
            device: Arc::clone(device),
            pending: Mutex::new(Vec::new()),
            queued: Mutex::new(Vec::new()),
            flushes: Mutex::new(Vec::new()),
            failing: AtomicBool::new(false),
        }
    }

    /// Every text queued so far, in order.
    pub fn queued(&self) -> Vec<Queued> {
        locked(&self.queued).clone()
    }

    /// Every flush so far, in order.
    pub fn flushes(&self) -> Vec<Flush> {
        locked(&self.flushes).clone()
    }

    /// Fails every flush from now on.
    pub fn fail_flushes(&self) {
        self.failing.store(true, Ordering::SeqCst);
    }

    /// Succeeds every flush from now on.
    pub fn stop_failing(&self) {
        self.failing.store(false, Ordering::SeqCst);
    }
}

impl TextRenderer for RecordingText {
    fn layout_text(
        &self,
        _text: &str,
        _face: FontFace<'_>,
        _font_size: f32,
        _max_width: Option<f32>,
    ) -> Box<dyn TextLayout> {
        Box::new(FixedLayout(Vec2::new(40.0, 16.0)))
    }

    fn queue_text(&self, layout: &dyn TextLayout, pos: Vec2, color: Vec4, z_index: i32) {
        let queued = Queued {
            layout: layout.size(),
            pos,
            color,
            z_index,
        };
        locked(&self.pending).push(queued);
        locked(&self.queued).push(queued);
    }

    fn flush(
        &self,
        _device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        color_target: &TextureViewId,
        target: Extent2D,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let encoder = encoder
            .as_any_mut()
            .downcast_mut::<LedgerEncoder>()
            .map(|encoder| encoder.buffer);
        let texts = std::mem::take(&mut *locked(&self.pending));
        locked(&self.flushes).push(Flush {
            encoder,
            color: *color_target,
            size: target,
            texts,
            passes_before: self.device.passes().len(),
        });
        if self.failing.load(Ordering::SeqCst) {
            return Err("the text renderer refused the flush".into());
        }
        Ok(())
    }
}

// ─── Scenes ─────────────────────────────────────────────────────────────────

/// A target of `width`×`height` pixels, its colour in `color` and its depth,
/// when `depth` names a format, in that format; one sample per pixel.
pub fn target(
    width: u32,
    height: u32,
    color: TextureFormat,
    depth: Option<TextureFormat>,
) -> Target {
    sampled_target(width, height, color, depth, 1)
}

/// [`target`] with `samples` samples per pixel.
pub fn sampled_target(
    width: u32,
    height: u32,
    color: TextureFormat,
    depth: Option<TextureFormat>,
    samples: u32,
) -> Target {
    Target {
        color: COLOR_VIEW,
        depth: depth.map(|_| DEPTH_VIEW),
        size: Extent2D { width, height },
        key: TargetKey {
            color,
            depth,
            samples,
        },
    }
}

/// The target most frames draw into: 800×600, sRGB colour, 32-bit float
/// depth (the frame has one; the UI never tests it).
pub fn default_target() -> Target {
    target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth32Float),
    )
}

/// The same target at another size.
pub fn sized(width: u32, height: u32) -> Target {
    Target {
        size: Extent2D { width, height },
        ..default_target()
    }
}

/// A plain node at `pos`, `size` big: no colour, no border, no image.
pub fn node(pos: Vec2, size: Vec2) -> ExtractedUiNode {
    ExtractedUiNode {
        pos,
        size,
        color: None,
        border: None,
        image: None,
        z_index: 0,
    }
}

/// The `k`-th node of a column: at `(0, 30k)`, 100×20.
pub fn nth_node(k: usize) -> ExtractedUiNode {
    node(Vec2::new(0.0, 30.0 * k as f32), Vec2::new(100.0, 20.0))
}

/// A node in `color`, with a border `width` wide on its left and corners
/// `radius` round.
pub fn styled_node(pos: Vec2, size: Vec2, color: Vec4, width: f32, radius: f32) -> ExtractedUiNode {
    ExtractedUiNode {
        color: Some(UiColor(color)),
        border: Some(UiBorder {
            width: UiRect {
                left: width,
                right: width,
                top: width,
                bottom: width,
            },
            color: UiColor(Vec4::ONE),
            radius,
        }),
        ..node(pos, size)
    }
}

/// A node showing the image `texture`.
pub fn image_node(pos: Vec2, size: Vec2, texture: AssetUUID) -> ExtractedUiNode {
    ExtractedUiNode {
        image: Some(UiImage { texture }),
        ..node(pos, size)
    }
}

/// A text at `pos` on layer `z_index`, its layout `layout` big, in `color`.
pub fn text(pos: Vec2, layout: Vec2, color: Vec4, z_index: i32) -> ExtractedUiText {
    ExtractedUiText {
        pos,
        layout: Arc::new(FixedLayout(layout)),
        color,
        z_index,
    }
}

/// The `k`-th text of a column: at `(4, 30k + 2)`, its layout 40×(k+1).
pub fn nth_text(k: usize) -> ExtractedUiText {
    text(
        Vec2::new(4.0, 30.0 * k as f32 + 2.0),
        Vec2::new(40.0, k as f32 + 1.0),
        Vec4::ONE,
        0,
    )
}

/// What `text` looks like once queued.
pub fn queued(text: &ExtractedUiText) -> Queued {
    Queued {
        layout: text.layout.size(),
        pos: text.pos,
        color: text.color,
        z_index: text.z_index,
    }
}

/// A UI image atlas whose GPU texture exists, as the `image_on_gpu`
/// residency leaves it once a UI image is wanted. Its texture and view come
/// from a ledger of its own, so no two atlases share a view.
pub fn gpu_atlas() -> Arc<UiImageAtlas> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let first = ATLAS_IDS + 100 * NEXT.fetch_add(1, Ordering::Relaxed);
    let atlas = UiImageAtlas::new();
    assert!(
        atlas.ensure_atlas(&Ledger::counting_from(first)),
        "the atlas is made on its own ledger"
    );
    Arc::new(atlas)
}

/// The view of `atlas`'s GPU texture; panics when it has none.
pub fn atlas_view(atlas: &UiImageAtlas) -> TextureViewId {
    atlas
        .lock_atlas()
        .and_then(|guard| guard.as_ref().map(|atlas| atlas.view()))
        .expect("the atlas has a GPU texture")
}

/// A rect of the atlas, from `(u0, v0)` to `(u1, v1)`.
pub fn rect(u0: f32, v0: f32, u1: f32, v1: f32) -> AtlasRect {
    AtlasRect {
        min: Vec2::new(u0, v0),
        max: Vec2::new(u1, v1),
    }
}

/// What a frame hands the UI lane: the scene on the bus, the target, and the
/// image atlas.
pub struct Scene {
    pub ui: UiScene,
    pub target: Option<Target>,
    pub atlas: Arc<UiImageAtlas>,
}

thread_local! {
    /// The atlas every scene of one test shares, as the engine's one
    /// `UiImageAtlas` resource is shared by every frame.
    static THE_ATLAS: Arc<UiImageAtlas> = gpu_atlas();
}

impl Scene {
    /// A scene of one node and one text, drawn into [`default_target`], with
    /// the test's one atlas, holding its GPU texture.
    pub fn new() -> Self {
        Self {
            ui: UiScene {
                nodes: vec![nth_node(0)],
                texts: vec![nth_text(0)],
            },
            target: Some(default_target()),
            atlas: THE_ATLAS.with(Arc::clone),
        }
    }

    /// The same scene with no node and no text.
    pub fn empty(mut self) -> Self {
        self.ui = UiScene::default();
        self
    }

    /// The same scene with these nodes, and no other.
    pub fn with_nodes(mut self, nodes: Vec<ExtractedUiNode>) -> Self {
        self.ui.nodes = nodes;
        self
    }

    /// The same scene with `count` nodes.
    pub fn with_node_count(self, count: usize) -> Self {
        self.with_nodes((0..count).map(nth_node).collect())
    }

    /// The same scene with these texts, and no other.
    pub fn with_texts(mut self, texts: Vec<ExtractedUiText>) -> Self {
        self.ui.texts = texts;
        self
    }

    /// The same scene with `count` texts.
    pub fn with_text_count(self, count: usize) -> Self {
        self.with_texts((0..count).map(nth_text).collect())
    }

    /// The same scene drawn into `target`.
    pub fn into_target(mut self, target: Target) -> Self {
        self.target = Some(target);
        self
    }

    /// The same scene with no target handed to the lane.
    pub fn without_a_target(mut self) -> Self {
        self.target = None;
        self
    }

    /// The same scene with `atlas` as the UI image atlas.
    pub fn with_atlas(mut self, atlas: Arc<UiImageAtlas>) -> Self {
        self.atlas = atlas;
        self
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// The pass one record closed, as its slot held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPass {
    pub name: &'static str,
    pub reads: Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
    pub command_buffer: CommandBufferId,
}

/// What one record gave back.
pub struct Recorded {
    /// The run as the agent sees it.
    pub run: Run,
    /// The committed pass; `None` when the slot holds none.
    pub pass: Option<ClosedPass>,
}

impl Recorded {
    /// Panics unless the lane recorded and its pass was committed.
    pub fn recorded(&self) -> &ClosedPass {
        match &self.run.outcome {
            RunOutcome::Recorded(_) => {}
            other => panic!("the UI lane did not record: {other:?}"),
        }
        self.pass
            .as_ref()
            .expect("a recorded run closes the UI pass into its slot")
    }

    /// Panics unless the frame failed and committed nothing.
    pub fn failed(&self) {
        assert!(
            matches!(self.run.outcome, RunOutcome::Failed(_)),
            "the frame fails: {:?}",
            self.run.outcome
        );
        assert!(self.pass.is_none(), "a failed frame commits no pass");
    }

    /// The refusal of the frame's inputs; panics unless they were refused.
    pub fn refused(&self) -> InputRefusal {
        match &self.run.outcome {
            RunOutcome::Refused(refusal) => *refusal,
            other => panic!("expected the inputs to be refused, got {other:?}"),
        }
    }
}

/// The UI lane, kept across frames, on a ledger device, with a recording
/// text renderer.
pub struct Ui {
    lane: Erased<UiLane>,
    pub device: Arc<Ledger>,
    pub pipelines: Arc<LedgerPipelines>,
    pub text: Arc<RecordingText>,
    frame: u64,
}

impl Ui {
    /// A lane on a device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::on(Ledger::new())
    }

    /// A lane on `device`.
    pub fn on(device: Ledger) -> Self {
        let device = Arc::new(device);
        Self {
            lane: Erased::new(UiLane::default()),
            text: Arc::new(RecordingText::beside(&device)),
            device,
            pipelines: Arc::new(LedgerPipelines::default()),
            frame: 0,
        }
    }

    /// The lane, erased as its agent holds it.
    pub fn lane(&self) -> &dyn AnyLane {
        &self.lane
    }

    /// Records the next frame over `scene`.
    pub fn record(&mut self, scene: &Scene) -> Recorded {
        self.frame += 1;
        let frame = FrameId(self.frame);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(scene.ui.clone());
        let mut runtime = Runtime::new();
        runtime.resources.insert(Arc::clone(&scene.atlas));
        runtime
            .backends
            .insert(Arc::clone(&self.text) as Arc<dyn TextRenderer>);
        let mut deck = OutputDeck::new();
        let run = {
            let mut inputs = InputSource::new(frame, &bus, &runtime);
            if let Some(target) = scene.target {
                inputs = inputs.with_target(target);
            }
            let mut outputs = OutputSink::new(&mut deck).with_device(self.device.as_ref());
            let mut gpu =
                GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
            self.lane.run(LEVEL, &mut inputs, &mut outputs, &mut gpu)
        };
        Recorded {
            run,
            pass: deck
                .contains::<UiPassSlot>()
                .then(|| deck.take::<UiPassSlot>().0)
                .flatten()
                .map(|contribution| {
                    let PassDescriptor {
                        name,
                        reads,
                        writes,
                    } = contribution.descriptor;
                    ClosedPass {
                        name,
                        reads,
                        writes,
                        command_buffer: contribution.command_buffer,
                    }
                }),
        }
    }

    /// Shuts the lane down on its device.
    pub fn shut_down(&mut self) {
        let mut gpu = GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
        self.lane.shutdown(&mut gpu);
    }

    /// The level's graphics memory, as the lane quotes it now.
    pub fn quoted_memory(&self) -> u64 {
        self.lane().levels()[LEVEL].memory.gpu
    }

    /// The `n`-th render pass the lane recorded that draws quads.
    pub fn quad_pass(&self, n: usize) -> RecordedPass {
        let drawn: Vec<RecordedPass> = self
            .device
            .passes()
            .into_iter()
            .filter(|pass| !pass.draws.is_empty())
            .collect();
        drawn
            .get(n)
            .cloned()
            .unwrap_or_else(|| panic!("no quad pass #{n}: {drawn:?}"))
    }

    /// The buffer the `n`-th quad pass binds whole at `group`.
    pub fn buffer_at(&self, n: usize, group: u32) -> BufferId {
        let bind_group = self
            .quad_pass(n)
            .bind_group(group)
            .unwrap_or_else(|| panic!("quad pass #{n} binds group {group}"));
        self.device.buffer_of(bind_group)
    }
}

// ─── Shared assertions ──────────────────────────────────────────────────────

/// Asserts that `recorded` is a recorded frame whose slot holds the frame's
/// one encoder, finished, with no quad drawn through it.
pub fn assert_empty_pass(device: &Ledger, recorded: &Recorded) {
    let pass = recorded.recorded();
    assert_eq!(pass.name, "UiPass");
    assert_eq!(
        device.opened().last(),
        Some(&pass.command_buffer),
        "the slot holds the frame's encoder"
    );
    assert_eq!(
        device.finished().last(),
        Some(&pass.command_buffer),
        "the commit finished it"
    );
    let drawn: Vec<RecordedPass> = device
        .passes()
        .into_iter()
        .filter(|recorded| {
            recorded.buffer == pass.command_buffer
                && (!recorded.draws.is_empty() || !recorded.indexed_draws.is_empty())
        })
        .collect();
    assert!(drawn.is_empty(), "nothing is drawn: {drawn:?}");
}

/// Asserts that every object the lane made on `device` was destroyed, exactly
/// once, and that nothing else was: not the `PipelineSystem`'s layouts and
/// pipelines, not the atlas's texture and view.
pub fn assert_everything_released(device: &Ledger) {
    let made = device.made();
    let destroyed = device.destroyed();

    let leaked: Vec<String> = made
        .iter()
        .filter(|made| !destroyed.contains(&(made.kind, made.id)))
        .map(|made| format!("{:?} #{} {:?}", made.kind, made.id, made.label))
        .collect();
    assert!(
        leaked.is_empty(),
        "{} objects are left alive:\n  {}",
        leaked.len(),
        leaked.join("\n  ")
    );

    let foreign: Vec<String> = destroyed
        .iter()
        .filter(|(kind, id)| !made.iter().any(|made| made.kind == *kind && made.id == *id))
        .map(|(kind, id)| {
            let owner = if *id >= ATLAS_IDS {
                "the atlas's"
            } else if *id >= PIPELINE_SYSTEM_IDS {
                "the PipelineSystem's"
            } else {
                "never made"
            };
            format!("{kind:?} #{id} ({owner})")
        })
        .collect();
    assert!(
        foreign.is_empty(),
        "the lane destroys what it does not own:\n  {}",
        foreign.join("\n  ")
    );

    let mut seen = std::collections::HashSet::new();
    let twice: Vec<String> = destroyed
        .iter()
        .filter(|entry| !seen.insert(**entry))
        .map(|(kind, id)| format!("{kind:?} #{id}"))
        .collect();
    assert!(twice.is_empty(), "destroyed twice: {twice:?}");
}

/// The little-endian `f32`s of `bytes`.
pub fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// Where the column-major 4×4 `projection` sends the pixel `(x, y)`, in
/// normalised device coordinates.
pub fn ndc(projection: &[f32], x: f32, y: f32) -> (f32, f32) {
    assert_eq!(projection.len(), 16, "a 4×4 matrix: {projection:?}");
    let m = |row: usize, col: usize| projection[col * 4 + row];
    let clip_x = m(0, 0) * x + m(0, 1) * y + m(0, 3);
    let clip_y = m(1, 0) * x + m(1, 1) * y + m(1, 3);
    let clip_w = m(3, 0) * x + m(3, 1) * y + m(3, 3);
    (clip_x / clip_w, clip_y / clip_w)
}

/// Asserts the column-major `projection` maps the corners of a `target`-sized
/// UI onto the corners of clip space — top-left pixel to (-1, 1),
/// bottom-right to (1, -1) — and holds only finite numbers.
pub fn assert_projects_onto_clip_space(projection: &[f32], target: Extent2D) {
    let (w, h) = (target.width as f32, target.height as f32);
    for ((x, y), expected) in [
        ((0.0, 0.0), (-1.0, 1.0)),
        ((w, h), (1.0, -1.0)),
        ((w, 0.0), (1.0, 1.0)),
        ((0.0, h), (-1.0, -1.0)),
    ] {
        let got = ndc(projection, x, y);
        let close = (got.0 - expected.0).abs() < 1e-5 && (got.1 - expected.1).abs() < 1e-5;
        assert!(
            close,
            "{target:?}: pixel ({x}, {y}) -> {got:?}, expected {expected:?}"
        );
    }
    assert!(
        projection.iter().all(|v| v.is_finite()),
        "{target:?}: {projection:?}"
    );
}
