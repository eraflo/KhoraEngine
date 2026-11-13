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
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderVariantKey};
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
    fn copy_buffer_to_buffer(&mut self, _s: &BufferId, _so: u64, _d: &BufferId, _do: u64, _n: u64) {
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
    fn create_bind_group(&self, _d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
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
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
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
