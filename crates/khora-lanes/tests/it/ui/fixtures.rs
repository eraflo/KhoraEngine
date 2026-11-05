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
//! The glyph atlas is filled as the engine fills it: by the engine's own
//! `glyphs_on_gpu` DataSystem, over a world of laid-out texts, rasterising
//! through a fake rasteriser whose images have known sizes and offsets, on a
//! ledger of its own counting from [`GLYPH_IDS`]. Every draw the lane records
//! is kept with the pipeline and the bind groups bound when it was made, so a
//! test tells the quads' draws from the glyphs' by the shader of their
//! pipeline, wherever the lane records them.
//!
//! A frame is what the engine hands the UI lane: the `UiScene` on the bus,
//! stamped for the frame, the frame's `Target`, a runtime holding the image
//! atlas and the glyph atlas among its resources, and an `OutputSink` whose
//! pass opens on the ledger.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    BindGroupLayoutId, BindingResource, BindingType, BufferBindingType, CommandBufferId,
    ComputePassDescriptor, LoadOp, RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::renderer::api::pipeline::state::BlendStateDescriptor;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey, LayoutSpec,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
    RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SampleCount, SamplerDescriptor, SamplerId,
    TextureDescriptor, TextureFormat, TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::api::util::AtlasRect;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::text::{
    FontError, GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer, ShapedGlyph,
};
use khora_core::ui::types::UiRect;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, World};
use khora_data::render::{PassDescriptor, ResourceId, UiPassSlot};
use khora_data::ui::{
    ExtractedUiNode, ExtractedUiText, GlyphAtlas, GlyphSlot, UiBorder, UiColor, UiImage,
    UiImageAtlas, UiScene, UiTextLayout, UiTransform,
};
use khora_lanes::ui_lane::UiLane;

/// The UI lane's one level.
pub const LEVEL: usize = 0;

/// The shader the UI pipeline is composed from.
pub const UI_SHADER: &str = "khora::pipelines::ui";

/// The shader the UI's glyph pipeline is composed from.
pub const UI_TEXT_SHADER: &str = "khora::pipelines::ui_text";

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

/// First id of the glyph atlases' own ledgers, above the image atlases'.
pub const GLYPH_IDS: usize = 8_000_000;

/// The bytes one UI node takes in the instance storage: the shader's
/// `UiInstance` — position, size, colour, params, uv min, uv max — sixteen
/// floats.
pub const INSTANCE_BYTES: u64 = 64;

/// The nodes the instance storage held when it stopped silently past them.
pub const OLD_CAPACITY: usize = 1024;

/// The floats one glyph takes in the glyph instance storage, as
/// `ui_text.wgsl` reads it — in the order of the quads' `UiInstance`:
/// position (2), size (2), colour (4), params (4: `1.0` when it samples the
/// colour atlas, `0.0` the mask atlas, then three unused), uv min (2), uv max
/// (2).
pub const GLYPH_FLOATS: usize = 16;

/// What `ui_text.wgsl` binds, `(group, binding, slot)`: the projection at
/// 0.0; the glyph instances at 1.0; the mask atlas at 2.0, the colour atlas
/// at 2.1 and their sampler at 2.2. The shader's own test in khora-infra
/// pins the same set.
pub const UI_TEXT_BINDINGS: [(u32, u32, Slot); 5] = [
    (0, 0, Slot::Uniform),
    (1, 0, Slot::Storage),
    (2, 0, Slot::Texture),
    (2, 1, Slot::Texture),
    (2, 2, Slot::Sampler),
];

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
    /// Every non-indexed draw, with what was bound when it was made.
    pub drawn: Vec<Drawn>,
}

/// One non-indexed draw, with the pipeline and the bind groups bound when it
/// was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// Its place among every draw the ledger recorded, from 0.
    pub seq: usize,
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// The pipeline set last before it.
    pub pipeline: Option<RenderPipelineId>,
    /// The bind group set last at each group before it.
    pub groups: BTreeMap<u32, BindGroupId>,
    pub vertices: Range<u32>,
    pub instances: Range<u32>,
}

impl Drawn {
    /// The bind group bound at `group`; panics when none was.
    pub fn group(&self, group: u32) -> BindGroupId {
        *self
            .groups
            .get(&group)
            .unwrap_or_else(|| panic!("draw #{} binds no group {group}: {self:?}", self.seq))
    }
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
    /// How many draws were recorded, through every pass.
    draws: usize,
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

    /// Every non-indexed draw recorded, in the order it was made.
    pub fn drawn(&self) -> Vec<Drawn> {
        let mut drawn: Vec<Drawn> = self
            .passes()
            .into_iter()
            .flat_map(|pass| pass.drawn)
            .collect();
        drawn.sort_by_key(|draw| draw.seq);
        drawn
    }

    /// The bytes `buffer` holds now: every write to it, in order, over a
    /// buffer of its size.
    pub fn contents(&self, buffer: BufferId) -> Vec<u8> {
        let size = self
            .find(GpuKind::Buffer, buffer.0)
            .and_then(|made| made.size)
            .unwrap_or_else(|| panic!("{buffer:?} is not a buffer the lane made"));
        let mut bytes = vec![0; size as usize];
        for write in self.writes_to(buffer) {
            let start = write.offset as usize;
            let end = start + write.data.len();
            assert!(
                end <= bytes.len(),
                "a write past the end of {buffer:?}: {start}..{end} of {size}"
            );
            bytes[start..end].copy_from_slice(&write.data);
        }
        bytes
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
    /// The pipeline set last.
    pipeline: Option<RenderPipelineId>,
    /// The bind group set last at each group.
    groups: BTreeMap<u32, BindGroupId>,
}

impl Drop for LedgerRenderPass {
    fn drop(&mut self) {
        locked(&self.log).passes.push(self.pass.clone());
    }
}

impl RenderPass<'_> for LedgerRenderPass {
    fn set_pipeline(&mut self, pipeline: &RenderPipelineId) {
        self.pass.pipelines.push(*pipeline);
        self.pipeline = Some(*pipeline);
    }
    fn set_bind_group(&mut self, index: u32, bind_group: &BindGroupId, offsets: &[u32]) {
        self.groups.insert(index, *bind_group);
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
        let seq = {
            let mut log = locked(&self.log);
            log.draws += 1;
            log.draws - 1
        };
        self.pass.drawn.push(Drawn {
            seq,
            buffer: self.pass.buffer,
            pipeline: self.pipeline,
            groups: self.groups.clone(),
            vertices: vertices.clone(),
            instances: instances.clone(),
        });
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
                drawn: Vec::new(),
            },
            log: Arc::clone(&self.log),
            pipeline: None,
            groups: BTreeMap::new(),
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
    /// What its inline layouts bind, `(group, binding, slot)`.
    pub bindings: BTreeSet<(u32, u32, Slot)>,
}

/// What one binding of a layout holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Uniform,
    Storage,
    Texture,
    Sampler,
}

/// What `spec`'s inline layouts bind, `(group, binding, slot)`.
fn bindings_of(spec: &PipelineSpec) -> BTreeSet<(u32, u32, Slot)> {
    spec.bind_group_layouts
        .iter()
        .enumerate()
        .flat_map(|(group, layout)| match layout {
            LayoutSpec::Inline { entries, .. } => entries
                .iter()
                .map(|entry| {
                    let slot = match entry.ty {
                        BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            ..
                        } => Slot::Uniform,
                        BindingType::Buffer { .. } => Slot::Storage,
                        BindingType::Texture { .. } => Slot::Texture,
                        BindingType::Sampler(_) => Slot::Sampler,
                    };
                    (group as u32, entry.binding, slot)
                })
                .collect::<Vec<_>>(),
            LayoutSpec::Named(_) => Vec::new(),
        })
        .collect()
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
            bindings: bindings_of(spec),
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

// ─── The glyphs ─────────────────────────────────────────────────────────────

/// The font every text of the tests is laid out in.
pub const BODY: AssetUUID = AssetUUID::from_bytes(*b"khora:test:lane!");

/// The size every text of the tests is laid out at, in pixels per em.
pub const SIZE_PX: f32 = 16.0;

/// The glyph the fake rasteriser draws nothing for: a space.
pub const SPACE: u16 = b' ' as u16;

/// Glyph ids from this one up are colour glyphs to the fake rasteriser;
/// every other glyph but [`SPACE`] is a mask.
pub const COLOR_GLYPHS: u16 = 1000;

/// What the fake rasteriser makes of a mask glyph: a 5 × 8 coverage mask,
/// one pixel right of its pen, its top 7 pixels above the baseline.
pub const MASK_WIDTH: u32 = 5;
pub const MASK_HEIGHT: u32 = 8;
pub const MASK_LEFT: i32 = 1;
pub const MASK_TOP: i32 = 7;

/// What it makes of a colour glyph: a 6 × 6 RGBA image, one pixel left of
/// its pen, its top 9 pixels above the baseline.
pub const COLOR_WIDTH: u32 = 6;
pub const COLOR_HEIGHT: u32 = 6;
pub const COLOR_LEFT: i32 = -1;
pub const COLOR_TOP: i32 = 9;

/// A rasteriser of fixed images: a [`SPACE`] draws nothing, a glyph from
/// [`COLOR_GLYPHS`] up is a colour image, any other a mask.
struct FakeRasterizer;

impl GlyphRasterizer for FakeRasterizer {
    fn register_font(&self, _id: AssetUUID, _data: Arc<[u8]>) -> Result<(), FontError> {
        Ok(())
    }

    fn rasterize(&self, key: &GlyphKey) -> Option<GlyphImage> {
        if key.glyph == SPACE {
            None
        } else if key.glyph >= COLOR_GLYPHS {
            Some(GlyphImage {
                width: COLOR_WIDTH,
                height: COLOR_HEIGHT,
                left: COLOR_LEFT,
                top: COLOR_TOP,
                kind: GlyphKind::Color,
                pixels: vec![200; (COLOR_WIDTH * COLOR_HEIGHT * 4) as usize],
            })
        } else {
            Some(GlyphImage {
                width: MASK_WIDTH,
                height: MASK_HEIGHT,
                left: MASK_LEFT,
                top: MASK_TOP,
                kind: GlyphKind::Mask,
                pixels: vec![255; (MASK_WIDTH * MASK_HEIGHT) as usize],
            })
        }
    }
}

/// The key the glyph atlas holds `glyph` of a [`BODY`] text at [`SIZE_PX`]
/// under: the key `glyphs_on_gpu` makes it resident under.
pub fn key(glyph: &ShapedGlyph) -> GlyphKey {
    GlyphKey::new(BODY, glyph.id, SIZE_PX, glyph.x)
}

/// A glyph atlas, filled as the engine fills it: `glyphs_on_gpu` run over a
/// world of laid-out texts, with the fake rasteriser, on a ledger of its own.
pub struct Glyphs {
    world: World,
    runtime: Runtime,
    /// The atlas, as the engine's `GlyphAtlas` resource.
    pub atlas: Arc<GlyphAtlas>,
    last_seen: Tick,
}

impl Glyphs {
    /// An empty atlas, its textures not made yet.
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let first = GLYPH_IDS + 100 * NEXT.fetch_add(1, Ordering::Relaxed);
        let atlas = Arc::new(GlyphAtlas::default());
        let mut runtime = Runtime::new();
        runtime
            .backends
            .insert(Arc::new(Ledger::counting_from(first)) as Arc<dyn GraphicsDevice>);
        runtime
            .backends
            .insert(Arc::new(FakeRasterizer) as Arc<dyn GlyphRasterizer>);
        runtime.resources.insert(Arc::clone(&atlas));
        Self {
            world: World::new(),
            runtime,
            atlas,
            last_seen: Tick(0),
        }
    }

    /// Lays `glyphs` — `(id, x, y)` each — out as one [`BODY`] text at
    /// [`SIZE_PX`], and makes them resident as the engine does, through one
    /// run of `glyphs_on_gpu`. Returns the glyphs, as a `UiTextLayout` holds
    /// them.
    pub fn resident(&mut self, glyphs: &[(u16, f32, f32)]) -> Vec<ShapedGlyph> {
        let shaped: Vec<ShapedGlyph> = glyphs
            .iter()
            .map(|&(id, x, y)| ShapedGlyph { id, x, y })
            .collect();
        self.world.spawn((
            UiTransform::default(),
            UiTextLayout {
                font: BODY,
                size_px: SIZE_PX,
                size: Vec2::new(64.0, SIZE_PX),
                lines: 1,
                glyphs: shaped.clone(),
            },
        ));
        let system = khora_script::inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == "glyphs_on_gpu")
            .expect("the engine registers `glyphs_on_gpu`");
        let tick = self.world.begin_run();
        (system.run)(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            SystemContext {
                since: self.last_seen,
                resumed: false,
            },
        );
        self.world.end_run();
        self.last_seen = tick;
        shaped
    }

    /// The slot `glyph` has in the atlas; panics when it has none.
    pub fn slot(&self, glyph: &ShapedGlyph) -> GlyphSlot {
        self.atlas
            .slot(&key(glyph))
            .unwrap_or_else(|| panic!("{glyph:?} is resident"))
    }

    /// The views of the two atlases, `(mask, colour)`.
    pub fn views(&self) -> (Option<TextureViewId>, Option<TextureViewId>) {
        self.atlas.views()
    }
}

/// One glyph instance, as the lane uploaded it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    /// Its top-left corner, in UI pixels.
    pub pos: Vec2,
    /// Its size, in pixels.
    pub size: Vec2,
    pub color: Vec4,
    /// Whether it samples the colour atlas rather than the mask atlas.
    pub color_glyph: bool,
    /// Its rect in its atlas, in UV.
    pub uv_min: Vec2,
    pub uv_max: Vec2,
}

impl GlyphQuad {
    /// The instance `floats` — [`GLYPH_FLOATS`] of them — hold.
    pub fn read(floats: &[f32]) -> Self {
        assert_eq!(floats.len(), GLYPH_FLOATS, "{floats:?}");
        let color_glyph = if floats[8] == 0.0 {
            false
        } else if floats[8] == 1.0 {
            true
        } else {
            panic!(
                "a glyph samples the mask (0) or the colour (1) atlas, not {}",
                floats[8]
            )
        };
        Self {
            pos: Vec2::new(floats[0], floats[1]),
            size: Vec2::new(floats[2], floats[3]),
            color: Vec4::new(floats[4], floats[5], floats[6], floats[7]),
            color_glyph,
            uv_min: Vec2::new(floats[12], floats[13]),
            uv_max: Vec2::new(floats[14], floats[15]),
        }
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

/// A [`BODY`] text at [`SIZE_PX`] at `pos` on layer `z_index`, in `color`,
/// of `glyphs`.
pub fn glyph_text(pos: Vec2, color: Vec4, z_index: i32, glyphs: &[ShapedGlyph]) -> ExtractedUiText {
    ExtractedUiText {
        pos,
        color,
        z_index,
        font: BODY,
        size_px: SIZE_PX,
        glyphs: Arc::from(glyphs),
    }
}

/// The `k`-th text of a column: at `(4, 30k + 2)`, white, of one glyph no
/// atlas holds.
pub fn nth_text(k: usize) -> ExtractedUiText {
    glyph_text(
        Vec2::new(4.0, 30.0 * k as f32 + 2.0),
        Vec4::ONE,
        0,
        &[ShapedGlyph {
            id: 65 + k as u16,
            x: 0.0,
            y: 12.0,
        }],
    )
}

/// The glyphs the text of [`Scene::with_resident_text`] holds: "Hi", two
/// masks.
pub const RESIDENT_GLYPHS: usize = 2;

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

/// What a frame hands the UI lane: the scene on the bus, the target, the
/// image atlas and the glyph atlas.
pub struct Scene {
    pub ui: UiScene,
    pub target: Option<Target>,
    pub atlas: Arc<UiImageAtlas>,
    pub glyphs: Arc<GlyphAtlas>,
}

thread_local! {
    /// The atlas every scene of one test shares, as the engine's one
    /// `UiImageAtlas` resource is shared by every frame.
    static THE_ATLAS: Arc<UiImageAtlas> = gpu_atlas();
    /// The glyph atlas every scene of one test shares unless it says
    /// otherwise: empty, so no glyph of [`nth_text`] has a slot.
    static THE_GLYPHS: Arc<GlyphAtlas> = Arc::new(GlyphAtlas::default());
}

impl Scene {
    /// A scene of one node and one text, drawn into [`default_target`], with
    /// the test's one atlas, holding its GPU texture, and the test's empty
    /// glyph atlas: the text has no glyph to draw.
    pub fn new() -> Self {
        Self {
            ui: UiScene {
                nodes: vec![nth_node(0)],
                texts: vec![nth_text(0)],
            },
            target: Some(default_target()),
            atlas: THE_ATLAS.with(Arc::clone),
            glyphs: THE_GLYPHS.with(Arc::clone),
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

    /// The same scene with `glyphs` as the glyph atlas.
    pub fn with_glyphs(mut self, glyphs: Arc<GlyphAtlas>) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// The same scene whose one text, "Hi" at `(4, 2)` in white, has its
    /// [`RESIDENT_GLYPHS`] mask glyphs resident in a glyph atlas of its own,
    /// which has no colour atlas.
    pub fn with_resident_text(self) -> Self {
        let mut glyphs = Glyphs::new();
        let shaped = glyphs.resident(&[(u16::from(b'H'), 0.0, 12.0), (u16::from(b'i'), 9.0, 12.0)]);
        self.with_glyphs(Arc::clone(&glyphs.atlas))
            .with_texts(vec![glyph_text(Vec2::new(4.0, 2.0), Vec4::ONE, 0, &shaped)])
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

/// The UI lane, kept across frames, on a ledger device.
pub struct Ui {
    lane: Erased<UiLane>,
    pub device: Arc<Ledger>,
    pub pipelines: Arc<LedgerPipelines>,
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
        runtime.resources.insert(Arc::clone(&scene.glyphs));
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

    /// Every draw made with a pipeline of `shader`, in the order made.
    pub fn draws_with(&self, shader: &str) -> Vec<Drawn> {
        let pipelines: Vec<RenderPipelineId> = self
            .pipelines
            .of(shader)
            .into_iter()
            .map(|requested| requested.id)
            .collect();
        self.device
            .drawn()
            .into_iter()
            .filter(|draw| draw.pipeline.is_some_and(|id| pipelines.contains(&id)))
            .collect()
    }

    /// Every draw of the UI's quads, in the order made.
    pub fn quad_draws(&self) -> Vec<Drawn> {
        self.draws_with(UI_SHADER)
    }

    /// Every draw of glyphs, in the order made.
    pub fn glyph_draws(&self) -> Vec<Drawn> {
        self.draws_with(UI_TEXT_SHADER)
    }

    /// The glyph draws recorded through the encoder of `buffer`, in order.
    pub fn glyph_draws_in(&self, buffer: CommandBufferId) -> Vec<Drawn> {
        self.glyph_draws()
            .into_iter()
            .filter(|draw| draw.buffer == buffer)
            .collect()
    }

    /// The glyph instances drawn through the encoder of `buffer`, in draw
    /// order, as the storage bound at group 1 holds them now — read right
    /// after the frame.
    pub fn glyphs_drawn_in(&self, buffer: CommandBufferId) -> Vec<GlyphQuad> {
        self.glyph_draws_in(buffer)
            .iter()
            .flat_map(|draw| {
                let storage = self.device.buffer_of(draw.group(1));
                let all = floats(&self.device.contents(storage));
                let instances: Vec<GlyphQuad> = all
                    .chunks_exact(GLYPH_FLOATS)
                    .skip(draw.instances.start as usize)
                    .take(draw.instances.len())
                    .map(GlyphQuad::read)
                    .collect();
                assert_eq!(
                    instances.len(),
                    draw.instances.len(),
                    "draw #{} reads past its storage",
                    draw.seq
                );
                instances
            })
            .collect()
    }

    /// The glyph instances the last frame drew.
    pub fn last_glyphs(&self) -> Vec<GlyphQuad> {
        let last = *self
            .device
            .opened()
            .last()
            .expect("the lane recorded a frame");
        self.glyphs_drawn_in(last)
    }

    /// The bind group every glyph draw of the `n`-th frame binds at `group`;
    /// panics unless the frame drew glyphs, all through that one group.
    pub fn glyph_group(&self, n: usize, group: u32) -> BindGroupId {
        let buffer = self.device.opened()[n];
        let draws = self.glyph_draws_in(buffer);
        assert!(!draws.is_empty(), "frame #{n} draws glyphs");
        let bound: BTreeSet<usize> = draws.iter().map(|draw| draw.group(group).0).collect();
        assert_eq!(
            bound.len(),
            1,
            "frame #{n} binds one group {group}: {draws:?}"
        );
        draws[0].group(group)
    }

    /// The view `bind_group` binds at `binding`; panics when it binds none.
    pub fn view_at(&self, bind_group: BindGroupId, binding: u32) -> TextureViewId {
        match self.device.bound_at(bind_group, binding) {
            Some(Bound::View(view)) => view,
            other => panic!("{bind_group:?} binds a view at {binding}, not {other:?}"),
        }
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
