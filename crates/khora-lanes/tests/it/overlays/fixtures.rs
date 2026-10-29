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

//! The device, scenes and frames the overlay lanes' tests share.
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
//! A frame is what the engine hands an overlay lane: the `RenderWorld` and
//! the three overlay Views on the bus, stamped for the frame, the frame's
//! `Target`, a runtime holding the `AssetStore`, and an `OutputSink` whose
//! pass opens on the ledger.

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::Lane;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, OutputSink, PassSlot, Target, TargetKey};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{AffineTransform, Extent2D, LinearRgba, Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, BindingResource, CommandBufferId, ComputePassDescriptor, LoadOp,
    RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
use khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor;
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
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::editor::GizmoLineInstance;
use khora_core::Runtime;
use khora_data::render::{
    ExtractedMesh, ExtractedView, GizmoFrame, GizmoPassSlot, GizmoView, GridConfig, GridPassSlot,
    GridView, PassContribution, PassDescriptor, RenderWorld, ResourceId, WireframeConfig,
    WireframePassSlot, WireframeView,
};
use khora_data::AssetStore;
use khora_lanes::overlay_lane::{GizmoLane, GridLane, WireframeLane};

/// Each overlay lane's one level.
pub const LEVEL: usize = 0;

/// The shader the grid pipeline is composed from.
pub const GRID_SHADER: &str = "khora::pipelines::grid";
/// The shader the wireframe pipeline is composed from.
pub const WIREFRAME_SHADER: &str = "khora::pipelines::wireframe";
/// The shader the gizmo pipeline is composed from.
pub const GIZMO_SHADER: &str = "khora::pipelines::gizmo";

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

/// First id of the meshes' own buffers, uploaded before the lane ran: never
/// an id the ledger hands out.
const MESH_BUFFER_IDS: usize = 8_000_000;

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
    /// Whether the depth attachment clears what the target held.
    pub clears_depth: bool,
    /// The pipelines set, in order.
    pub pipelines: Vec<RenderPipelineId>,
    /// The bind groups set, in order.
    pub bind_groups: Vec<SetBindGroup>,
    /// The vertex buffers set, in order.
    pub vertex_buffers: Vec<BufferId>,
    /// The index buffers set, in order.
    pub index_buffers: Vec<BufferId>,
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

    /// Every `set_bind_group` at `group`, in order.
    pub fn sets_at(&self, group: u32) -> Vec<SetBindGroup> {
        self.bind_groups
            .iter()
            .filter(|set| set.group == group)
            .cloned()
            .collect()
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
    /// Whether its encoders fail to finish.
    unfinishable: bool,
    /// The object it refuses to make, counted from 1 in the order it is
    /// asked; `None` makes everything.
    refused: Mutex<Option<usize>>,
}

impl Ledger {
    /// A device that makes whatever it is asked to.
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

struct LedgerEncoder {
    buffer: CommandBufferId,
    log: Arc<Mutex<Log>>,
    unfinishable: bool,
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
    fn set_index_buffer(&mut self, buffer: &BufferId, _offset: u64, _format: IndexFormat) {
        self.pass.index_buffers.push(*buffer);
    }
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
        let (depth, clears_depth) =
            desc.depth_stencil_attachment
                .as_ref()
                .map_or((None, false), |attachment| {
                    (
                        Some(*attachment.view),
                        attachment
                            .depth_ops
                            .as_ref()
                            .is_some_and(|ops| matches!(ops.load, LoadOp::Clear(_))),
                    )
                });
        Box::new(LedgerRenderPass {
            pass: RecordedPass {
                buffer: self.buffer,
                colors,
                loads_color,
                depth,
                clears_depth,
                pipelines: Vec::new(),
                bind_groups: Vec::new(),
                vertex_buffers: Vec::new(),
                index_buffers: Vec::new(),
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
            unfinishable: self.unfinishable,
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        locked(&self.log).submitted.push(command_buffer);
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        Some(SURFACE_FORMAT)
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (800, 600)
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
    /// The spec's depth format, if it tests depth.
    pub depth: Option<TextureFormat>,
    /// Whether the spec writes depth.
    pub writes_depth: bool,
    /// The spec's depth comparison, if it tests depth.
    pub depth_compare: Option<CompareFunction>,
    /// The spec's samples per pixel.
    pub samples: SampleCount,
    /// The spec's primitive topology.
    pub topology: PrimitiveTopology,
    /// The spec's vertex buffers, as `(stride, attributes)`.
    pub vertex_buffers: Vec<(u64, Vec<VertexAttributeDescriptor>)>,
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
            depth: spec.depth_stencil.as_ref().map(|depth| depth.format),
            writes_depth: spec
                .depth_stencil
                .as_ref()
                .is_some_and(|depth| depth.depth_write_enabled),
            depth_compare: spec.depth_stencil.as_ref().map(|depth| depth.depth_compare),
            samples: spec.multisample.count,
            topology: spec.primitive.topology,
            vertex_buffers: spec
                .vertex_buffers
                .iter()
                .map(|layout| (layout.array_stride, layout.attributes.to_vec()))
                .collect(),
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

// ─── Scenes ─────────────────────────────────────────────────────────────────

/// An invertible view-projection: a translation by `(k, 2k, 3k)`.
pub fn view_proj(k: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(k, 2.0 * k, 3.0 * k))
}

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
/// depth.
pub fn default_target() -> Target {
    target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth32Float),
    )
}

/// A gizmo line from the origin along `x` by `k`, coloured by `k`.
pub fn line(k: f32) -> GizmoLineInstance {
    GizmoLineInstance::new(Vec3::ZERO, Vec3::new(k, 0.0, 0.0), [k, 0.0, 0.0, 1.0])
}

/// One mesh of a scene, as the wireframe should draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshBuffers {
    pub vertex: BufferId,
    pub index: BufferId,
    pub index_count: u32,
}

/// What a frame hands an overlay lane: the views and the three overlay Views
/// on the bus, the target, and the runtime holding the asset store.
pub struct Scene {
    pub render_world: RenderWorld,
    pub grid: GridView,
    pub wireframe: WireframeView,
    pub gizmos: GizmoView,
    pub target: Option<Target>,
    pub runtime: Runtime,
    meshes: usize,
}

impl Scene {
    /// A scene seen from one camera at `(1, 2, 3)` through [`view_proj`]`(1)`,
    /// drawn into [`default_target`]: the grid and the wireframe on, one
    /// gizmo line, no mesh, an empty asset store.
    pub fn new() -> Self {
        let mut runtime = Runtime::new();
        runtime.resources.insert(AssetStore::new());
        let mut render_world = RenderWorld::new();
        render_world.views.push(ExtractedView {
            view_proj: view_proj(1.0),
            position: Vec3::new(1.0, 2.0, 3.0),
        });
        Self {
            render_world,
            grid: GridView(GridConfig { enabled: true }),
            wireframe: WireframeView(WireframeConfig {
                enabled: true,
                ..WireframeConfig::default()
            }),
            gizmos: GizmoView(GizmoFrame {
                lines: vec![line(1.0)],
            }),
            target: Some(default_target()),
            runtime,
            meshes: 0,
        }
    }

    /// The same scene with the grid switched `on` or off.
    pub fn with_grid(mut self, on: bool) -> Self {
        self.grid = GridView(GridConfig { enabled: on });
        self
    }

    /// The same scene with the wireframe switched `on` or off.
    pub fn with_wireframe(mut self, on: bool) -> Self {
        self.wireframe.0.enabled = on;
        self
    }

    /// The same scene with the wireframe drawn in `color`, `width` wide.
    pub fn with_wireframe_style(mut self, color: LinearRgba, width: f32) -> Self {
        self.wireframe.0.line_color = color;
        self.wireframe.0.line_width = width;
        self
    }

    /// The same scene with these gizmo lines, and no other.
    pub fn with_lines(mut self, lines: Vec<GizmoLineInstance>) -> Self {
        self.gizmos = GizmoView(GizmoFrame { lines });
        self
    }

    /// The same scene with `count` gizmo lines.
    pub fn with_line_count(self, count: usize) -> Self {
        self.with_lines((0..count).map(|k| line(k as f32 + 1.0)).collect())
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

    /// The same scene with no view.
    pub fn without_views(mut self) -> Self {
        self.render_world.views.clear();
        self
    }

    /// The same scene seen only through `view_proj`, from `position`.
    pub fn seen_through(mut self, view_proj: Mat4, position: Vec3) -> Self {
        self.render_world.views = vec![ExtractedView {
            view_proj,
            position,
        }];
        self
    }

    /// The same scene with a second view after the first.
    pub fn with_second_view(mut self, view_proj: Mat4, position: Vec3) -> Self {
        self.render_world.views.push(ExtractedView {
            view_proj,
            position,
        });
        self
    }

    /// The same scene whose runtime holds no asset store.
    pub fn without_the_asset_store(mut self) -> Self {
        self.runtime = Runtime::new();
        self
    }

    /// The same scene with one more mesh at `at`, of `index_count` indices,
    /// uploaded to the GPU when `uploaded`. Returns the buffers it was
    /// uploaded to (made up when it was not).
    pub fn add_mesh(&mut self, at: Vec3, index_count: u32, uploaded: bool) -> MeshBuffers {
        self.meshes += 1;
        let buffers = MeshBuffers {
            vertex: BufferId(MESH_BUFFER_IDS + 2 * self.meshes),
            index: BufferId(MESH_BUFFER_IDS + 2 * self.meshes + 1),
            index_count,
        };
        let uuid = AssetUUID::new();
        let gpu_mesh = AssetHandle::new(GpuMesh {
            vertex_buffer: buffers.vertex,
            index_buffer: buffers.index,
            index_count,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        });
        if uploaded {
            self.runtime
                .resources
                .get::<AssetStore>()
                .expect("the scene's runtime holds the asset store")
                .store::<GpuMesh>()
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(uuid, gpu_mesh.clone());
        }
        self.render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::from_translation(at),
            cpu_mesh_uuid: uuid,
            gpu_mesh,
            material: None,
            gpu_material: None,
            mirrored: false,
        });
        buffers
    }

    /// The same scene with `count` uploaded meshes, the `k`-th at `(k, 0, 0)`
    /// with `3 * k` indices.
    pub fn with_meshes(mut self, count: usize) -> Self {
        for k in 1..=count {
            self.add_mesh(Vec3::new(k as f32, 0.0, 0.0), 3 * k as u32, true);
        }
        self
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// A pass slot of one of the overlays: the contribution it holds.
pub trait OverlaySlot: PassSlot {
    /// Takes the closed pass out of the slot.
    fn contribution(self) -> Option<PassContribution>;
}

impl OverlaySlot for GridPassSlot {
    fn contribution(self) -> Option<PassContribution> {
        self.0
    }
}

impl OverlaySlot for WireframePassSlot {
    fn contribution(self) -> Option<PassContribution> {
        self.0
    }
}

impl OverlaySlot for GizmoPassSlot {
    fn contribution(self) -> Option<PassContribution> {
        self.0
    }
}

/// The pass one record closed, as its slot held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPass {
    pub name: &'static str,
    pub reads: Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
    pub command_buffer: CommandBufferId,
}

impl ClosedPass {
    fn from(contribution: PassContribution) -> Self {
        let PassDescriptor {
            name,
            reads,
            writes,
        } = contribution.descriptor;
        Self {
            name,
            reads,
            writes,
            command_buffer: contribution.command_buffer,
        }
    }
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
            other => panic!("the overlay lane did not record: {other:?}"),
        }
        self.pass
            .as_ref()
            .expect("a recorded run closes the overlay pass into its slot")
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
}

/// One overlay lane `L`, closing into slot `S`, kept across frames, on a
/// ledger device.
pub struct Overlay<L: Lane, S: OverlaySlot> {
    lane: Erased<L>,
    pub device: Arc<Ledger>,
    pub pipelines: Arc<LedgerPipelines>,
    frame: u64,
    slot: PhantomData<S>,
}

/// The grid lane on a ledger.
pub type Grid = Overlay<GridLane, GridPassSlot>;
/// The wireframe lane on a ledger.
pub type Wireframe = Overlay<WireframeLane, WireframePassSlot>;
/// The gizmo lane on a ledger.
pub type Gizmos = Overlay<GizmoLane, GizmoPassSlot>;

impl<L: Lane + Default, S: OverlaySlot> Overlay<L, S> {
    /// A lane on a device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::on(Ledger::new())
    }

    /// A lane on `device`.
    pub fn on(device: Ledger) -> Self {
        Self {
            lane: Erased::new(L::default()),
            device: Arc::new(device),
            pipelines: Arc::new(LedgerPipelines::default()),
            frame: 0,
            slot: PhantomData,
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
        bus.publish(scene.render_world.clone());
        bus.publish(scene.grid.clone());
        bus.publish(scene.wireframe.clone());
        bus.publish(scene.gizmos.clone());
        let mut deck = OutputDeck::new();
        let run = {
            let mut inputs = InputSource::new(frame, &bus, &scene.runtime);
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
                .contains::<S>()
                .then(|| deck.take::<S>().contribution())
                .flatten()
                .map(ClosedPass::from),
        }
    }

    /// Shuts the lane down on its device.
    pub fn shut_down(&mut self) {
        let mut gpu = GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
        self.lane.shutdown(&mut gpu);
    }
}

// ─── Shared assertions ──────────────────────────────────────────────────────

/// Asserts that `recorded` is a recorded frame whose slot holds the frame's
/// one encoder, with nothing drawn through it.
pub fn assert_empty_pass(device: &Ledger, recorded: &Recorded) {
    assert!(
        matches!(recorded.run.outcome, RunOutcome::Recorded(_)),
        "a frame with nothing to draw is still recorded: {:?}",
        recorded.run.outcome
    );
    let pass = recorded.recorded();
    assert_eq!(
        device.opened(),
        [pass.command_buffer],
        "the slot holds the frame's one encoder"
    );
    assert_eq!(
        device.finished(),
        [pass.command_buffer],
        "the commit finished it"
    );
    let drawn: Vec<RecordedPass> = device
        .passes()
        .into_iter()
        .filter(|pass| !pass.draws.is_empty() || !pass.indexed_draws.is_empty())
        .collect();
    assert!(drawn.is_empty(), "nothing is drawn: {drawn:?}");
}

/// Asserts that every object the lane made on `device` was destroyed, exactly
/// once, and that nothing else was: not the `PipelineSystem`'s layouts and
/// pipelines, not the meshes' buffers.
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
            let owner = if *id >= MESH_BUFFER_IDS {
                "a mesh's"
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

/// A view-projection and a position, as a camera uniform block holds them:
/// the matrix column by column, then the position and a 1.
pub fn camera_block(view_proj: Mat4, position: Vec3) -> Vec<f32> {
    view_proj
        .to_cols_array_2d()
        .iter()
        .flatten()
        .copied()
        .chain([position.x, position.y, position.z, 1.0])
        .collect()
}
