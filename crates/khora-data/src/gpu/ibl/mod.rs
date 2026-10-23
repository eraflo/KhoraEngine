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

//! Image-based lighting (IBL) bake — the data-layer "environment → GPU"
//! projection.
//!
//! [`IblBaker`] runs at startup (driven by the `ibl_bake` DataSystem), and
//! again whenever the environment it was baked from changes, and produces the
//! GPU resources image-based lighting samples:
//! - an environment cubemap (a procedural sky for now, an authored HDR later),
//! - a diffuse irradiance cube (cosine-convolved environment),
//! - and — added in a later increment — a prefiltered specular cube + the
//!   split-sum BRDF LUT (both stand-ins for now).
//!
//! Because the bake is occasional and needs render passes, it records into a
//! **standalone** command encoder ([`GraphicsDevice::create_command_encoder`])
//! and submits it directly, outside the per-frame loop — no lane or agent
//! required. The resulting [`IblGpuBindings`] is stored behind interior
//! mutability (the baker is a shared resource) and consumed by the lit lanes
//! at group 3 via [`IblBaker::bindings`].

use std::sync::{RwLock, RwLockReadGuard};

use bake::bake;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::math::Vec3;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use resources::{normalized_or_default, IblResources};

mod bake;
mod pipelines;
mod resources;

/// Env-cube face resolution. 256² is ample for a smooth procedural sky and as
/// the source the irradiance convolution / specular prefilter sample.
const ENV_FACE_SIZE: u32 = 256;

/// Irradiance cube face resolution. Irradiance is very low-frequency, so a
/// tiny cube captures it (sampled with linear filtering).
const IRRADIANCE_FACE_SIZE: u32 = 32;

/// Linear HDR format for every IBL cube (sky may exceed 1.0 once authored).
const IBL_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Prefiltered specular cube face resolution (mip 0). Higher-roughness mips
/// are progressively smaller.
const PREFILTER_FACE_SIZE: u32 = 128;

/// Number of prefiltered roughness levels (mips). Roughness = mip / (mips-1).
const PREFILTER_MIPS: u32 = 5;

/// Split-sum BRDF integration LUT resolution.
const BRDF_LUT_SIZE: u32 = 512;

const SKY_SHADER: &str = "khora::pipelines::ibl_sky";

const EQUIRECT_SHADER: &str = "khora::pipelines::ibl_equirect";

const EQUIRECT_LAYOUT: &str = "ibl_equirect";

const IRRADIANCE_SHADER: &str = "khora::pipelines::ibl_irradiance";

const PREFILTER_SHADER: &str = "khora::pipelines::ibl_prefilter";

const BRDF_SHADER: &str = "khora::pipelines::ibl_brdf_lut";

const FACE_BASIS_LAYOUT: &str = "ibl_sky_face_basis";

const IRRADIANCE_LAYOUT: &str = "ibl_irradiance_conv";

const PREFILTER_LAYOUT: &str = "ibl_prefilter";

/// Direction **toward** the sun used when the scene has no directional light —
/// a high afternoon sun, so the default environment still reads as a sky.
const DEFAULT_SUN_DIRECTION: Vec3 = Vec3::new(0.35, 0.78, 0.52);

/// Selects the scene's environment source for the IBL bake.
///
/// Registered as a shared resource by the host application. When it names an
/// equirectangular texture asset (an HDR `.hdr`/`.exr` keeps the dynamic range
/// that makes reflections read well), the bake projects it onto the environment
/// cube; absent — or naming an asset that has not been loaded — the procedural
/// sky is baked instead, so a scene without an authored environment still
/// lights correctly.
///
/// The selection is read every tick: selecting another texture, or the
/// selected texture changing in the store, bakes again.
#[derive(Debug, Default, Clone)]
pub struct EnvironmentMap {
    /// Equirectangular environment texture, as a loaded `CpuTexture` asset.
    pub texture: Option<AssetUUID>,
}

impl EnvironmentMap {
    /// Points the environment at an equirectangular texture asset.
    pub fn from_asset(texture: AssetUUID) -> Self {
        Self {
            texture: Some(texture),
        }
    }
}

/// What an IBL bake was made from: the environment the scene selected and the
/// exact texture handle the bake read.
///
/// Two sources are equal when they name the same environment **and** hold the
/// same texture handle — compared by pointer, so a texture replaced in the
/// store under the same uuid makes a different source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IblSource {
    /// The environment texture the scene selected, if any.
    pub environment: Option<AssetUUID>,
    /// The loaded texture read for that environment; `None` bakes the
    /// procedural sky.
    pub texture: Option<AssetHandle<CpuTexture>>,
}

impl IblSource {
    /// The procedural sky: no environment selected, no texture.
    pub fn procedural() -> Self {
        Self::default()
    }
}

/// IBL bake service. Registered as a shared resource at bootstrap and driven by
/// the `ibl_bake` DataSystem, which calls [`ensure_baked`] every frame with the
/// scene's current [`IblSource`]: the bake runs when the source differs from
/// the one the current bake was made from (the first frame, a repainted
/// environment, another environment selected) and is a no-op otherwise.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    /// The current bake and the source it was made from.
    baked: RwLock<Option<(IblSource, IblResources)>>,
    env_wait: std::sync::atomic::AtomicU32,
}

/// How many ticks the bake waits for a selected environment asset to finish
/// loading before falling back to the procedural sky.
///
/// The lit lanes skip rendering entirely until the IBL bindings exist, so
/// waiting forever on an asset that never arrives (a mistyped UUID, a missing
/// file) would leave the screen black. This bounds the wait and logs loudly.
const MAX_ENV_WAIT_TICKS: u32 = 120;

impl IblBaker {
    /// Creates an unbaked baker. The bake happens lazily on the first
    /// [`ensure_baked`](Self::ensure_baked) once a device is available.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current bake, poisoning ignored: it is only ever replaced whole.
    fn read(&self) -> RwLockReadGuard<'_, Option<(IblSource, IblResources)>> {
        self.baked.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether a bake has run.
    pub fn is_baked(&self) -> bool {
        self.read().is_some()
    }

    /// Whether the current bake was made from exactly `source`.
    pub fn is_baked_from(&self, source: &IblSource) -> bool {
        self.read()
            .as_ref()
            .is_some_and(|(baked, _)| baked == source)
    }

    /// A baker marked as baked from `source`, holding resources with no GPU
    /// behind them — for deciding what a tick does without a device.
    #[cfg(test)]
    pub(crate) fn baked_for_test(source: IblSource) -> IblBaker {
        use khora_core::renderer::api::resource::{SamplerId, TextureViewId};
        let resources = IblResources {
            bindings: IblGpuBindings {
                env_cube: TextureViewId(0),
                irradiance_cube: TextureViewId(0),
                prefiltered_cube: TextureViewId(0),
                brdf_lut: TextureViewId(0),
                sampler: SamplerId(0),
            },
            objects: resources::BakeObjects::default(),
        };
        IblBaker {
            baked: RwLock::new(Some((source, resources))),
            env_wait: Default::default(),
        }
    }

    /// Records one tick spent waiting for the scene's environment asset to
    /// load. Returns `true` while the caller should keep waiting, and `false`
    /// once the budget is spent and it must bake the procedural sky instead.
    pub fn wait_for_environment(&self) -> bool {
        let waited = self
            .env_wait
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        waited < MAX_ENV_WAIT_TICKS
    }

    /// Bakes the IBL resources from `source` unless the current bake was made
    /// from it. Safe to call every frame.
    ///
    /// `sun_direction` points **toward** the sun in world space — the scene's
    /// directional light, so the procedural sky's sun disk agrees with the
    /// light that casts the shadows. A zero/degenerate vector falls back to
    /// [`DEFAULT_SUN_DIRECTION`]. It is captured at each bake.
    ///
    /// `source.texture` is an authored equirectangular environment map (see
    /// [`EnvironmentMap`]); `None` bakes the procedural sky instead. Both fill
    /// the same env cube, so the rest of the chain is unaffected.
    ///
    /// A new bake is published before the previous one is freed, and the
    /// environment wait starts over; a bake that fails keeps the previous one.
    pub fn ensure_baked(
        &self,
        device: &dyn GraphicsDevice,
        pipeline_system: &dyn PipelineSystem,
        sun_direction: Vec3,
        source: &IblSource,
    ) {
        if self.is_baked_from(source) {
            return;
        }
        let sun = normalized_or_default(sun_direction);
        match bake(device, pipeline_system, sun, source.texture.as_deref()) {
            Ok(res) => {
                log::info!(
                    "IBL: baked environment ({0}x{0}) + diffuse irradiance ({1}x{1}) cubes, sun=({2:.2}, {3:.2}, {4:.2})",
                    ENV_FACE_SIZE,
                    IRRADIANCE_FACE_SIZE,
                    sun.x,
                    sun.y,
                    sun.z
                );
                let previous = self
                    .baked
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .replace((source.clone(), res));
                if let Some((_, previous)) = previous {
                    previous.destroy(device);
                }
                self.env_wait.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => log::error!("IBL: bake failed: {e:?}"),
        }
    }

    /// Returns the group-3 IBL bindings once baked, else `None` (lit lanes skip
    /// the IBL term until it is ready).
    pub fn bindings(&self) -> Option<IblGpuBindings> {
        self.read().as_ref().map(|(_, res)| res.bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::pipelines::{equirect_layout_entries, irradiance_layout_entries};
    use super::resources::FACE_BASES;
    use super::*;
    use khora_core::renderer::api::command::{BindingType, TextureViewDimension};

    #[test]
    fn face_bases_cover_six_faces_with_unit_axes() {
        assert_eq!(FACE_BASES.len(), 6);
        for basis in &FACE_BASES {
            let f = basis.forward;
            let mag = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
            assert!((mag - 1.0).abs() < 1e-6, "forward must be a unit axis");
        }
    }

    #[test]
    fn unbaked_baker_has_no_bindings() {
        let baker = IblBaker::new();
        assert!(baker.bindings().is_none());
        assert!(!baker.is_baked());
    }

    /// A 1×1 RGBA texture: enough to stand for a loaded environment.
    fn texture() -> AssetHandle<CpuTexture> {
        use khora_core::renderer::api::resource::{SampleCount, TextureDimension, TextureUsage};
        AssetHandle::new(CpuTexture {
            pixels: vec![255; 4],
            size: khora_core::math::Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    #[test]
    fn a_source_equals_itself_and_not_a_replaced_texture() {
        let uuid = AssetUUID::new_v5("test/sky.hdr");
        let first = texture();
        let source = IblSource {
            environment: Some(uuid),
            texture: Some(first.clone()),
        };

        let same = IblSource {
            environment: Some(uuid),
            texture: Some(first.clone()),
        };
        assert_eq!(source, same, "the same uuid and handle are the same source");

        let replaced = IblSource {
            environment: Some(uuid),
            texture: Some(texture()),
        };
        assert_ne!(
            source, replaced,
            "a texture replaced under the same uuid is another source"
        );

        let selected_not_loaded = IblSource {
            environment: Some(uuid),
            texture: None,
        };
        assert_ne!(
            IblSource::procedural(),
            selected_not_loaded,
            "the procedural sky is not a selected environment that never loaded"
        );
        assert_eq!(IblSource::procedural(), IblSource::default());
    }

    #[test]
    fn an_unbaked_baker_is_baked_from_nothing() {
        let baker = IblBaker::new();
        assert!(!baker.is_baked_from(&IblSource::procedural()));
        let uuid = AssetUUID::new_v5("test/sky.hdr");
        assert!(!baker.is_baked_from(&IblSource {
            environment: Some(uuid),
            texture: Some(texture()),
        }));
    }

    #[test]
    fn environment_wait_is_bounded() {
        // A selected-but-never-loaded environment must not stall the bake
        // forever: the lit lanes render nothing until the bindings exist.
        let baker = IblBaker::new();
        for _ in 0..MAX_ENV_WAIT_TICKS {
            assert!(baker.wait_for_environment(), "should still be waiting");
        }
        assert!(
            !baker.wait_for_environment(),
            "must give up and fall back to the procedural sky"
        );
    }

    #[test]
    fn environment_map_defaults_to_procedural() {
        assert!(EnvironmentMap::default().texture.is_none());
        let uuid = AssetUUID::new_v5("test/env.hdr");
        assert_eq!(EnvironmentMap::from_asset(uuid).texture, Some(uuid));
    }

    #[test]
    fn equirect_layout_declares_2d_source_sampler_uniform() {
        // The equirect source is a 2D lat-long map, unlike the cube the
        // convolution and prefilter read at the same binding index.
        let entries = equirect_layout_entries();
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            entries[0].ty,
            BindingType::Texture {
                view_dimension: TextureViewDimension::D2,
                ..
            }
        ));
    }

    #[test]
    fn irradiance_layout_has_cube_sampler_uniform() {
        let entries = irradiance_layout_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].binding, 0);
        assert_eq!(entries[1].binding, 1);
        assert_eq!(entries[2].binding, 2);
    }

    /// A GPU object a bake creates and frees, by kind and id.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Object {
        Texture(usize),
        View(usize),
        Buffer(usize),
        BindGroup(usize),
        Sampler(usize),
    }

    /// A device that gives every object a bake creates a fresh id and records
    /// what it created and what it was asked to destroy.
    #[derive(Debug, Default)]
    struct BakeDevice {
        next: std::sync::atomic::AtomicUsize,
        created: std::sync::Mutex<Vec<Object>>,
        destroyed: std::sync::Mutex<Vec<Object>>,
    }

    impl BakeDevice {
        fn create(&self, make: fn(usize) -> Object) -> usize {
            let id = 1 + self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.created.lock().unwrap().push(make(id));
            id
        }
        fn destroy(&self, object: Object) -> Result<(), ResourceError> {
            self.destroyed.lock().unwrap().push(object);
            Ok(())
        }
        fn created(&self) -> Vec<Object> {
            let mut objects = self.created.lock().unwrap().clone();
            objects.sort();
            objects
        }
        fn destroyed(&self) -> Vec<Object> {
            let mut objects = self.destroyed.lock().unwrap().clone();
            objects.sort();
            objects
        }
        /// The objects created and never destroyed.
        fn alive(&self) -> Vec<Object> {
            let destroyed = self.destroyed();
            self.created()
                .into_iter()
                .filter(|object| !destroyed.contains(object))
                .collect()
        }
    }

    use khora_core::renderer::api::command::{
        BindGroupDescriptor, BindGroupId, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
        BindGroupLayoutId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
    };
    use khora_core::renderer::api::device::GraphicsAdapterInfo;
    use khora_core::renderer::api::pipeline::{
        ComputePipelineDescriptor, ComputePipelineId, ComputePipelineSpec, LayoutKey,
        PipelineLayoutDescriptor, PipelineLayoutId, PipelineSpec, RenderPipelineDescriptor,
        RenderPipelineId,
    };
    use khora_core::renderer::api::resource::{
        BufferDescriptor, BufferId, IndexFormat, SamplerDescriptor, SamplerId, TextureDescriptor,
        TextureId, TextureViewDescriptor, TextureViewId,
    };
    use khora_core::renderer::api::shader::{
        ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey,
    };
    use khora_core::renderer::error::{RenderError, ResourceError};
    use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};

    struct NoPass;

    impl RenderPass<'_> for NoPass {
        fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
        fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
        fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
        fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
        fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
        fn draw_indexed(&mut self, _x: std::ops::Range<u32>, _b: i32, _i: std::ops::Range<u32>) {}
        fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _n: f32, _f: f32) {}
        fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
    }

    impl ComputePass<'_> for NoPass {
        fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
        fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
        fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
    }

    /// An encoder that records nothing: the bake's passes have no GPU here.
    struct NoEncoder;

    impl CommandEncoder for NoEncoder {
        fn begin_render_pass<'e>(
            &'e mut self,
            _d: &RenderPassDescriptor<'e>,
        ) -> Box<dyn RenderPass<'e> + 'e> {
            Box::new(NoPass)
        }
        fn begin_compute_pass<'e>(
            &'e mut self,
            _d: &ComputePassDescriptor<'e>,
        ) -> Box<dyn ComputePass<'e> + 'e> {
            Box::new(NoPass)
        }
        fn begin_profiler_compute_pass<'e>(
            &'e mut self,
            _label: Option<&str>,
            _profiler: &'e dyn GpuProfiler,
            _pass_index: u32,
        ) -> Box<dyn ComputePass<'e> + 'e> {
            Box::new(NoPass)
        }
        fn copy_buffer_to_buffer(
            &mut self,
            _s: &BufferId,
            _so: u64,
            _d: &BufferId,
            _do: u64,
            _n: u64,
        ) {
        }
        fn finish(self: Box<Self>) -> Option<CommandBufferId> {
            Some(CommandBufferId(0))
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    impl GraphicsDevice for BakeDevice {
        fn create_shader_module(
            &self,
            _d: &ShaderModuleDescriptor,
        ) -> Result<ShaderModuleId, ResourceError> {
            unreachable!("the pipeline system builds the shaders")
        }
        fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
            unreachable!()
        }
        fn create_render_pipeline(
            &self,
            _d: &RenderPipelineDescriptor,
        ) -> Result<RenderPipelineId, ResourceError> {
            unreachable!("the pipeline system builds the pipelines")
        }
        fn create_pipeline_layout(
            &self,
            _d: &PipelineLayoutDescriptor,
        ) -> Result<PipelineLayoutId, ResourceError> {
            unreachable!()
        }
        fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
            unreachable!("the bake's pipelines are cached, never freed by it")
        }
        fn create_compute_pipeline(
            &self,
            _d: &ComputePipelineDescriptor,
        ) -> Result<ComputePipelineId, ResourceError> {
            unreachable!()
        }
        fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
            unreachable!()
        }
        fn create_bind_group_layout(
            &self,
            _d: &BindGroupLayoutDescriptor,
        ) -> Result<BindGroupLayoutId, ResourceError> {
            unreachable!("the pipeline system builds the layouts")
        }
        fn create_bind_group(
            &self,
            _d: &BindGroupDescriptor,
        ) -> Result<BindGroupId, ResourceError> {
            Ok(BindGroupId(self.create(Object::BindGroup)))
        }
        fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
            unreachable!("the bake's layouts are cached, never freed by it")
        }
        fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
            self.destroy(Object::BindGroup(id.0))
        }
        fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            Ok(BufferId(self.create(Object::Buffer)))
        }
        fn create_buffer_with_data(
            &self,
            _d: &BufferDescriptor,
            _data: &[u8],
        ) -> Result<BufferId, ResourceError> {
            Ok(BufferId(self.create(Object::Buffer)))
        }
        fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
            self.destroy(Object::Buffer(id.0))
        }
        fn write_buffer(&self, _id: BufferId, _o: u64, _data: &[u8]) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_buffer_async<'a>(
            &'a self,
            _id: BufferId,
            _offset: u64,
            _data: &'a [u8],
        ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static>
        {
            unreachable!()
        }
        fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
            Ok(TextureId(self.create(Object::Texture)))
        }
        fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
            self.destroy(Object::Texture(id.0))
        }
        fn write_texture(
            &self,
            _id: TextureId,
            _data: &[u8],
            _bytes_per_row: Option<u32>,
            _offset: khora_core::math::dimension::Origin3D,
            _size: khora_core::math::dimension::Extent3D,
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_texture_view(
            &self,
            _id: TextureId,
            _d: &TextureViewDescriptor,
        ) -> Result<TextureViewId, ResourceError> {
            Ok(TextureViewId(self.create(Object::View)))
        }
        fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
            self.destroy(Object::View(id.0))
        }
        fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
            Ok(SamplerId(self.create(Object::Sampler)))
        }
        fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
            self.destroy(Object::Sampler(id.0))
        }
        fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
            Box::new(NoEncoder)
        }
        fn submit_command_buffer(&self, _command_buffer: CommandBufferId) {}
        fn get_surface_format(&self) -> Option<TextureFormat> {
            unreachable!()
        }
        fn get_surface_size(&self) -> (u32, u32) {
            unreachable!()
        }
        fn get_adapter_info(&self) -> GraphicsAdapterInfo {
            unreachable!()
        }
        fn supports_feature(&self, _feature_name: &str) -> bool {
            unreachable!()
        }
    }

    /// A pipeline system that hands out cached ids, and fails the pipeline of
    /// one shader when asked to — a shader that does not compile.
    #[derive(Default)]
    struct BakePipelines {
        failing_shader: Option<&'static str>,
    }

    impl PipelineSystem for BakePipelines {
        fn layout(
            &self,
            _device: &dyn GraphicsDevice,
            _key: LayoutKey,
            _variant: &ShaderVariantKey,
        ) -> Result<BindGroupLayoutId, RenderError> {
            unreachable!("the bake uses inline layouts")
        }
        fn inline_layout(
            &self,
            _device: &dyn GraphicsDevice,
            _label: &'static str,
            _entries: &[BindGroupLayoutEntry],
        ) -> Result<BindGroupLayoutId, RenderError> {
            Ok(BindGroupLayoutId(1))
        }
        fn pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            spec: &PipelineSpec,
        ) -> Result<RenderPipelineId, RenderError> {
            if self.failing_shader == Some(spec.shader) {
                return Err(RenderError::Internal(format!(
                    "{} does not compile",
                    spec.shader
                )));
            }
            Ok(RenderPipelineId(1))
        }
        fn compute_pipeline(
            &self,
            _device: &dyn GraphicsDevice,
            _spec: &ComputePipelineSpec,
        ) -> Result<ComputePipelineId, RenderError> {
            unreachable!("the bake has no compute pass")
        }
    }

    fn sky_from(texture: &AssetHandle<CpuTexture>) -> IblSource {
        IblSource {
            environment: Some(AssetUUID::new_v5("test/sky.hdr")),
            texture: Some(texture.clone()),
        }
    }

    #[test]
    fn a_re_bake_frees_exactly_what_the_bake_it_replaces_created() {
        let device = BakeDevice::default();
        let pipelines = BakePipelines::default();
        let baker = IblBaker::new();
        let first = sky_from(&texture());

        baker.ensure_baked(&device, &pipelines, Vec3::ZERO, &first);
        let first_bake = device.created();
        assert!(!first_bake.is_empty());
        assert!(device.destroyed().is_empty(), "a first bake frees nothing");

        baker.ensure_baked(&device, &pipelines, Vec3::ZERO, &first);
        assert_eq!(
            device.created(),
            first_bake,
            "a bake from the same source creates nothing"
        );

        let repainted = sky_from(&texture());
        baker.ensure_baked(&device, &pipelines, Vec3::ZERO, &repainted);
        assert!(baker.is_baked_from(&repainted));
        assert_eq!(
            device.destroyed(),
            first_bake,
            "every object of the replaced bake is freed once, and nothing of the new one"
        );
        let env_cube = baker
            .bindings()
            .expect("the new bake is published")
            .env_cube;
        assert!(
            !first_bake.contains(&Object::View(env_cube.0)),
            "the published env cube is the new bake's"
        );
    }

    #[test]
    fn a_bake_gives_the_environment_wait_back() {
        // The selected environment never loaded: the wait is spent and the
        // procedural sky is baked in its place.
        let device = BakeDevice::default();
        let pipelines = BakePipelines::default();
        let baker = IblBaker::new();
        while baker.wait_for_environment() {}
        let fallback = IblSource {
            environment: Some(AssetUUID::new_v5("test/sky.hdr")),
            texture: None,
        };

        baker.ensure_baked(&device, &pipelines, Vec3::ZERO, &fallback);

        assert!(baker.is_baked_from(&fallback));
        assert!(
            baker.wait_for_environment(),
            "an environment lost after this bake is waited for again"
        );
    }

    #[test]
    fn a_bake_that_fails_frees_what_it_created() {
        let device = BakeDevice::default();
        // The equirect projection's shader does not compile: the bake fails
        // after its cubes, sampler, uniforms and the uploaded source exist.
        let pipelines = BakePipelines {
            failing_shader: Some(EQUIRECT_SHADER),
        };
        let current = sky_from(&texture());
        let baker = IblBaker::baked_for_test(current.clone());

        baker.ensure_baked(&device, &pipelines, Vec3::ZERO, &sky_from(&texture()));

        assert!(baker.is_baked_from(&current), "the current bake stays");
        assert!(
            !device.created().is_empty(),
            "the bake created its targets before failing"
        );
        assert_eq!(
            device.alive(),
            Vec::<Object>::new(),
            "a failed bake leaves nothing behind (the next tick bakes again)"
        );
    }
}
