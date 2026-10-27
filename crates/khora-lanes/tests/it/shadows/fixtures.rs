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

//! The device, scenes and frames the shadows lane's tests share.
//!
//! The device is a ledger: it hands out ids, and keeps every object it made
//! (with the texture's size and the view's dimension), every object it was
//! told to destroy, every encoder it opened and finished, every render pass
//! recorded through one, and every buffer submitted. The `PipelineSystem`
//! hands out ids of its own, at or above [`PIPELINE_SYSTEM_IDS`], which the
//! lane must never destroy.
//!
//! A frame is what the engine hands the lane: the `RenderWorld` and
//! `ShadowView` on the bus, stamped for the frame, an `InputSource` over the
//! bus and a runtime holding the `AssetStore`, and an `OutputSink` whose
//! passes open on the ledger.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::affine_transform::AffineTransform;
use khora_core::math::dimension::{Extent3D, Origin3D};
use khora_core::math::{Mat4, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindGroupLayoutId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
    PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, PrimitiveTopology,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
    TextureFormat, TextureId, TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
use khora_core::renderer::api::shadow::ShadowFrame;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, PipelineSystem, RenderPass,
};
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;
use khora_data::flow::{ShadowMatrices, ShadowView};
use khora_data::render::{
    ExtractedLight, ExtractedMesh, PassDescriptor, RenderWorld, ResourceId, ShadowPassSlot,
};
use khora_data::AssetStore;
use khora_lanes::shadow_lane::ShadowsLane;

// ─── Levels ─────────────────────────────────────────────────────────────────

/// The level drawing into the smallest atlases.
pub const LOW: usize = 0;
/// The middle level.
pub const MEDIUM: usize = 1;
/// The level drawing into the largest atlases.
pub const HIGH: usize = 2;

/// Each level with the side of its 2D atlas layers and of its cube faces, in
/// texels, cheapest first.
pub const TIERS: [(usize, u32, u32); 3] = [(LOW, 512, 128), (MEDIUM, 1024, 256), (HIGH, 2048, 512)];

/// The directional and spot lights the 2D atlas holds a frame, at every level.
pub const ATLAS_LIGHTS: u32 = 4;
/// The point lights the cube atlas holds a frame, at every level.
pub const CUBE_LIGHTS: u32 = 4;

const MIB: u64 = 1024 * 1024;

/// The graphics memory each level's atlases hold, at four bytes a texel:
/// 2D layers of `side²` for four lights, cube faces of `face²` for four
/// cubes of six faces.
pub const LEVEL_MEMORY: [u64; 3] = [
    4 * MIB + 3 * MIB / 2,
    16 * MIB + 6 * MIB,
    64 * MIB + 24 * MIB,
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

/// First id the [`LedgerPipelines`] hands out. The device counts from 1, so
/// an id at or above this one belongs to the `PipelineSystem`.
pub const PIPELINE_SYSTEM_IDS: usize = 1_000_000;

/// One object the device made.
#[derive(Debug, Clone)]
pub struct Made {
    pub kind: GpuKind,
    pub id: usize,
    pub label: String,
    /// A texture's size.
    pub size: Option<Extent3D>,
    /// A texture's format.
    pub format: Option<TextureFormat>,
    /// A view's dimension.
    pub dimension: Option<TextureViewDimension>,
}

/// One render pass recorded through an encoder of the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPass {
    /// The buffer of the encoder it was recorded through.
    pub buffer: CommandBufferId,
    /// Its depth attachment, if it had one.
    pub depth: Option<TextureViewId>,
    /// The layer of the attachment it rendered into.
    pub layer: u32,
    /// The draws it recorded.
    pub draws: u32,
}

#[derive(Debug, Default)]
struct Log {
    made: Vec<Made>,
    destroyed: Vec<(GpuKind, usize)>,
    opened: Vec<CommandBufferId>,
    finished: Vec<CommandBufferId>,
    passes: Vec<RecordedPass>,
    submitted: Vec<CommandBufferId>,
}

/// A device keeping a ledger of what it made, destroyed and recorded.
#[derive(Debug, Default)]
pub struct ShadowLedger {
    next_id: AtomicUsize,
    log: Arc<Mutex<Log>>,
    /// The array layers of a texture the device refuses, once.
    refuse_once: Mutex<Option<u32>>,
    /// Whether its encoders fail to finish.
    unfinishable: bool,
    /// The kind, and the 1-based rank among the objects of that kind made
    /// from then on, of an object the device refuses, once.
    refuse_nth: Mutex<Option<(GpuKind, usize)>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ShadowLedger {
    /// A device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::default()
    }

    /// A device refusing, once, the first texture of `layers` array layers
    /// it is asked for — the cube atlas, at 24.
    pub fn refusing_once_a_texture_of(layers: u32) -> Self {
        Self {
            refuse_once: Mutex::new(Some(layers)),
            ..Self::default()
        }
    }

    /// A device whose encoders never finish.
    pub fn unfinishable() -> Self {
        Self {
            unfinishable: true,
            ..Self::default()
        }
    }

    /// A device refusing, once, the `n`-th object of `kind` it is asked for
    /// (1-based, counting the ones it made).
    pub fn refusing_the_nth(kind: GpuKind, n: usize) -> Self {
        Self {
            refuse_nth: Mutex::new(Some((kind, n))),
            ..Self::default()
        }
    }

    /// Whether the next object of `kind` is the one to refuse; disarms the
    /// refusal when it is.
    fn refuses(&self, kind: GpuKind) -> Result<(), ResourceError> {
        let mut refuse = locked(&self.refuse_nth);
        if let Some((refused, n)) = *refuse {
            let made = self.made().iter().filter(|made| made.kind == kind).count();
            if refused == kind && made + 1 == n {
                *refuse = None;
                return Err(ResourceError::BackendError(format!(
                    "the ledger refuses {kind:?} #{n}, once"
                )));
            }
        }
        Ok(())
    }

    fn make(&self, made: Made) -> usize {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        locked(&self.log).made.push(Made { id, ..made });
        id
    }

    fn plain(&self, kind: GpuKind, label: Option<&str>) -> usize {
        self.make(Made {
            kind,
            id: 0,
            label: label.unwrap_or_default().to_owned(),
            size: None,
            format: None,
            dimension: None,
        })
    }

    fn destroy(&self, kind: GpuKind, id: usize) -> Result<(), ResourceError> {
        locked(&self.log).destroyed.push((kind, id));
        Ok(())
    }

    /// Every object made so far.
    pub fn made(&self) -> Vec<Made> {
        locked(&self.log).made.clone()
    }

    /// Every destroy received so far.
    pub fn destroyed(&self) -> Vec<(GpuKind, usize)> {
        locked(&self.log).destroyed.clone()
    }

    /// The objects made and not destroyed.
    pub fn alive(&self) -> Vec<Made> {
        let log = locked(&self.log);
        log.made
            .iter()
            .filter(|made| !log.destroyed.contains(&(made.kind, made.id)))
            .cloned()
            .collect()
    }

    /// The depth textures alive, as `(side, array layers)`, sorted.
    pub fn depth_textures(&self) -> Vec<(u32, u32)> {
        let mut textures: Vec<(u32, u32)> = self
            .alive()
            .iter()
            .filter(|made| {
                made.kind == GpuKind::Texture && made.format == Some(TextureFormat::Depth32Float)
            })
            .filter_map(|made| made.size)
            .map(|size| {
                assert_eq!(size.width, size.height, "a shadow atlas layer is square");
                (size.width, size.depth_or_array_layers)
            })
            .collect();
        textures.sort_unstable();
        textures
    }

    /// The live views of `dimension`.
    pub fn views_of(&self, dimension: TextureViewDimension) -> Vec<TextureViewId> {
        self.alive()
            .iter()
            .filter(|made| made.kind == GpuKind::TextureView && made.dimension == Some(dimension))
            .map(|made| TextureViewId(made.id))
            .collect()
    }

    /// The live samplers.
    pub fn samplers(&self) -> Vec<SamplerId> {
        self.alive()
            .iter()
            .filter(|made| made.kind == GpuKind::Sampler)
            .map(|made| SamplerId(made.id))
            .collect()
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

    /// The buffers submitted to the queue.
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
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {
        self.pass.draws += 1;
    }
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {
        self.pass.draws += 1;
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
        let (depth, layer) = desc
            .depth_stencil_attachment
            .as_ref()
            .map_or((None, 0), |attachment| {
                (Some(*attachment.view), attachment.base_array_layer)
            });
        Box::new(LedgerRenderPass {
            pass: RecordedPass {
                buffer: self.buffer,
                depth,
                layer,
                draws: 0,
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

impl GraphicsDevice for ShadowLedger {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(self.plain(GpuKind::ShaderModule, d.label)))
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ShaderModule, id.0)
    }
    fn create_render_pipeline(
        &self,
        d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(
            self.plain(GpuKind::RenderPipeline, d.label.as_deref()),
        ))
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(
            self.plain(GpuKind::PipelineLayout, d.label.as_deref()),
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
            self.plain(GpuKind::ComputePipeline, d.label.as_deref()) as u64,
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
            self.plain(GpuKind::BindGroupLayout, d.label),
        ))
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.refuses(GpuKind::BindGroup)?;
        Ok(BindGroupId(self.plain(GpuKind::BindGroup, d.label)))
    }
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroupLayout, id.0)
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroup, id.0)
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        self.refuses(GpuKind::Buffer)?;
        Ok(BufferId(self.plain(GpuKind::Buffer, d.label.as_deref())))
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
        {
            let mut refuse = locked(&self.refuse_once);
            if *refuse == Some(d.size.depth_or_array_layers) {
                *refuse = None;
                return Err(ResourceError::BackendError(
                    "the ledger refuses this texture, once".to_owned(),
                ));
            }
        }
        Ok(TextureId(self.make(Made {
            kind: GpuKind::Texture,
            id: 0,
            label: d.label.as_deref().unwrap_or_default().to_owned(),
            size: Some(d.size),
            format: Some(d.format),
            dimension: None,
        })))
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
        Ok(TextureViewId(self.make(Made {
            kind: GpuKind::TextureView,
            id: 0,
            label: d.label.as_deref().unwrap_or_default().to_owned(),
            size: None,
            format: d.format,
            dimension: d.dimension,
        })))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::TextureView, id.0)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        self.refuses(GpuKind::Sampler)?;
        Ok(SamplerId(self.plain(GpuKind::Sampler, d.label.as_deref())))
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
        Some(TextureFormat::Rgba8UnormSrgb)
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

/// A `PipelineSystem` answering every request with a fresh id at or above
/// [`PIPELINE_SYSTEM_IDS`]. It owns what it hands out.
#[derive(Debug, Default)]
pub struct LedgerPipelines {
    next_id: AtomicUsize,
}

impl LedgerPipelines {
    fn next(&self) -> usize {
        PIPELINE_SYSTEM_IDS + self.next_id.fetch_add(1, Ordering::Relaxed)
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

// ─── Scenes ─────────────────────────────────────────────────────────────────

/// A view-projection no two lights share: a translation by `k`.
pub fn view_proj(k: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(k, 0.0, 0.0))
}

/// Six view-projections, one per cube face, for the `k`-th point light.
pub fn faces(k: f32) -> [Mat4; 6] {
    std::array::from_fn(|face| view_proj(k * 10.0 + face as f32))
}

/// What the Flows publish for a frame, and the asset store the meshes are in.
pub struct Scene {
    pub render_world: RenderWorld,
    pub shadows: ShadowView,
    pub runtime: Runtime,
}

impl Scene {
    /// A scene with one mesh on the GPU, at the origin, and no light.
    pub fn new() -> Self {
        let assets = AssetStore::new();
        let uuid = AssetUUID::new();
        let gpu_mesh = AssetHandle::new(GpuMesh {
            vertex_buffer: BufferId(0),
            index_buffer: BufferId(0),
            index_count: 3,
            index_format: IndexFormat::Uint32,
            primitive_topology: PrimitiveTopology::TriangleList,
        });
        assets
            .store::<GpuMesh>()
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(uuid, gpu_mesh.clone());
        let mut runtime = Runtime::new();
        runtime.resources.insert(assets);
        let mut render_world = RenderWorld::new();
        render_world.meshes.push(ExtractedMesh {
            transform: AffineTransform::IDENTITY,
            cpu_mesh_uuid: uuid,
            gpu_mesh,
            material: None,
            gpu_material: None,
            mirrored: false,
        });
        Self {
            render_world,
            shadows: ShadowView::default(),
            runtime,
        }
    }

    fn light(&mut self, light_type: LightType, position: Vec3, matrices: Option<ShadowMatrices>) {
        let index = self.render_world.lights.len();
        self.render_world.lights.push(ExtractedLight {
            light_type,
            position,
            direction: Vec3::new(0.0, -1.0, 0.0),
            shadow_view_proj: Mat4::IDENTITY,
            shadow_atlas_index: None,
        });
        self.shadows.light_count = self.render_world.lights.len();
        if let Some(matrices) = matrices {
            self.shadows.matrices.insert(index, matrices);
        }
    }

    /// The same scene with a shadow-casting directional light, its matrix
    /// `view_proj`.
    pub fn with_sun(mut self, view_proj: Mat4) -> Self {
        self.light(
            LightType::Directional(DirectionalLight {
                shadow_enabled: true,
                ..Default::default()
            }),
            Vec3::new(0.0, 20.0, 0.0),
            Some(ShadowMatrices::Single(view_proj)),
        );
        self
    }

    /// The same scene with a shadow-casting point light at `position`, of
    /// `range`, its six face matrices `faces`.
    pub fn with_lamp(mut self, position: Vec3, range: f32, faces: [Mat4; 6]) -> Self {
        self.light(
            LightType::Point(PointLight {
                range,
                shadow_enabled: true,
                ..Default::default()
            }),
            position,
            Some(ShadowMatrices::Cube(Box::new(faces))),
        );
        self
    }

    /// The same scene with a directional light casting no shadow.
    pub fn with_unshadowed_sun(mut self) -> Self {
        self.light(
            LightType::Directional(DirectionalLight::default()),
            Vec3::new(0.0, 20.0, 0.0),
            None,
        );
        self
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// The shadow pass one record closed, as its slot held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPass {
    pub name: &'static str,
    pub reads: Vec<ResourceId>,
    pub writes: Vec<ResourceId>,
    pub command_buffer: CommandBufferId,
}

impl ClosedPass {
    fn from_slot(slot: ShadowPassSlot) -> Option<Self> {
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
    /// The committed shadow frame; `None` when nothing was committed.
    pub frame: Option<ShadowFrame>,
    /// The committed shadow pass; `None` when the slot holds none.
    pub pass: Option<ClosedPass>,
}

impl Recorded {
    /// Panics unless the lane recorded and its outputs were committed.
    pub fn recorded(&self) -> (&ShadowFrame, &ClosedPass) {
        match &self.run.outcome {
            RunOutcome::Recorded(_) => {}
            other => panic!("the shadows lane did not record: {other:?}"),
        }
        (
            self.frame
                .as_ref()
                .expect("a recorded run commits the shadow frame"),
            self.pass
                .as_ref()
                .expect("a recorded run closes the shadow pass into its slot"),
        )
    }

    /// Whether nothing at all was committed.
    pub fn committed_nothing(&self) -> bool {
        self.frame.is_none() && self.pass.is_none()
    }
}

/// One shadows lane, kept across frames, on a ledger device.
pub struct Shadows {
    lane: Erased<ShadowsLane>,
    pub device: Arc<ShadowLedger>,
    pub pipelines: Arc<LedgerPipelines>,
    frame: u64,
}

impl Shadows {
    /// A lane on a device that makes whatever it is asked to.
    pub fn new() -> Self {
        Self::on(ShadowLedger::new())
    }

    /// A lane on `device`.
    pub fn on(device: ShadowLedger) -> Self {
        Self {
            lane: Erased::new(ShadowsLane::default()),
            device: Arc::new(device),
            pipelines: Arc::new(LedgerPipelines::default()),
            frame: 0,
        }
    }

    /// The lane, erased as its agent holds it.
    pub fn lane(&self) -> &dyn AnyLane {
        &self.lane
    }

    /// Records the next frame at `level` over `scene`.
    pub fn record(&mut self, level: usize, scene: &Scene) -> Recorded {
        self.run(level, scene, true, None)
    }

    /// Records the next frame at `level` over `scene`, its outputs opening on
    /// what `seed` holds this frame.
    pub fn record_seeded(&mut self, level: usize, scene: &Scene, seed: &OutputDeck) -> Recorded {
        self.run(level, scene, true, Some(seed))
    }

    /// Records the next frame at `level` with no `ShadowView` on the bus.
    pub fn record_without_shadow_view(&mut self, level: usize, scene: &Scene) -> Recorded {
        self.run(level, scene, false, None)
    }

    /// Shuts the lane down on its device.
    pub fn shut_down(&mut self) {
        let mut gpu = GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
        self.lane.shutdown(&mut gpu);
    }

    fn run(
        &mut self,
        level: usize,
        scene: &Scene,
        with_view: bool,
        seed: Option<&OutputDeck>,
    ) -> Recorded {
        self.frame += 1;
        let frame = FrameId(self.frame);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(scene.render_world.clone());
        if with_view {
            bus.publish(scene.shadows.clone());
        }
        let mut deck = OutputDeck::new();
        let run = {
            let mut inputs = InputSource::new(frame, &bus, &scene.runtime);
            let mut outputs = OutputSink::new(&mut deck).with_device(self.device.as_ref());
            if let Some(seed) = seed {
                outputs = outputs.with_seed(seed);
            }
            let mut gpu =
                GpuSetup::new(self.device.as_ref()).with_pipelines(self.pipelines.as_ref());
            self.lane.run(level, &mut inputs, &mut outputs, &mut gpu)
        };
        Recorded {
            run,
            frame: deck.get::<ShadowFrame>().cloned(),
            pass: deck
                .contains::<ShadowPassSlot>()
                .then(|| ClosedPass::from_slot(deck.take::<ShadowPassSlot>()))
                .flatten(),
        }
    }
}
