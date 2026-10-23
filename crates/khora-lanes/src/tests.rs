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
