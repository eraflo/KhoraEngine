use khora_agents::render_agent::{RenderAgent, RenderingStrategy};
use khora_core::math::{LinearRgba, Vec3};
use khora_core::renderer::api::*;
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight};
use khora_core::renderer::traits::{CommandEncoder, ComputePass, GraphicsDevice, RenderPass};
use khora_core::renderer::{
    BindGroupId, BufferId, ComputePipelineId, GraphicsBackendType, IndexFormat, PipelineLayoutId,
    RenderPipelineId, RendererDeviceType, ResourceError, ShaderModuleId, TextureFormat,
};
use khora_data::ecs::{GlobalTransform, Light, Transform, World};
use std::any::Any;
use std::future::Future;
use std::ops::Range;

#[derive(Debug)]
struct MockGraphicsDevice;

struct MockCommandEncoder;
struct MockRenderPass;
struct MockComputePass;

impl RenderPass<'_> for MockRenderPass {
    fn set_pipeline(&mut self, _pipeline: &RenderPipelineId) {}
    fn set_bind_group(&mut self, _index: u32, _bind_group: &BindGroupId, _offsets: &[u32]) {}
    fn set_vertex_buffer(&mut self, _slot: u32, _buffer: &BufferId, _offset: u64) {}
    fn set_index_buffer(&mut self, _buffer: &BufferId, _offset: u64, _index_format: IndexFormat) {}
    fn draw(&mut self, _vertices: Range<u32>, _instances: Range<u32>) {}
    fn draw_indexed(&mut self, _indices: Range<u32>, _base_vertex: i32, _instances: Range<u32>) {}
}

impl ComputePass<'_> for MockComputePass {
    fn set_pipeline(&mut self, _pipeline: &ComputePipelineId) {}
    fn set_bind_group(&mut self, _index: u32, _bind_group: &BindGroupId, _offsets: &[u32]) {}
    fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
}

impl CommandEncoder for MockCommandEncoder {
    fn begin_render_pass<'encoder>(
        &'encoder mut self,
        _desc: &RenderPassDescriptor<'encoder>,
    ) -> Box<dyn RenderPass<'encoder> + 'encoder> {
        Box::new(MockRenderPass)
    }

    fn begin_compute_pass<'encoder>(
        &'encoder mut self,
        _desc: &ComputePassDescriptor<'encoder>,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder> {
        Box::new(MockComputePass)
    }

    fn begin_profiler_compute_pass<'encoder>(
        &'encoder mut self,
        _label: Option<&str>,
        _profiler: &'encoder dyn khora_core::renderer::traits::GpuProfiler,
        _pass_index: u32,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder> {
        Box::new(MockComputePass)
    }

    fn copy_buffer_to_buffer(
        &mut self,
        _source: &BufferId,
        _source_offset: u64,
        _destination: &BufferId,
        _destination_offset: u64,
        _size: u64,
    ) {
    }

    fn finish(self: Box<Self>) -> khora_core::renderer::api::command::CommandBufferId {
        khora_core::renderer::api::command::CommandBufferId(0)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GraphicsDevice for MockGraphicsDevice {
    fn create_shader_module(
        &self,
        _desc: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        Ok(ShaderModuleId(0))
    }

    fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_render_pipeline(
        &self,
        _desc: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        Ok(RenderPipelineId(0))
    }

    fn create_pipeline_layout(
        &self,
        _desc: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        Ok(PipelineLayoutId(0))
    }

    fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_compute_pipeline(
        &self,
        _desc: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        Ok(ComputePipelineId(0))
    }

    fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        Ok(BindGroupLayoutId(0))
    }

    fn create_bind_group(&self, _desc: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        Ok(BindGroupId(0))
    }

    fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_buffer(&self, _desc: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        Ok(BufferId(0))
    }

    fn create_buffer_with_data(
        &self,
        _desc: &BufferDescriptor,
        _data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        Ok(BufferId(0))
    }

    fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
        Ok(())
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

    fn create_texture(&self, _desc: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        Ok(TextureId(0))
    }

    fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
        Ok(())
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
        _desc: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        Ok(TextureViewId(0))
    }

    fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_sampler(&self, _desc: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        Ok(SamplerId(0))
    }

    fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
        Ok(())
    }

    fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
        Box::new(MockCommandEncoder)
    }

    fn submit_command_buffer(
        &self,
        _command_buffer: khora_core::renderer::api::command::CommandBufferId,
    ) {
    }

    fn get_surface_format(&self) -> Option<TextureFormat> {
        None
    }

    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        GraphicsAdapterInfo {
            name: "Mock".to_string(),
            backend_type: GraphicsBackendType::Unknown,
            device_type: RendererDeviceType::Unknown,
        }
    }

    fn supports_feature(&self, _feature: &str) -> bool {
        true
    }
}

#[test]
fn test_render_agent_strategy_selection() {
    let device = MockGraphicsDevice;

    // Setup RenderAgent
    let mut agent = RenderAgent::new();

    // Case 1: No lights -> SimpleUnlit
    let mut world = World::new();
    agent.prepare_frame(&mut world, &device);
    let lane = agent.select_lane();
    assert_eq!(lane.strategy_name(), "SimpleUnlit");

    // Case 2: One directional light -> LitForward
    let mut world = World::new();

    world.spawn((
        Light::new(LightType::Directional(DirectionalLight {
            direction: Vec3::new(0.0, -1.0, 0.0),
            color: LinearRgba::WHITE,
            intensity: 1.0,
        })),
        Transform::default(),
        GlobalTransform::default(),
    ));

    agent.prepare_frame(&mut world, &device);
    let lane = agent.select_lane();
    assert_eq!(lane.strategy_name(), "LitForward");

    // Case 3: Many point lights -> ForwardPlus
    let mut world = World::new();

    // Current threshold is 20
    for i in 0..25 {
        world.spawn((
            Light::new(LightType::Point(PointLight {
                color: LinearRgba::WHITE,
                intensity: 10.0,
                range: 5.0,
            })),
            Transform {
                translation: Vec3::new(i as f32, 0.0, 0.0),
                ..Default::default()
            },
            GlobalTransform::default(),
        ));
    }

    agent.prepare_frame(&mut world, &device);
    let lane = agent.select_lane();
    assert_eq!(lane.strategy_name(), "ForwardPlus");

    // Case 4: Forced strategy
    agent.set_strategy(RenderingStrategy::LitForward);
    let lane = agent.select_lane();
    assert_eq!(lane.strategy_name(), "LitForward");
}
