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

//! Test-only helpers shared by the lanes' unit tests.

use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutId, CommandBufferId,
};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    PipelineSpec, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, SamplerDescriptor, SamplerId, TextureDescriptor, TextureFormat,
    TextureId, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::GraphicsDevice;
use std::future::Future;

/// A device that only answers the introspection a pipeline spec reads (the
/// surface format). Building a spec must never create a GPU resource, so every
/// other call is a test failure.
#[derive(Debug, Default)]
pub(crate) struct SpecOnlyDevice;

const NOT_FOR_SPECS: &str = "building a pipeline spec must not touch the GPU";

impl GraphicsDevice for SpecOnlyDevice {
    fn create_shader_module(
        &self,
        _descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_render_pipeline(
        &self,
        _descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_pipeline_layout(
        &self,
        _descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_compute_pipeline(
        &self,
        _descriptor: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_bind_group_layout(
        &self,
        _descriptor: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_bind_group(
        &self,
        _descriptor: &BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_buffer(&self, _descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_buffer_with_data(
        &self,
        _descriptor: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn write_buffer(&self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn write_buffer_async<'a>(
        &'a self,
        _id: BufferId,
        _offset: u64,
        _data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_texture(&self, _descriptor: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: khora_core::math::dimension::Origin3D,
        _size: khora_core::math::dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_texture_view(
        &self,
        _texture_id: TextureId,
        _descriptor: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_sampler(&self, _descriptor: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {
        unreachable!("{NOT_FOR_SPECS}")
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        None
    }
    fn get_surface_size(&self) -> (u32, u32) {
        (1, 1)
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo::default()
    }
    fn supports_feature(&self, _feature_name: &str) -> bool {
        false
    }
}

/// Asserts that a mesh pipeline binds exactly one vertex buffer laid out as
/// the engine mesh vertex ([`Mesh::VERTEX_ATTRIBUTES`], [`Mesh::VERTEX_STRIDE`]),
/// i.e. the layout [`Mesh::vertex_buffer_layout`] returns.
pub(crate) fn assert_binds_the_mesh_vertex(spec: &PipelineSpec) {
    assert_eq!(
        spec.vertex_buffers.len(),
        1,
        "{}: a mesh pipeline binds one vertex buffer",
        spec.label
    );
    let declared = &spec.vertex_buffers[0];
    assert_eq!(
        declared.array_stride,
        Mesh::VERTEX_STRIDE,
        "{}: vertex stride differs from the mesh vertex",
        spec.label
    );
    assert_eq!(
        declared.attributes.as_ref(),
        Mesh::VERTEX_ATTRIBUTES,
        "{}: vertex attributes differ from the mesh vertex",
        spec.label
    );

    let contract = Mesh::vertex_buffer_layout();
    assert_eq!(
        declared.array_stride, contract.array_stride,
        "{}",
        spec.label
    );
    assert_eq!(declared.step_mode, contract.step_mode, "{}", spec.label);
    assert_eq!(
        declared.attributes.as_ref(),
        contract.attributes.as_ref(),
        "{}",
        spec.label
    );
}

/// Asserts how a lit lane's pipeline culls, given `build(double_sided,
/// mirrored, blend)`: a double-sided material culls nothing, a mirrored draw
/// (negative-determinant model matrix, so its triangles wind clockwise on
/// screen) culls front faces, any other draw culls back faces. The front face
/// stays counter-clockwise in every case: the mirror is handled by the cull
/// mode alone, which is already part of the pipeline cache key.
pub(crate) fn assert_culls_by_sidedness_and_mirroring(
    build: impl Fn(bool, bool, bool) -> PipelineSpec,
) {
    use khora_core::renderer::api::pipeline::enums::{CullMode, FrontFace};

    for blend in [false, true] {
        for (double_sided, mirrored, expected) in [
            (false, false, Some(CullMode::Back)),
            (false, true, Some(CullMode::Front)),
            (true, false, None),
            (true, true, None),
        ] {
            let spec = build(double_sided, mirrored, blend);
            assert_eq!(
                spec.primitive.cull_mode, expected,
                "{}: cull mode for double_sided={double_sided}, mirrored={mirrored}, blend={blend}",
                spec.label
            );
            assert_eq!(
                spec.primitive.front_face,
                FrontFace::Ccw,
                "{}: front face for double_sided={double_sided}, mirrored={mirrored}, blend={blend}",
                spec.label
            );
        }
    }
}

/// Asserts that a mirrored single-sided draw resolves to a different cached
/// pipeline than the plain one, for opaque and blended materials alike, so
/// the per-draw sort by pipeline keeps them apart.
pub(crate) fn assert_mirrored_pipeline_has_its_own_key(
    build: impl Fn(bool, bool, bool) -> PipelineSpec,
) {
    for blend in [false, true] {
        let plain = build(false, false, blend);
        let mirrored = build(false, true, blend);
        assert_ne!(
            plain.key(),
            mirrored.key(),
            "{}: a mirrored draw shares the plain pipeline (blend={blend})",
            plain.label
        );
    }
}

// ─── The GPU ledger: what a lane created, and what it gave back ──────────────

/// One `draw(vertices, instances)` call.
pub(crate) type Draw = (std::ops::Range<u32>, std::ops::Range<u32>);

/// The draws recorded through a [`LedgerDevice`]'s encoders, shared with them.
type DrawLog = std::sync::Arc<std::sync::Mutex<Vec<Draw>>>;

/// The kind of device object a [`LedgerDevice`] hands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum GpuKind {
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

/// First id a [`LedgerPipelines`] hands out. The device counts from 1, so an
/// id at or above this one was never created by the device: it belongs to the
/// `PipelineSystem`.
const PIPELINE_SYSTEM_IDS: usize = 1_000_000;

/// A device that keeps a ledger: every object it creates (kind, id, label),
/// every destroy it receives, and every draw recorded through its encoders.
#[derive(Debug, Default)]
pub(crate) struct LedgerDevice {
    next_id: std::sync::atomic::AtomicUsize,
    created: std::sync::Mutex<Vec<(GpuKind, usize, String)>>,
    destroyed: std::sync::Mutex<Vec<(GpuKind, usize)>>,
    draws: DrawLog,
}

impl LedgerDevice {
    fn create(&self, kind: GpuKind, label: Option<&str>) -> usize {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.created.lock().expect("ledger lock").push((
            kind,
            id,
            label.unwrap_or_default().to_owned(),
        ));
        id
    }

    fn destroy(&self, kind: GpuKind, id: usize) -> Result<(), ResourceError> {
        self.destroyed.lock().expect("ledger lock").push((kind, id));
        Ok(())
    }

    /// Every object created so far.
    pub(crate) fn created(&self) -> Vec<(GpuKind, usize, String)> {
        self.created.lock().expect("ledger lock").clone()
    }

    /// Every destroy received so far.
    pub(crate) fn destroyed(&self) -> Vec<(GpuKind, usize)> {
        self.destroyed.lock().expect("ledger lock").clone()
    }

    /// Every draw recorded through this device's encoders.
    pub(crate) fn draws(&self) -> Vec<Draw> {
        self.draws.lock().expect("ledger lock").clone()
    }
}

/// An encoder that records the draws of its render passes into the device's ledger.
pub(crate) struct LedgerEncoder {
    draws: DrawLog,
}

struct LedgerRenderPass {
    draws: DrawLog,
}

struct LedgerComputePass;

impl khora_core::renderer::traits::RenderPass<'_> for LedgerRenderPass {
    fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
    fn set_index_buffer(
        &mut self,
        _b: &BufferId,
        _o: u64,
        _f: khora_core::renderer::api::resource::IndexFormat,
    ) {
    }
    fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        self.draws
            .lock()
            .expect("ledger lock")
            .push((vertices, instances));
    }
    fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {}
    fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
    fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
}

impl khora_core::renderer::traits::ComputePass<'_> for LedgerComputePass {
    fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for LedgerEncoder {
    fn begin_render_pass<'enc>(
        &'enc mut self,
        _desc: &khora_core::renderer::api::command::RenderPassDescriptor<'enc>,
    ) -> Box<dyn khora_core::renderer::traits::RenderPass<'enc> + 'enc> {
        Box::new(LedgerRenderPass {
            draws: std::sync::Arc::clone(&self.draws),
        })
    }
    fn begin_compute_pass<'enc>(
        &'enc mut self,
        _desc: &khora_core::renderer::api::command::ComputePassDescriptor<'enc>,
    ) -> Box<dyn khora_core::renderer::traits::ComputePass<'enc> + 'enc> {
        Box::new(LedgerComputePass)
    }
    fn begin_profiler_compute_pass<'enc>(
        &'enc mut self,
        _label: Option<&str>,
        _profiler: &'enc dyn khora_core::renderer::traits::GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn khora_core::renderer::traits::ComputePass<'enc> + 'enc> {
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
        Some(CommandBufferId(0))
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl GraphicsDevice for LedgerDevice {
    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(
            self.create(GpuKind::ShaderModule, descriptor.label),
        ))
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ShaderModule, id.0)
    }
    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(self.create(
            GpuKind::RenderPipeline,
            descriptor.label.as_deref(),
        )))
    }
    fn create_pipeline_layout(
        &self,
        descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(self.create(
            GpuKind::PipelineLayout,
            descriptor.label.as_deref(),
        )))
    }
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::RenderPipeline, id.0)
    }
    fn create_compute_pipeline(
        &self,
        descriptor: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(
            self.create(GpuKind::ComputePipeline, descriptor.label.as_deref()) as u64,
        ))
    }
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::ComputePipeline, id.0 as usize)
    }
    fn create_bind_group_layout(
        &self,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(
            self.create(GpuKind::BindGroupLayout, descriptor.label),
        ))
    }
    fn create_bind_group(
        &self,
        descriptor: &BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError> {
        Ok(BindGroupId(
            self.create(GpuKind::BindGroup, descriptor.label),
        ))
    }
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroupLayout, id.0)
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::BindGroup, id.0)
    }
    fn create_buffer(&self, descriptor: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        Ok(BufferId(
            self.create(GpuKind::Buffer, descriptor.label.as_deref()),
        ))
    }
    fn create_buffer_with_data(
        &self,
        descriptor: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.create_buffer(descriptor)
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
    fn create_texture(&self, descriptor: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        Ok(TextureId(
            self.create(GpuKind::Texture, descriptor.label.as_deref()),
        ))
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Texture, id.0)
    }
    fn write_texture(
        &self,
        _texture_id: TextureId,
        _data: &[u8],
        _bytes_per_row: Option<u32>,
        _offset: khora_core::math::dimension::Origin3D,
        _size: khora_core::math::dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        Ok(())
    }
    fn create_texture_view(
        &self,
        _texture_id: TextureId,
        descriptor: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        Ok(TextureViewId(
            self.create(GpuKind::TextureView, descriptor.label.as_deref()),
        ))
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::TextureView, id.0)
    }
    fn create_sampler(&self, descriptor: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(
            self.create(GpuKind::Sampler, descriptor.label.as_deref()),
        ))
    }
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
        self.destroy(GpuKind::Sampler, id.0)
    }
    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        Box::new(LedgerEncoder {
            draws: std::sync::Arc::clone(&self.draws),
        })
    }
    fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {}
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

/// A `PipelineSystem` that answers every request with a fresh id at or above
/// [`PIPELINE_SYSTEM_IDS`], so its objects can never be mistaken for the
/// device's. It owns them: a lane must never destroy one.
#[derive(Debug, Default)]
pub(crate) struct LedgerPipelines {
    next_id: std::sync::atomic::AtomicUsize,
}

impl LedgerPipelines {
    fn next(&self) -> usize {
        PIPELINE_SYSTEM_IDS
            + self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

impl khora_core::renderer::traits::PipelineSystem for LedgerPipelines {
    fn layout(
        &self,
        _device: &dyn GraphicsDevice,
        _key: khora_core::renderer::api::pipeline::LayoutKey,
        _variant: &khora_core::renderer::api::shader::ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, khora_core::renderer::error::RenderError> {
        Ok(BindGroupLayoutId(self.next()))
    }
    fn inline_layout(
        &self,
        _device: &dyn GraphicsDevice,
        _label: &'static str,
        _entries: &[khora_core::renderer::api::command::BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, khora_core::renderer::error::RenderError> {
        Ok(BindGroupLayoutId(self.next()))
    }
    fn pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, khora_core::renderer::error::RenderError> {
        Ok(RenderPipelineId(self.next()))
    }
    fn compute_pipeline(
        &self,
        _device: &dyn GraphicsDevice,
        _spec: &khora_core::renderer::api::pipeline::ComputePipelineSpec,
    ) -> Result<ComputePipelineId, khora_core::renderer::error::RenderError> {
        Ok(ComputePipelineId(self.next() as u64))
    }
}

/// A lane context carrying `device` and `pipelines`, the two things every GPU
/// lane initialises and shuts down with.
pub(crate) fn ledger_context(
    device: &std::sync::Arc<LedgerDevice>,
    pipelines: &std::sync::Arc<LedgerPipelines>,
) -> khora_core::lane::LaneContext<'static> {
    let mut ctx = khora_core::lane::LaneContext::new();
    ctx.insert(std::sync::Arc::clone(device) as std::sync::Arc<dyn GraphicsDevice>);
    ctx.insert(std::sync::Arc::clone(pipelines)
        as std::sync::Arc<dyn khora_core::renderer::traits::PipelineSystem>);
    ctx
}

/// Initialises `lane` on a ledger device, shuts it down, and asserts the
/// shutdown gave back **exactly** what the lane created through the device:
///
/// - every buffer, texture, view, sampler and bind group created is destroyed;
/// - nothing is destroyed that the device did not create — in particular no
///   layout or pipeline handed out by the `PipelineSystem`, which owns and
///   shares them across lanes;
/// - nothing is destroyed twice.
pub(crate) fn assert_shutdown_destroys_what_it_created(lane: &dyn khora_core::lane::Lane) {
    let device = std::sync::Arc::new(LedgerDevice::default());
    let pipelines = std::sync::Arc::new(LedgerPipelines::default());
    let name = lane.strategy_name();

    lane.on_initialize(&mut ledger_context(&device, &pipelines))
        .unwrap_or_else(|e| panic!("{name}: initialises on a ledger device: {e}"));
    let created = device.created();
    let owned: Vec<&(GpuKind, usize, String)> = created
        .iter()
        .filter(|(kind, _, _)| {
            matches!(
                kind,
                GpuKind::Buffer
                    | GpuKind::Texture
                    | GpuKind::TextureView
                    | GpuKind::Sampler
                    | GpuKind::BindGroup
            )
        })
        .collect();
    assert!(
        owned.iter().any(|(kind, _, _)| *kind == GpuKind::Buffer),
        "{name}: creates no buffer at initialisation, so its shutdown proves nothing"
    );

    lane.on_shutdown(&mut ledger_context(&device, &pipelines));
    let destroyed = device.destroyed();

    let leaked: Vec<String> = owned
        .iter()
        .filter(|(kind, id, _)| !destroyed.contains(&(*kind, *id)))
        .map(|(kind, id, label)| format!("{kind:?} #{id} {label:?}"))
        .collect();
    assert!(
        leaked.is_empty(),
        "{name}: on_shutdown leaves {} of the {} objects it created alive:\n  {}",
        leaked.len(),
        owned.len(),
        leaked.join("\n  ")
    );

    let foreign: Vec<String> = destroyed
        .iter()
        .filter(|(kind, id)| {
            !created
                .iter()
                .any(|(created_kind, created_id, _)| created_kind == kind && created_id == id)
        })
        .map(|(kind, id)| {
            let owner = if *id >= PIPELINE_SYSTEM_IDS {
                "the PipelineSystem's"
            } else {
                "never created"
            };
            format!("{kind:?} #{id} ({owner})")
        })
        .collect();
    assert!(
        foreign.is_empty(),
        "{name}: on_shutdown destroys what it does not own:\n  {}",
        foreign.join("\n  ")
    );

    let mut seen = std::collections::HashSet::new();
    let twice: Vec<String> = destroyed
        .iter()
        .filter(|entry| !seen.insert(**entry))
        .map(|(kind, id)| format!("{kind:?} #{id}"))
        .collect();
    assert!(
        twice.is_empty(),
        "{name}: on_shutdown destroys the same object twice: {twice:?}"
    );
}

// ─── A lit lane reads the shadow frame; it never writes the deck ─────────────

/// Draws an empty frame with `lane` (initialised on a ledger device) against a
/// deck that holds, or not, the frame's `ShadowFrame`, and asserts the lane
/// wrote no deck slot: it only *reads* another agent's output, so the
/// scheduler's write check must not count it as a writer — and it must not
/// conjure a `ShadowFrame` into a deck that held none.
pub(crate) fn assert_reading_the_shadow_frame_writes_no_deck_slot(
    lane: &dyn khora_core::lane::Lane,
) {
    use khora_core::lane::{
        ClearColor, ColorTarget, ColorTargetSize, DepthTarget, LaneContext, OutputDeck,
    };
    use khora_core::math::{Extent2D, LinearRgba};
    use khora_core::renderer::api::gpu_scene::GpuMesh;
    use khora_core::renderer::api::shadow::ShadowFrame;
    use khora_data::assets::Assets;
    use khora_data::render::RenderWorld;
    use std::sync::{Arc, RwLock};

    let name = lane.strategy_name();
    let device = Arc::new(LedgerDevice::default());
    let pipelines = Arc::new(LedgerPipelines::default());
    lane.on_initialize(&mut ledger_context(&device, &pipelines))
        .unwrap_or_else(|e| panic!("{name}: initialises on a ledger device: {e}"));

    for shadow_published in [false, true] {
        let mut deck = OutputDeck::new();
        if shadow_published {
            let _ = deck.slot::<ShadowFrame>();
        }
        let _ = deck.drain_written();

        let gpu_meshes = Arc::new(RwLock::new(Assets::<GpuMesh>::new()));
        let render_world = RenderWorld::default();
        let mut encoder: Box<dyn CommandEncoder> = device.create_command_encoder(None);
        {
            let mut ctx = LaneContext::new();
            ctx.insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);
            ctx.insert(
                Arc::clone(&pipelines) as Arc<dyn khora_core::renderer::traits::PipelineSystem>
            );
            ctx.insert(gpu_meshes);
            ctx.insert_slot(encoder.as_mut());
            ctx.insert_ref(&render_world);
            ctx.insert(ColorTarget(TextureViewId(1)));
            ctx.insert(DepthTarget(TextureViewId(2)));
            ctx.insert(ClearColor(LinearRgba::BLACK));
            ctx.insert(ColorTargetSize(Extent2D {
                width: 800,
                height: 600,
            }));
            ctx.insert_slot(&mut deck);
            lane.execute(&mut ctx)
                .unwrap_or_else(|e| panic!("{name}: draws an empty frame: {e}"));
        }

        let written = deck.drain_written();
        assert!(
            written.is_empty(),
            "{name}: reading the ShadowFrame (published: {shadow_published}) recorded deck \
             writes {written:?}"
        );
        assert_eq!(
            deck.contains::<ShadowFrame>(),
            shadow_published,
            "{name}: the deck's ShadowFrame must be exactly what the shadow agent published"
        );
    }
}
