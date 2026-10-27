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

//! The device, scenes and frames the skybox lane's tests share.
//!
//! The device is a ledger: it hands out ids, and keeps every object it made,
//! what each bind group binds, every object it was told to destroy, every
//! buffer write, every encoder it opened and finished, every render pass
//! recorded through one, and every buffer submitted. Whatever it made while
//! an IBL bake ran is marked as the bake's, so a test sees the lane's objects
//! alone. The `PipelineSystem` hands out ids of its own, at or above
//! [`PIPELINE_SYSTEM_IDS`], and keeps every pipeline spec it was asked for.
//!
//! A frame is what the engine hands the lane: the `RenderWorld` on the bus,
//! stamped for the frame, the frame's `Target`, a runtime holding the
//! `IblBaker`, and an `OutputSink` whose pass opens on the ledger.

use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Extent2D, Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, BindingResource, CommandBufferId, ComputePassDescriptor, LoadOp,
    RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::pipeline::enums::CompareFunction;
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
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::{ExtractedView, PassDescriptor, RenderWorld, ResourceId, SkyboxPassSlot};
use khora_data::{IblBaker, IblSource};
use khora_lanes::skybox_lane::SkyboxLane;

/// The skybox lane's one level.
pub const LEVEL: usize = 0;

/// The shader the skybox pipeline is composed from.
pub const SKYBOX_SHADER: &str = "khora::pipelines::skybox";

/// The format the ledger says its surface has: never the format of a test's
/// target, so a lane building for the surface instead of the target shows.
pub const SURFACE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

/// The frame's colour attachment, made by the renderer, not the lane.
pub const COLOR_VIEW: TextureViewId = TextureViewId(9_000_001);
/// The frame's depth attachment, made by the renderer, not the lane.
pub const DEPTH_VIEW: TextureViewId = TextureViewId(9_000_002);

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

/// First id the [`LedgerPipelines`] hands out. The device counts from 1, so
/// an id at or above this one belongs to the `PipelineSystem`.
pub const PIPELINE_SYSTEM_IDS: usize = 1_000_000;

/// One object the device made.
#[derive(Debug, Clone)]
pub struct Made {
    pub kind: GpuKind,
    pub id: usize,
    pub label: String,
    /// Whether an IBL bake made it, rather than the lane.
    pub by_bake: bool,
}

/// What one binding of a bind group binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Buffer(BufferId),
    View(TextureViewId),
    Sampler(SamplerId),
}

/// One render pass recorded through an encoder of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPass {
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// Whether an IBL bake recorded it.
    pub by_bake: bool,
    /// Its colour attachments.
    pub colors: Vec<TextureViewId>,
    /// Whether every colour attachment loads what the target held.
    pub loads_color: bool,
    /// Its depth attachment, if it had one.
    pub depth: Option<TextureViewId>,
    /// Whether the depth attachment loads what the target held.
    pub loads_depth: bool,
    /// The pipelines set, in order.
    pub pipelines: Vec<RenderPipelineId>,
    /// The bind groups set, as `(group, bind group)`, in order.
    pub bind_groups: Vec<(u32, BindGroupId)>,
    /// The draws, as `(vertices, instances)`.
    pub draws: Vec<(Range<u32>, Range<u32>)>,
}

impl RecordedPass {
    /// The bind group set last at `group`.
    pub fn bind_group(&self, group: u32) -> Option<BindGroupId> {
        self.bind_groups
            .iter()
            .rev()
            .find(|(at, _)| *at == group)
            .map(|(_, bind_group)| *bind_group)
    }
}

/// One buffer write.
#[derive(Debug, Clone)]
pub struct Write {
    pub buffer: BufferId,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
struct Log {
    made: Vec<Made>,
    bound: HashMap<usize, Vec<(u32, Bound)>>,
    destroyed: Vec<(GpuKind, usize)>,
    writes: Vec<Write>,
    /// Every encoder opened, and whether a bake opened it.
    opened: Vec<(CommandBufferId, bool)>,
    finished: Vec<CommandBufferId>,
    passes: Vec<RecordedPass>,
    /// Every buffer submitted, and whether a bake submitted it.
    submitted: Vec<(CommandBufferId, bool)>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A device keeping a ledger of what it made, bound, destroyed and recorded.
#[derive(Debug, Default)]
pub struct SkyLedger {
    next_id: AtomicUsize,
    log: Arc<Mutex<Log>>,
    /// Whether an IBL bake is running on the device.
    baking: Arc<AtomicBool>,
    /// Whether its encoders fail to finish.
    unfinishable: bool,
    /// The labels of the bind groups it refuses to make.
    pub refused_bind_groups: Mutex<Vec<&'static str>>,
}

impl SkyLedger {
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

    fn baking(&self) -> bool {
        self.baking.load(Ordering::SeqCst)
    }

    fn make(&self, kind: GpuKind, label: Option<&str>) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        locked(&self.log).made.push(Made {
            kind,
            id,
            label: label.unwrap_or_default().to_owned(),
            by_bake: self.baking(),
        });
        id
    }

    fn destroy(&self, kind: GpuKind, id: usize) -> Result<(), ResourceError> {
        locked(&self.log).destroyed.push((kind, id));
        Ok(())
    }

    /// Every object the lane made, in order.
    pub fn lane_made(&self) -> Vec<Made> {
        locked(&self.log)
            .made
            .iter()
            .filter(|made| !made.by_bake)
            .cloned()
            .collect()
    }

    /// The objects of `kind` the lane made.
    pub fn lane_made_of(&self, kind: GpuKind) -> Vec<Made> {
        self.lane_made()
            .into_iter()
            .filter(|made| made.kind == kind)
            .collect()
    }

    /// Every object an IBL bake made.
    pub fn bake_made(&self) -> Vec<Made> {
        locked(&self.log)
            .made
            .iter()
            .filter(|made| made.by_bake)
            .cloned()
            .collect()
    }

    /// Every destroy received so far.
    pub fn destroyed(&self) -> Vec<(GpuKind, usize)> {
        locked(&self.log).destroyed.clone()
    }

    /// Whether the object `(kind, id)` was destroyed.
    pub fn is_destroyed(&self, kind: GpuKind, id: usize) -> bool {
        locked(&self.log).destroyed.contains(&(kind, id))
    }

    /// What `bind_group` binds at `binding`.
    pub fn bound_at(&self, bind_group: BindGroupId, binding: u32) -> Option<Bound> {
        locked(&self.log)
            .bound
            .get(&bind_group.0)
            .and_then(|entries| entries.iter().find(|(at, _)| *at == binding))
            .map(|(_, bound)| *bound)
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

    /// The buffers of every encoder the lane's frames opened.
    pub fn lane_opened(&self) -> Vec<CommandBufferId> {
        locked(&self.log)
            .opened
            .iter()
            .filter(|(_, by_bake)| !by_bake)
            .map(|(buffer, _)| *buffer)
            .collect()
    }

    /// The buffers of every encoder finished, outside a bake.
    pub fn lane_finished(&self) -> Vec<CommandBufferId> {
        let log = locked(&self.log);
        log.finished
            .iter()
            .filter(|buffer| log.opened.contains(&(**buffer, false)))
            .copied()
            .collect()
    }

    /// Every render pass the lane's frames recorded.
    pub fn lane_passes(&self) -> Vec<RecordedPass> {
        locked(&self.log)
            .passes
            .iter()
            .filter(|pass| !pass.by_bake)
            .cloned()
            .collect()
    }

    /// The buffers submitted outside a bake.
    pub fn lane_submitted(&self) -> Vec<CommandBufferId> {
        locked(&self.log)
            .submitted
            .iter()
            .filter(|(_, by_bake)| !by_bake)
            .map(|(buffer, _)| *buffer)
            .collect()
    }
}

struct LedgerEncoder {
    buffer: CommandBufferId,
    by_bake: bool,
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
    fn set_bind_group(&mut self, index: u32, bind_group: &BindGroupId, _o: &[u32]) {
        self.pass.bind_groups.push((index, *bind_group));
    }
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draws.push((vertices, instances));
    }
    fn draw_indexed(&mut self, indices: Range<u32>, _bv: i32, instances: Range<u32>) {
        self.pass.draws.push((indices, instances));
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
        let (depth, loads_depth) =
            desc.depth_stencil_attachment
                .as_ref()
                .map_or((None, false), |attachment| {
                    (
                        Some(*attachment.view),
                        attachment
                            .depth_ops
                            .as_ref()
                            .is_some_and(|ops| matches!(ops.load, LoadOp::Load)),
                    )
                });
        Box::new(LedgerRenderPass {
            pass: RecordedPass {
                buffer: self.buffer,
                by_bake: self.by_bake,
                colors,
                loads_color,
                depth,
                loads_depth,
                pipelines: Vec::new(),
                bind_groups: Vec::new(),
                draws: Vec::new(),
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
        if self.unfinishable && !self.by_bake {
            return None;
        }
        locked(&self.log).finished.push(self.buffer);
        Some(self.buffer)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for SkyLedger {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.make(GpuKind::ShaderModule, d.label)))
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ShaderModule, id.0)
    }
    fn create_render_pipeline(
        &self,
        d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(
            self.make(GpuKind::RenderPipeline, d.label.as_deref()),
        ))
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(
            self.make(GpuKind::PipelineLayout, d.label.as_deref()),
        ))
    }
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::RenderPipeline, id.0)
    }
    fn create_compute_pipeline(
        &self,
        d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(
            self.make(GpuKind::ComputePipeline, d.label.as_deref()) as u64,
        ))
    }
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ComputePipeline, id.0 as usize)
    }
    fn create_bind_group_layout(
        &self,
        d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(
            self.make(GpuKind::BindGroupLayout, d.label),
        ))
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        if d.label
            .is_some_and(|label| locked(&self.refused_bind_groups).contains(&label))
        {
            return Err(ResourceError::BackendError("bind group refused".into()));
        }
        let id = self.make(GpuKind::BindGroup, d.label);
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
        Ok(BufferId(self.make(GpuKind::Buffer, d.label.as_deref())))
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
    fn write_buffer(&self, id: BufferId, _offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        locked(&self.log).writes.push(Write {
            buffer: id,
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
        Ok(TextureId(self.make(GpuKind::Texture, d.label.as_deref())))
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
        Ok(TextureViewId(
            self.make(GpuKind::TextureView, d.label.as_deref()),
        ))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::TextureView, id.0)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.make(GpuKind::Sampler, d.label.as_deref())))
    }
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Sampler, id.0)
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        let by_bake = self.baking();
        let mut log = locked(&self.log);
        // Buffer ids start at 100, so a zeroed id is never mistaken for one.
        let buffer = CommandBufferId(100 + log.opened.len() as u64);
        log.opened.push((buffer, by_bake));
        Box::new(LedgerEncoder {
            buffer,
            by_bake,
            log: Arc::clone(&self.log),
            unfinishable: self.unfinishable,
        })
    }
    fn submit_command_buffer(&self, command_buffer: CommandBufferId) {
        let by_bake = self.baking();
        locked(&self.log).submitted.push((command_buffer, by_bake));
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Whether an IBL bake asked for it.
    pub by_bake: bool,
}

/// A `PipelineSystem` answering every request with a fresh id at or above
/// [`PIPELINE_SYSTEM_IDS`], keeping every render pipeline spec it was asked
/// for. It owns what it hands out.
#[derive(Debug, Default)]
pub struct LedgerPipelines {
    next_id: AtomicUsize,
    requested: Mutex<Vec<Requested>>,
    baking: Arc<AtomicBool>,
    /// Whether it refuses every skybox pipeline it is asked for.
    pub refuses_skybox_pipelines: AtomicBool,
}

impl LedgerPipelines {
    fn next(&self) -> usize {
        PIPELINE_SYSTEM_IDS + self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Every skybox pipeline the lane asked for, in order.
    pub fn skybox_pipelines(&self) -> Vec<Requested> {
        locked(&self.requested)
            .iter()
            .filter(|requested| !requested.by_bake && requested.shader == SKYBOX_SHADER)
            .cloned()
            .collect()
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
        if spec.shader == SKYBOX_SHADER && self.refuses_skybox_pipelines.load(Ordering::SeqCst) {
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
            by_bake: self.baking.load(Ordering::SeqCst),
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
/// when `depth` names a format, in that format.
pub fn target(
    width: u32,
    height: u32,
    color: TextureFormat,
    depth: Option<TextureFormat>,
) -> Target {
    Target {
        color: COLOR_VIEW,
        depth: depth.map(|_| DEPTH_VIEW),
        size: Extent2D { width, height },
        key: TargetKey {
            color,
            depth,
            samples: 1,
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

/// What a frame hands the lane: the views on the bus, the target, and the
/// runtime holding the IBL bake.
pub struct Scene {
    pub render_world: RenderWorld,
    pub target: Option<Target>,
    pub runtime: Runtime,
    /// The bake's bindings, when the bake ran.
    pub env: Option<IblGpuBindings>,
}

impl Scene {
    /// A scene seen from one camera at `(1, 2, 3)` through [`view_proj`]`(1)`,
    /// drawn into [`default_target`], the runtime holding `baker`.
    pub fn with_baker(baker: IblBaker) -> Self {
        let env = baker.bindings();
        let mut runtime = Runtime::new();
        runtime.resources.insert(baker);
        let mut render_world = RenderWorld::new();
        render_world.views.push(ExtractedView {
            view_proj: view_proj(1.0),
            position: Vec3::new(1.0, 2.0, 3.0),
        });
        Self {
            render_world,
            target: Some(default_target()),
            runtime,
            env,
        }
    }

    /// A scene whose runtime holds no IBL bake at all.
    pub fn without_a_baker() -> Self {
        let mut scene = Self::with_baker(IblBaker::new());
        scene.runtime = Runtime::new();
        scene
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

    /// The environment cube the bake published.
    pub fn env_cube(&self) -> TextureViewId {
        self.env.expect("the scene's bake ran").env_cube
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// The skybox pass one record closed, as its slot held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPass {
    pub name: &'static str,
    pub reads: Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
    pub command_buffer: CommandBufferId,
}

impl ClosedPass {
    fn from_slot(slot: SkyboxPassSlot) -> Option<Self> {
        slot.0.map(|pass| {
            let PassDescriptor {
                name,
                reads,
                writes,
            } = pass.descriptor;
            Self {
                name,
                reads,
                writes,
                command_buffer: pass.command_buffer,
            }
        })
    }
}

/// What one record gave back.
pub struct Recorded {
    /// The run as the agent sees it.
    pub run: Run,
    /// The committed skybox pass; `None` when the slot holds none.
    pub pass: Option<ClosedPass>,
}

impl Recorded {
    /// Panics unless the lane recorded and its pass was committed.
    pub fn recorded(&self) -> &ClosedPass {
        match &self.run.outcome {
            RunOutcome::Recorded(_) => {}
            other => panic!("the skybox lane did not record: {other:?}"),
        }
        self.pass
            .as_ref()
            .expect("a recorded run closes the skybox pass into its slot")
    }
}

/// One skybox lane, kept across frames, on a ledger device.
pub struct Sky {
    lane: Erased<SkyboxLane>,
    pub device: Arc<SkyLedger>,
    pub pipelines: Arc<LedgerPipelines>,
    frame: u64,
}

impl Sky {
    /// A lane on a device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::on(SkyLedger::new())
    }

    /// A lane on `device`.
    pub fn on(device: SkyLedger) -> Self {
        let pipelines = LedgerPipelines {
            baking: Arc::clone(&device.baking),
            ..LedgerPipelines::default()
        };
        Self {
            lane: Erased::new(SkyboxLane::default()),
            device: Arc::new(device),
            pipelines: Arc::new(pipelines),
            frame: 0,
        }
    }

    /// The lane, erased as its agent holds it.
    pub fn lane(&self) -> &dyn AnyLane {
        &self.lane
    }

    /// An IBL bake of the procedural sky, run on the lane's device: what the
    /// `ibl_bake` DataSystem publishes before the skybox draws.
    pub fn bake(&self) -> IblBaker {
        let baker = IblBaker::new();
        self.device.baking.store(true, Ordering::SeqCst);
        baker.ensure_baked(
            self.device.as_ref(),
            self.pipelines.as_ref(),
            Vec3::new(0.3, 1.0, 0.2),
            &IblSource::procedural(),
        );
        self.device.baking.store(false, Ordering::SeqCst);
        assert!(
            baker.bindings().is_some(),
            "the bake ran on the ledger and published its bindings"
        );
        baker
    }

    /// A scene seen from one camera, drawn into the default target, its sky
    /// baked on the lane's device.
    pub fn scene(&self) -> Scene {
        Scene::with_baker(self.bake())
    }

    /// Records the next frame over `scene`.
    pub fn record(&mut self, scene: &Scene) -> Recorded {
        self.frame += 1;
        let frame = FrameId(self.frame);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(scene.render_world.clone());
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
                .contains::<SkyboxPassSlot>()
                .then(|| ClosedPass::from_slot(deck.take::<SkyboxPassSlot>()))
                .flatten(),
        }
    }

    /// Shuts the lane down on its device.
    pub fn shut_down(&mut self) {
        let mut gpu = GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
        self.lane.shutdown(&mut gpu);
    }
}
