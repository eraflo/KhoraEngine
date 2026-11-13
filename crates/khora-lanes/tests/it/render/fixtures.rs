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

//! The device, scenes and frames the render lanes' tests share.
//!
//! The device is a ledger: it hands out ids and keeps every object it made
//! (with a buffer's size), what each bind group binds, every object it was
//! told to destroy, every encoder it opened and finished, every render pass
//! and every compute dispatch recorded through one, and every buffer
//! submitted. Whatever it made while an IBL bake ran is marked as the bake's,
//! so a test sees the lane's objects alone. The `PipelineSystem` hands out
//! ids of its own, at or above [`PIPELINE_SYSTEM_IDS`], and keeps what every
//! render pipeline spec it was asked for says.
//!
//! A frame is what the engine hands a render lane: the `RenderWorld` on the
//! bus, stamped for the frame; the frame's `Target` and clear colour; the
//! deck holding the `ShadowFrame` when the shadows drew one; a runtime
//! holding the `AssetStore` and the `IblBaker`; and an `OutputSink` whose
//! passes open on the ledger and are lent its pipeline system.

use std::collections::HashMap;
use std::future::Future;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::gpu::GpuSetup;
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, LaneRun, RunOutcome};
use khora_core::lane::guard::InputRefusal;
use khora_core::lane::io::{FrameId, InputSource, OutputSink, Target, TargetKey};
use khora_core::lane::level::Level;
use khora_core::lane::{ClearColor, LaneBus, OutputDeck};
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{AffineTransform, Extent2D, LinearRgba, Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, BindingResource, CommandBufferId, ComputePassDescriptor, LoadOp,
    RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::api::pipeline::enums::{CompareFunction, CullMode, PrimitiveTopology};
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
use khora_core::renderer::api::shadow::{ShadowFrame, ShadowGpuBindings};
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::render::{
    ExtractedLight, ExtractedMesh, ExtractedView, PassDescriptor, RenderWorld, ResourceId,
    ScenePassSlot, TransparentPassSlot,
};
use khora_data::{AssetStore, IblBaker, IblSource};
use khora_lanes::render::{ForwardPlusLane, LitForwardLane, SimpleUnlitLane};

/// The one level each render lane offers.
pub const LEVEL: usize = 0;

/// The format the ledger says its surface has: never the format of a test's
/// target, so a lane building for the surface instead of the target shows.
pub const SURFACE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

/// The frame's colour attachment, made by the renderer, not the lane.
pub const COLOR_VIEW: TextureViewId = TextureViewId(9_000_001);
/// The frame's depth attachment, made by the renderer, not the lane.
pub const DEPTH_VIEW: TextureViewId = TextureViewId(9_000_002);

/// The colour the frame's target is cleared to.
pub const CLEAR: LinearRgba = LinearRgba {
    r: 0.25,
    g: 0.5,
    b: 0.75,
    a: 1.0,
};

/// First id the [`LedgerPipelines`] hands out. The device counts from 1, so
/// an id at or above this one belongs to the `PipelineSystem`.
pub const PIPELINE_SYSTEM_IDS: usize = 1_000_000;

/// First id of the objects a scene's materials, meshes and shadow frame hold:
/// made by the engine before the lane ran, never by the lane.
const SCENE_IDS: usize = 8_000_000;

// ─── Which lane ─────────────────────────────────────────────────────────────

/// One of the render family's three lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `SimpleUnlitLane`, `LowPower`.
    Unlit,
    /// `LitForwardLane`, `Balanced`.
    Forward,
    /// `ForwardPlusLane`, `HighPerformance`.
    ForwardPlus,
}

/// Every lane of the family.
pub const EVERY_LANE: [Which; 3] = [Which::Unlit, Which::Forward, Which::ForwardPlus];

/// The lanes that light the scene.
pub const LIT_LANES: [Which; 2] = [Which::Forward, Which::ForwardPlus];

impl Which {
    /// A new lane of this kind, erased as its agent holds it.
    pub fn lane(self) -> Box<dyn AnyLane> {
        match self {
            Which::Unlit => Box::new(Erased::new(SimpleUnlitLane::default())),
            Which::Forward => Box::new(Erased::new(LitForwardLane::default())),
            Which::ForwardPlus => Box::new(Erased::new(ForwardPlusLane::default())),
        }
    }

    /// The binding the IBL block starts at in the lane's group-3 bind group:
    /// after the shadow bindings for the forward lane, after the Forward+
    /// light lists for Forward+.
    pub fn ibl_base(self) -> u32 {
        match self {
            Which::Unlit => panic!("the unlit lane binds no IBL"),
            Which::Forward => 4,
            Which::ForwardPlus => 8,
        }
    }
}

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

/// The kinds a lane owns once it made them: what its shutdown must give
/// back. Layouts and pipelines belong to the `PipelineSystem`.
pub const OWNED_KINDS: [GpuKind; 5] = [
    GpuKind::Buffer,
    GpuKind::Texture,
    GpuKind::TextureView,
    GpuKind::Sampler,
    GpuKind::BindGroup,
];

/// One object the device made.
#[derive(Debug, Clone)]
pub struct Made {
    pub kind: GpuKind,
    pub id: usize,
    pub label: String,
    /// A buffer's size in bytes; 0 for anything else.
    pub size: u64,
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
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedPass {
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// Whether an IBL bake recorded it.
    pub by_bake: bool,
    /// Its colour attachments.
    pub colors: Vec<TextureViewId>,
    /// The colour every colour attachment is cleared to; `None` when one
    /// loads what the target held.
    pub color_clear: Option<LinearRgba>,
    /// Its depth attachment, if it had one.
    pub depth: Option<TextureViewId>,
    /// The depth the depth attachment is cleared to; `None` when it loads.
    pub depth_clear: Option<f32>,
    /// The pipelines set, in order.
    pub pipelines: Vec<RenderPipelineId>,
    /// The bind groups set, as `(group, bind group)`, in order.
    pub bind_groups: Vec<(u32, BindGroupId)>,
    /// The draws, as `(vertices or indices, instances)`.
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

/// One compute dispatch recorded through an encoder of the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dispatch {
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// Whether an IBL bake recorded it.
    pub by_bake: bool,
    /// The workgroups dispatched.
    pub workgroups: (u32, u32, u32),
}

#[derive(Debug, Default)]
struct Log {
    made: Vec<Made>,
    bound: HashMap<usize, Vec<(u32, Bound)>>,
    destroyed: Vec<(GpuKind, usize)>,
    /// Every encoder opened, and whether a bake opened it.
    opened: Vec<(CommandBufferId, bool)>,
    finished: Vec<CommandBufferId>,
    passes: Vec<RecordedPass>,
    dispatches: Vec<Dispatch>,
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
pub struct RenderLedger {
    next_id: AtomicUsize,
    log: Arc<Mutex<Log>>,
    /// Whether an IBL bake is running on the device.
    baking: Arc<AtomicBool>,
}

impl RenderLedger {
    fn baking(&self) -> bool {
        self.baking.load(Ordering::SeqCst)
    }

    fn make(&self, kind: GpuKind, label: Option<&str>, size: u64) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        locked(&self.log).made.push(Made {
            kind,
            id,
            label: label.unwrap_or_default().to_owned(),
            size,
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

    /// Whether the lane made the object `(kind, id)`.
    pub fn lane_made_it(&self, kind: GpuKind, id: usize) -> bool {
        self.lane_made()
            .iter()
            .any(|made| made.kind == kind && made.id == id)
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

    /// The buffers of every encoder the lane's frames opened.
    pub fn lane_opened(&self) -> Vec<CommandBufferId> {
        locked(&self.log)
            .opened
            .iter()
            .filter(|(_, by_bake)| !by_bake)
            .map(|(buffer, _)| *buffer)
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

    /// The render passes recorded through the encoder that finished into
    /// `buffer`.
    pub fn passes_into(&self, buffer: CommandBufferId) -> Vec<RecordedPass> {
        self.lane_passes()
            .into_iter()
            .filter(|pass| pass.buffer == buffer)
            .collect()
    }

    /// Every compute dispatch the lane's frames recorded.
    pub fn lane_dispatches(&self) -> Vec<Dispatch> {
        locked(&self.log)
            .dispatches
            .iter()
            .filter(|dispatch| !dispatch.by_bake)
            .copied()
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

struct LedgerComputePass {
    buffer: CommandBufferId,
    by_bake: bool,
    log: Arc<Mutex<Log>>,
}

impl ComputePass<'_> for LedgerComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        locked(&self.log).dispatches.push(Dispatch {
            buffer: self.buffer,
            by_bake: self.by_bake,
            workgroups: (x, y, z),
        });
    }
}

impl LedgerEncoder {
    fn compute_pass(&self) -> LedgerComputePass {
        LedgerComputePass {
            buffer: self.buffer,
            by_bake: self.by_bake,
            log: Arc::clone(&self.log),
        }
    }
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
        let clears: Vec<Option<LinearRgba>> = desc
            .color_attachments
            .iter()
            .map(|attachment| match attachment.ops.load {
                LoadOp::Clear(color) => Some(color),
                LoadOp::Load => None,
            })
            .collect();
        let color_clear = if clears.iter().all(Option::is_some) {
            clears.first().copied().flatten()
        } else {
            None
        };
        let (depth, depth_clear) =
            desc.depth_stencil_attachment
                .as_ref()
                .map_or((None, None), |attachment| {
                    (
                        Some(*attachment.view),
                        attachment
                            .depth_ops
                            .as_ref()
                            .and_then(|ops| match ops.load {
                                LoadOp::Clear(depth) => Some(depth),
                                LoadOp::Load => None,
                            }),
                    )
                });
        Box::new(LedgerRenderPass {
            pass: RecordedPass {
                buffer: self.buffer,
                by_bake: self.by_bake,
                colors,
                color_clear,
                depth,
                depth_clear,
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
        Box::new(self.compute_pass())
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'enc> + 'enc> {
        Box::new(self.compute_pass())
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
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for RenderLedger {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.make(GpuKind::ShaderModule, d.label, 0)))
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
            0,
        )))
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.make(
            GpuKind::PipelineLayout,
            d.label.as_deref(),
            0,
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
            self.make(GpuKind::ComputePipeline, d.label.as_deref(), 0) as u64,
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
            0,
        )))
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        let id = self.make(GpuKind::BindGroup, d.label, 0);
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
        Ok(BufferId(self.make(
            GpuKind::Buffer,
            d.label.as_deref(),
            d.size,
        )))
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.create_buffer(d)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Buffer, id.0)
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
        Ok(TextureId(self.make(
            GpuKind::Texture,
            d.label.as_deref(),
            0,
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
        Ok(TextureViewId(self.make(
            GpuKind::TextureView,
            d.label.as_deref(),
            0,
        )))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::TextureView, id.0)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(self.make(
            GpuKind::Sampler,
            d.label.as_deref(),
            0,
        )))
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

/// What one render pipeline spec the `PipelineSystem` was asked for says.
#[derive(Debug, Clone, PartialEq)]
pub struct Requested {
    /// The id it answered with.
    pub id: RenderPipelineId,
    /// The shader the spec names.
    pub shader: &'static str,
    /// The spec's colour target formats.
    pub colors: Vec<TextureFormat>,
    /// Whether the spec blends its colour.
    pub blends: bool,
    /// The spec's depth format, if it tests depth.
    pub depth: Option<TextureFormat>,
    /// Whether the spec writes depth.
    pub writes_depth: bool,
    /// The spec's depth comparison, if it tests depth.
    pub depth_compare: Option<CompareFunction>,
    /// The spec's samples per pixel.
    pub samples: SampleCount,
    /// Which faces it culls.
    pub cull_mode: Option<CullMode>,
    /// The stride of each vertex buffer it reads.
    pub vertex_strides: Vec<u64>,
    /// Whether an IBL bake asked for it.
    pub by_bake: bool,
}

/// A `PipelineSystem` answering every request with a fresh id at or above
/// [`PIPELINE_SYSTEM_IDS`], keeping what every render pipeline spec it was
/// asked for says. It owns what it hands out.
#[derive(Debug, Default)]
pub struct LedgerPipelines {
    next_id: AtomicUsize,
    requested: Mutex<Vec<Requested>>,
    baking: Arc<AtomicBool>,
}

impl LedgerPipelines {
    fn next(&self) -> usize {
        PIPELINE_SYSTEM_IDS + self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Every render pipeline the lane asked for, in order.
    pub fn lane_pipelines(&self) -> Vec<Requested> {
        locked(&self.requested)
            .iter()
            .filter(|requested| !requested.by_bake)
            .cloned()
            .collect()
    }

    /// The pipeline the lane was handed as `id`.
    pub fn requested(&self, id: RenderPipelineId) -> Requested {
        self.lane_pipelines()
            .into_iter()
            .find(|requested| requested.id == id)
            .unwrap_or_else(|| panic!("{id:?} was handed out to the lane"))
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
                .any(|target| target.blend.is_some()),
            depth: spec.depth_stencil.as_ref().map(|depth| depth.format),
            writes_depth: spec
                .depth_stencil
                .as_ref()
                .is_some_and(|depth| depth.depth_write_enabled),
            depth_compare: spec.depth_stencil.as_ref().map(|depth| depth.depth_compare),
            samples: spec.multisample.count,
            cull_mode: spec.primitive.cull_mode,
            vertex_strides: spec
                .vertex_buffers
                .iter()
                .map(|layout| layout.array_stride)
                .collect(),
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

/// A target of `width`×`height` pixels and `samples` samples, its colour in
/// `color` and its depth, when `depth` names a format, in that format.
pub fn target(
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
/// depth, one sample.
pub fn default_target() -> Target {
    target(
        800,
        600,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth32Float),
        1,
    )
}

/// The target `width`×`height`, otherwise the default one.
pub fn sized(width: u32, height: u32) -> Target {
    target(
        width,
        height,
        TextureFormat::Rgba8UnormSrgb,
        Some(TextureFormat::Depth32Float),
        1,
    )
}

/// How a mesh's material draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Surface {
    /// Alpha-blended, drawn after the sky.
    pub blend: bool,
    /// Its back faces drawn too.
    pub double_sided: bool,
    /// Drawn through a mirroring matrix (negative determinant).
    pub mirrored: bool,
}

impl Surface {
    /// An opaque, single-sided, unmirrored surface.
    pub const OPAQUE: Surface = Surface {
        blend: false,
        double_sided: false,
        mirrored: false,
    };
    /// An alpha-blended surface.
    pub const BLENDED: Surface = Surface {
        blend: true,
        double_sided: false,
        mirrored: false,
    };
}

/// What a frame hands a render lane.
pub struct Scene {
    pub render_world: RenderWorld,
    pub target: Option<Target>,
    pub clear: ClearColor,
    /// The shadow frame on the deck, when the shadows family drew one.
    pub shadow: Option<ShadowFrame>,
    /// The asset store the meshes are in, and the IBL bake.
    pub runtime: Runtime,
    assets: AssetStore,
    next_id: usize,
}

impl Scene {
    /// A scene seen from one camera at `(1, 2, 3)`, drawn into
    /// [`default_target`], holding no mesh and no light; no shadow frame; an
    /// IBL bake that has not run.
    pub fn new() -> Self {
        Self::with_baker(IblBaker::new())
    }

    /// [`Scene::new`] with `baker` as the IBL bake.
    pub fn with_baker(baker: IblBaker) -> Self {
        let assets = AssetStore::new();
        let mut runtime = Runtime::new();
        runtime.resources.insert(assets.clone());
        runtime.resources.insert(baker);
        let mut render_world = RenderWorld::new();
        render_world.views.push(ExtractedView {
            view_proj: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            position: Vec3::new(1.0, 2.0, 3.0),
        });
        Self {
            render_world,
            target: Some(default_target()),
            clear: ClearColor(CLEAR),
            shadow: None,
            runtime,
            assets,
            next_id: SCENE_IDS,
        }
    }

    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    /// The same scene with one more mesh on the GPU, its material drawing as
    /// `surface`, `k` units along the view axis.
    pub fn with_mesh(mut self, surface: Surface, k: f32) -> Self {
        let uuid = AssetUUID::new();
        let gpu_mesh = AssetHandle::new(GpuMesh {
            vertex_buffer: BufferId(self.id()),
            index_buffer: BufferId(self.id()),
            index_count: 36,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        });
        self.assets
            .store::<GpuMesh>()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(uuid, gpu_mesh.clone());
        let material = GpuMaterial {
            uniform_buffer: BufferId(self.id()),
            base_color_view: None,
            metallic_roughness_view: None,
            normal_view: None,
            emissive_view: None,
            occlusion_view: None,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            occlusion_texture: None,
            specular_view: None,
            specular_color_view: None,
            specular_texture: None,
            specular_color_texture: None,
            sampler: SamplerId(self.id()),
            bind_group: BindGroupId(self.id()),
            variant: ShaderVariantKey::empty(),
            double_sided: surface.double_sided,
            blend: surface.blend,
        };
        let scale = if surface.mirrored {
            Vec3::new(-1.0, 1.0, 1.0)
        } else {
            Vec3::ONE
        };
        self.render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform(
                Mat4::from_translation(Vec3::new(0.0, 0.0, -k)) * Mat4::from_scale(scale),
            ),
            cpu_mesh_uuid: uuid,
            gpu_mesh,
            material: None,
            gpu_material: Some(AssetHandle::new(material)),
            mirrored: surface.mirrored,
        });
        self
    }

    /// The same scene with `count` more opaque meshes.
    pub fn with_meshes(mut self, count: usize) -> Self {
        for k in 0..count {
            self = self.with_mesh(Surface::OPAQUE, 2.0 + k as f32);
        }
        self
    }

    fn light(mut self, light_type: LightType, count: usize) -> Self {
        for k in 0..count {
            self.render_world.lights.push(ExtractedLight {
                light_type,
                position: Vec3::new(k as f32, 5.0, 0.0),
                direction: Vec3::new(0.0, -1.0, 0.0),
                shadow_view_proj: Mat4::IDENTITY,
                shadow_atlas_index: None,
            });
        }
        self
    }

    /// The same scene with `count` more directional lights.
    pub fn with_suns(self, count: usize) -> Self {
        self.light(LightType::Directional(DirectionalLight::default()), count)
    }

    /// The same scene with `count` more point lights.
    pub fn with_lamps(self, count: usize) -> Self {
        self.light(LightType::Point(PointLight::default()), count)
    }

    /// The same scene with `count` more spot lights.
    pub fn with_spots(self, count: usize) -> Self {
        self.light(LightType::Spot(SpotLight::default()), count)
    }

    /// The same scene with no camera.
    pub fn without_a_camera(mut self) -> Self {
        self.render_world.views.clear();
        self
    }

    /// The same scene drawn into `target`.
    pub fn into_target(mut self, target: Target) -> Self {
        self.target = Some(target);
        self
    }

    /// The same scene with the shadows family's frame on the deck, its
    /// atlases and sampler made by the engine (never by the lane).
    pub fn with_shadow_frame(mut self) -> Self {
        let bindings = ShadowGpuBindings {
            atlas_2d: TextureViewId(self.id()),
            atlas_cube: TextureViewId(self.id()),
            sampler: SamplerId(self.id()),
        };
        self.shadow = Some(ShadowFrame {
            bindings: Some(bindings),
            ..ShadowFrame::default()
        });
        self
    }

    /// The shadow frame's bindings.
    pub fn shadow_bindings(&self) -> ShadowGpuBindings {
        self.shadow
            .as_ref()
            .and_then(|frame| frame.bindings)
            .expect("the scene has a shadow frame")
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// A pass one record closed, as its slot held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPass {
    pub name: &'static str,
    pub reads: Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
    pub command_buffer: CommandBufferId,
}

impl ClosedPass {
    fn from(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        let PassDescriptor {
            name,
            reads,
            writes,
        } = descriptor;
        Self {
            name,
            reads,
            writes,
            command_buffer,
        }
    }
}

/// What one record gave back.
pub struct Recorded {
    /// The run as the agent sees it.
    pub run: LaneRun,
    /// The committed scene pass; `None` when the slot holds none.
    pub scene: Option<ClosedPass>,
    /// The committed transparent pass; `None` when the slot holds none.
    pub transparent: Option<ClosedPass>,
}

impl Recorded {
    /// Panics unless the lane recorded and both passes were committed: the
    /// scene pass and the transparent pass.
    pub fn recorded(&self) -> (&ClosedPass, &ClosedPass) {
        match &self.run.outcome {
            RunOutcome::Recorded(_) => {}
            other => panic!("the render lane did not record: {other:?}"),
        }
        (
            self.scene
                .as_ref()
                .expect("a recorded run closes the scene pass into its slot"),
            self.transparent
                .as_ref()
                .expect("a recorded run closes the transparent pass into its slot"),
        )
    }

    /// The refusal of a refused run; panics on any other outcome.
    pub fn refused(&self) -> InputRefusal {
        match &self.run.outcome {
            RunOutcome::Refused(refusal) => *refusal,
            other => panic!("expected the frame refused, got {other:?}"),
        }
    }
}

/// One render lane, kept across frames, on a ledger device.
pub struct Renderer {
    pub which: Which,
    lane: Box<dyn AnyLane>,
    pub device: Arc<RenderLedger>,
    pub pipelines: Arc<LedgerPipelines>,
    frame: u64,
}

impl Renderer {
    /// A lane of kind `which` on a device that makes whatever it is asked to.
    pub fn new(which: Which) -> Self {
        let device = RenderLedger::default();
        let pipelines = LedgerPipelines {
            baking: Arc::clone(&device.baking),
            ..LedgerPipelines::default()
        };
        Self {
            which,
            lane: which.lane(),
            device: Arc::new(device),
            pipelines: Arc::new(pipelines),
            frame: 0,
        }
    }

    /// The lane's levels.
    pub fn levels(&self) -> Vec<Level> {
        self.lane.levels().to_vec()
    }

    /// An IBL bake of the procedural sky, run on the lane's device: what the
    /// `ibl_bake` DataSystem publishes before the lit lanes draw.
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

    /// Records the next frame over `scene`.
    pub fn record(&mut self, scene: &Scene) -> Recorded {
        self.frame += 1;
        let frame = FrameId(self.frame);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(scene.render_world.clone());
        let mut earlier = OutputDeck::new();
        if let Some(shadow) = &scene.shadow {
            *earlier.slot::<ShadowFrame>() = shadow.clone();
        }
        let mut deck = OutputDeck::new();
        let run = {
            let mut inputs = InputSource::new(frame, &bus, &scene.runtime)
                .with_deck(&earlier)
                .with_clear_color(scene.clear);
            if let Some(target) = scene.target {
                inputs = inputs.with_target(target);
            }
            let mut outputs = OutputSink::new(&mut deck)
                .with_device(self.device.as_ref())
                .with_pipelines(self.pipelines.as_ref());
            let mut gpu =
                GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
            self.lane.run(LEVEL, &mut inputs, &mut outputs, &mut gpu)
        };
        let scene_pass = deck
            .contains::<ScenePassSlot>()
            .then(|| deck.take::<ScenePassSlot>().0)
            .flatten()
            .map(|pass| ClosedPass::from(pass.descriptor, pass.command_buffer));
        let transparent = deck
            .contains::<TransparentPassSlot>()
            .then(|| deck.take::<TransparentPassSlot>().0)
            .flatten()
            .map(|pass| ClosedPass::from(pass.descriptor, pass.command_buffer));
        Recorded {
            run,
            scene: scene_pass,
            transparent,
        }
    }

    /// What the lane quotes for every level over `scene`, without running.
    pub fn quote(&self, scene: &Scene) -> Option<Vec<Cost>> {
        let frame = FrameId(self.frame + 1);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(scene.render_world.clone());
        let mut earlier = OutputDeck::new();
        if let Some(shadow) = &scene.shadow {
            *earlier.slot::<ShadowFrame>() = shadow.clone();
        }
        let mut inputs = InputSource::new(frame, &bus, &scene.runtime)
            .with_deck(&earlier)
            .with_clear_color(scene.clear);
        if let Some(target) = scene.target {
            inputs = inputs.with_target(target);
        }
        self.lane.quote(&mut inputs)
    }

    /// The lane's quote at its one level over `scene`; panics when the lane
    /// is not quoted.
    pub fn cost_of(&self, scene: &Scene) -> Cost {
        let quote = self
            .quote(scene)
            .unwrap_or_else(|| panic!("{:?} is quoted over the scene", self.which));
        quote[LEVEL]
    }

    /// Shuts the lane down on its device.
    pub fn shut_down(&mut self) {
        let mut gpu = GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
        self.lane.shutdown(&mut gpu);
    }
}
