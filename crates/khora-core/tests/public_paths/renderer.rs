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

//! Compile-level guard over `khora_core::renderer` (the rest of the crate is
//! in `public_paths.rs` and its siblings).
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, statics, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), trait
//! items, and the std / serde / bytemuck / operator trait impls. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions, bound forwarding for traits, `ptr::eq`
//! for statics).
//!
//! The list was generated from `khora_core`'s rustdoc JSON, so it is complete
//! for the tree it was written against. Nothing is constructed; the tests only
//! have to type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_asset<T: khora_core::asset::Asset>() {}
fn is_bit_and<T: std::ops::BitAnd>() {}
fn is_bit_and_assign<T: std::ops::BitAndAssign>() {}
fn is_bit_or<T: std::ops::BitOr>() {}
fn is_bit_or_assign<T: std::ops::BitOrAssign>() {}
fn is_bit_xor<T: std::ops::BitXor>() {}
fn is_bit_xor_assign<T: std::ops::BitXorAssign>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_display<T: std::fmt::Display>() {}
fn is_eq<T: Eq>() {}
fn is_error<T: std::error::Error>() {}
fn is_from_pipeline_error<T: From<khora_core::renderer::error::PipelineError>>() {}
fn is_from_resource_error<T: From<khora_core::renderer::error::ResourceError>>() {}
fn is_from_shader_error<T: From<khora_core::renderer::error::ShaderError>>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_not<T: std::ops::Not>() {}
fn is_ord<T: Ord>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_partial_ord<T: PartialOrd>() {}
fn is_pod<T: bytemuck::Pod>() {}
fn is_send<T: Send>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_sync<T: Sync>() {}
fn is_zeroable<T: bytemuck::Zeroable>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_core::renderer as _;
    use khora_core::renderer::api as _;
    use khora_core::renderer::api::command as _;
    use khora_core::renderer::api::command::bind_group as _;
    use khora_core::renderer::api::command::draw_command as _;
    use khora_core::renderer::api::command::pass as _;
    use khora_core::renderer::api::device as _;
    use khora_core::renderer::api::device::adapter as _;
    use khora_core::renderer::api::device::backend as _;
    use khora_core::renderer::api::device::gpu_hook as _;
    use khora_core::renderer::api::device::settings as _;
    use khora_core::renderer::api::device::stats as _;
    use khora_core::renderer::api::frame as _;
    use khora_core::renderer::api::frame::frame_context as _;
    use khora_core::renderer::api::frame::render_context as _;
    use khora_core::renderer::api::gpu_scene as _;
    use khora_core::renderer::api::gpu_scene::mesh as _;
    use khora_core::renderer::api::gpu_scene::model_uniforms as _;
    use khora_core::renderer::api::gpu_scene::render_object as _;
    use khora_core::renderer::api::ibl as _;
    use khora_core::renderer::api::ibl::bindings as _;
    use khora_core::renderer::api::ibl::bindings::offset as _;
    use khora_core::renderer::api::material as _;
    use khora_core::renderer::api::material::bindings as _;
    use khora_core::renderer::api::material::bindings::binding as _;
    use khora_core::renderer::api::material::bindings::flag as _;
    use khora_core::renderer::api::material::gpu_material as _;
    use khora_core::renderer::api::material::uniforms as _;
    use khora_core::renderer::api::pipeline as _;
    use khora_core::renderer::api::pipeline::compute as _;
    use khora_core::renderer::api::pipeline::descriptor as _;
    use khora_core::renderer::api::pipeline::enums as _;
    use khora_core::renderer::api::pipeline::layout as _;
    use khora_core::renderer::api::pipeline::spec as _;
    use khora_core::renderer::api::pipeline::state as _;
    use khora_core::renderer::api::resource as _;
    use khora_core::renderer::api::resource::buffer as _;
    use khora_core::renderer::api::resource::texture as _;
    use khora_core::renderer::api::resource::texture_format as _;
    use khora_core::renderer::api::resource::view as _;
    use khora_core::renderer::api::shader as _;
    use khora_core::renderer::api::shader::defs as _;
    use khora_core::renderer::api::shader::module as _;
    use khora_core::renderer::api::shader::source as _;
    use khora_core::renderer::api::shader::stage as _;
    use khora_core::renderer::api::shader::variant as _;
    use khora_core::renderer::api::shadow as _;
    use khora_core::renderer::api::shadow::bindings as _;
    use khora_core::renderer::api::shadow::bindings::binding as _;
    use khora_core::renderer::api::text as _;
    use khora_core::renderer::api::util as _;
    use khora_core::renderer::api::util::flags as _;
    use khora_core::renderer::api::util::half_float as _;
    use khora_core::renderer::error as _;
    use khora_core::renderer::light as _;
    use khora_core::renderer::light::forward_plus as _;
    use khora_core::renderer::light::uniforms as _;
    use khora_core::renderer::traits as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn bind_group_descriptor_fields(
    x: &khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>,
) {
    let _ = (&x.label, &x.layout, &x.entries);
}

fn bind_group_entry_fields(
    x: &khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>,
) {
    let _ = (&x.binding, &x.resource, &x._phantom);
}

fn bind_group_id_fields(x: &khora_core::renderer::api::command::bind_group::BindGroupId) {
    let _ = (&x.0,);
}

fn bind_group_layout_descriptor_fields(
    x: &khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>,
) {
    let _ = (&x.label, &x.entries);
}

fn bind_group_layout_entry_fields(
    x: &khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry,
) {
    let _ = (&x.binding, &x.visibility, &x.ty);
}

fn bind_group_layout_id_fields(
    x: &khora_core::renderer::api::command::bind_group::BindGroupLayoutId,
) {
    let _ = (&x.0,);
}

fn binding_resource_variants(x: &khora_core::renderer::api::command::bind_group::BindingResource) {
    match x {
        khora_core::renderer::api::command::bind_group::BindingResource::Buffer(..) => {}
        khora_core::renderer::api::command::bind_group::BindingResource::TextureView(..) => {}
        khora_core::renderer::api::command::bind_group::BindingResource::Sampler(..) => {}
    }
}

fn binding_type_variants(x: &khora_core::renderer::api::command::bind_group::BindingType) {
    match x {
        khora_core::renderer::api::command::bind_group::BindingType::Buffer { .. } => {}
        khora_core::renderer::api::command::bind_group::BindingType::Texture { .. } => {}
        khora_core::renderer::api::command::bind_group::BindingType::Sampler(..) => {}
    }
}

fn buffer_binding_fields(x: &khora_core::renderer::api::command::bind_group::BufferBinding) {
    let _ = (&x.buffer, &x.offset, &x.size);
}

fn buffer_binding_type_variants(
    x: &khora_core::renderer::api::command::bind_group::BufferBindingType,
) {
    match x {
        khora_core::renderer::api::command::bind_group::BufferBindingType::Uniform => {}
        khora_core::renderer::api::command::bind_group::BufferBindingType::Storage { .. } => {}
    }
}

fn sampler_binding_type_variants(
    x: &khora_core::renderer::api::command::bind_group::SamplerBindingType,
) {
    match x {
        khora_core::renderer::api::command::bind_group::SamplerBindingType::Filtering => {}
        khora_core::renderer::api::command::bind_group::SamplerBindingType::NonFiltering => {}
        khora_core::renderer::api::command::bind_group::SamplerBindingType::Comparison => {}
    }
}

fn texture_sample_type_variants(
    x: &khora_core::renderer::api::command::bind_group::TextureSampleType,
) {
    match x {
        khora_core::renderer::api::command::bind_group::TextureSampleType::Float { .. } => {}
        khora_core::renderer::api::command::bind_group::TextureSampleType::Depth => {}
        khora_core::renderer::api::command::bind_group::TextureSampleType::Uint => {}
        khora_core::renderer::api::command::bind_group::TextureSampleType::Sint => {}
    }
}

fn compute_pipeline_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>,
) {
    let _ = (&x.label, &x.layout, &x.shader_module, &x.entry_point);
}

fn compute_pipeline_id_fields(x: &khora_core::renderer::api::pipeline::compute::ComputePipelineId) {
    let _ = (&x.0,);
}

fn command_buffer_id_fields(x: &khora_core::renderer::api::command::draw_command::CommandBufferId) {
    let _ = (&x.0,);
}

fn draw_command_fields(x: &khora_core::renderer::api::command::draw_command::DrawCommand) {
    let _ = (
        &x.pipeline,
        &x.vertex_buffer,
        &x.index_buffer,
        &x.index_format,
        &x.index_count,
        &x.model_bind_group,
        &x.model_offset,
        &x.material_bind_group,
        &x.material_offset,
    );
}

fn compute_pass_descriptor_fields(
    x: &khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>,
) {
    let _ = (&x.label, &x.timestamp_writes);
}

fn load_op_variants(x: &khora_core::renderer::api::command::pass::LoadOp<f32>) {
    match x {
        khora_core::renderer::api::command::pass::LoadOp::Load => {}
        khora_core::renderer::api::command::pass::LoadOp::Clear(..) => {}
    }
}

fn operations_fields(x: &khora_core::renderer::api::command::pass::Operations<f32>) {
    let _ = (&x.load, &x.store);
}

fn pass_timestamp_writes_fields(
    x: &khora_core::renderer::api::command::pass::PassTimestampWrites<'static>,
) {
    let _ = (&x.beginning_of_pass_hook, &x.end_of_pass_hook);
}

fn render_pass_color_attachment_fields(
    x: &khora_core::renderer::api::command::pass::RenderPassColorAttachment<'static>,
) {
    let _ = (
        &x.view,
        &x.resolve_target,
        &x.ops,
        &x.base_array_layer,
        &x.base_mip_level,
    );
}

fn render_pass_depth_stencil_attachment_fields(
    x: &khora_core::renderer::api::command::pass::RenderPassDepthStencilAttachment<'static>,
) {
    let _ = (&x.view, &x.depth_ops, &x.stencil_ops, &x.base_array_layer);
}

fn render_pass_descriptor_fields(
    x: &khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>,
) {
    let _ = (&x.label, &x.color_attachments, &x.depth_stencil_attachment);
}

fn store_op_variants(x: &khora_core::renderer::api::command::pass::StoreOp) {
    match x {
        khora_core::renderer::api::command::pass::StoreOp::Store => {}
        khora_core::renderer::api::command::pass::StoreOp::Discard => {}
    }
}

fn graphics_adapter_info_fields(
    x: &khora_core::renderer::api::device::adapter::GraphicsAdapterInfo,
) {
    let _ = (&x.name, &x.backend_type, &x.device_type);
}

fn backend_selection_config_fields(
    x: &khora_core::renderer::api::device::backend::BackendSelectionConfig,
) {
    let _ = (&x.preferred_backends, &x.timeout, &x.prefer_discrete_gpu);
}

fn backend_selection_result_fields(
    x: &khora_core::renderer::api::device::backend::BackendSelectionResult<u32>,
) {
    let _ = (
        &x.adapter,
        &x.adapter_info,
        &x.selection_time_ms,
        &x.attempted_backends,
    );
}

fn render_context_fields(
    x: &khora_core::renderer::api::frame::render_context::RenderContext<'static>,
) {
    let _ = (
        &x.color_target,
        &x.depth_target,
        &x.clear_color,
        &x.shadow_atlas,
        &x.shadow_cube_atlas,
        &x.shadow_sampler,
    );
}

fn gpu_hook_variants(x: &khora_core::renderer::api::device::gpu_hook::GpuHook) {
    match x {
        khora_core::renderer::api::device::gpu_hook::GpuHook::FrameStart => {}
        khora_core::renderer::api::device::gpu_hook::GpuHook::MainPassBegin => {}
        khora_core::renderer::api::device::gpu_hook::GpuHook::MainPassEnd => {}
        khora_core::renderer::api::device::gpu_hook::GpuHook::FrameEnd => {}
    }
}

fn render_settings_fields(x: &khora_core::renderer::api::device::settings::RenderSettings) {
    let _ = (
        &x.strategy,
        &x.quality_level,
        &x.show_wireframe,
        &x.resize_debounce_ms,
        &x.resize_max_pending_frames,
        &x.enable_gpu_timestamps,
    );
}

fn shader_module_descriptor_fields(
    x: &khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>,
) {
    let _ = (&x.label, &x.source);
}

fn shader_module_id_fields(x: &khora_core::renderer::api::shader::module::ShaderModuleId) {
    let _ = (&x.0,);
}

fn shader_source_data_variants(
    x: &khora_core::renderer::api::shader::module::ShaderSourceData<'static>,
) {
    match x {
        khora_core::renderer::api::shader::module::ShaderSourceData::Wgsl(..) => {}
    }
}

fn render_stats_fields(x: &khora_core::renderer::api::device::stats::RenderStats) {
    let _ = (
        &x.frame_number,
        &x.cpu_preparation_time_ms,
        &x.cpu_render_submission_time_ms,
        &x.gpu_main_pass_time_ms,
        &x.gpu_frame_total_time_ms,
        &x.draw_calls,
        &x.triangles_rendered,
        &x.vram_usage_estimate_mb,
    );
}

fn ibl_gpu_bindings_fields(x: &khora_core::renderer::api::ibl::bindings::IblGpuBindings) {
    let _ = (
        &x.env_cube,
        &x.irradiance_cube,
        &x.prefiltered_cube,
        &x.brdf_lut,
        &x.sampler,
    );
}

fn material_gpu_bindings_fields(
    x: &khora_core::renderer::api::material::bindings::MaterialGpuBindings,
) {
    let _ = (
        &x.uniform_buffer,
        &x.base_color,
        &x.metallic_roughness,
        &x.normal,
        &x.emissive,
        &x.occlusion,
        &x.sampler,
    );
}

fn multisample_state_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor,
) {
    let _ = (&x.count, &x.mask, &x.alpha_to_coverage_enabled);
}

fn render_pipeline_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>,
) {
    let _ = (
        &x.label,
        &x.vertex_shader_module,
        &x.vertex_entry_point,
        &x.fragment_shader_module,
        &x.fragment_entry_point,
        &x.vertex_buffers_layout,
        &x.layout,
        &x.primitive_state,
        &x.depth_stencil_state,
        &x.color_target_states,
        &x.multisample_state,
    );
}

fn render_pipeline_id_fields(
    x: &khora_core::renderer::api::pipeline::descriptor::RenderPipelineId,
) {
    let _ = (&x.0,);
}

fn blend_factor_variants(x: &khora_core::renderer::api::pipeline::enums::BlendFactor) {
    match x {
        khora_core::renderer::api::pipeline::enums::BlendFactor::Zero => {}
        khora_core::renderer::api::pipeline::enums::BlendFactor::One => {}
        khora_core::renderer::api::pipeline::enums::BlendFactor::SrcAlpha => {}
        khora_core::renderer::api::pipeline::enums::BlendFactor::OneMinusSrcAlpha => {}
    }
}

fn blend_operation_variants(x: &khora_core::renderer::api::pipeline::enums::BlendOperation) {
    match x {
        khora_core::renderer::api::pipeline::enums::BlendOperation::Add => {}
        khora_core::renderer::api::pipeline::enums::BlendOperation::Subtract => {}
        khora_core::renderer::api::pipeline::enums::BlendOperation::ReverseSubtract => {}
        khora_core::renderer::api::pipeline::enums::BlendOperation::Min => {}
        khora_core::renderer::api::pipeline::enums::BlendOperation::Max => {}
    }
}

fn compare_function_variants(x: &khora_core::renderer::api::pipeline::enums::CompareFunction) {
    match x {
        khora_core::renderer::api::pipeline::enums::CompareFunction::Never => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::Less => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::Equal => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::LessEqual => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::Greater => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::NotEqual => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::GreaterEqual => {}
        khora_core::renderer::api::pipeline::enums::CompareFunction::Always => {}
    }
}

fn cull_mode_variants(x: &khora_core::renderer::api::pipeline::enums::CullMode) {
    match x {
        khora_core::renderer::api::pipeline::enums::CullMode::None => {}
        khora_core::renderer::api::pipeline::enums::CullMode::Front => {}
        khora_core::renderer::api::pipeline::enums::CullMode::Back => {}
    }
}

fn front_face_variants(x: &khora_core::renderer::api::pipeline::enums::FrontFace) {
    match x {
        khora_core::renderer::api::pipeline::enums::FrontFace::Ccw => {}
        khora_core::renderer::api::pipeline::enums::FrontFace::Cw => {}
    }
}

fn polygon_mode_variants(x: &khora_core::renderer::api::pipeline::enums::PolygonMode) {
    match x {
        khora_core::renderer::api::pipeline::enums::PolygonMode::Fill => {}
        khora_core::renderer::api::pipeline::enums::PolygonMode::Line => {}
        khora_core::renderer::api::pipeline::enums::PolygonMode::Point => {}
    }
}

fn primitive_topology_variants(x: &khora_core::renderer::api::pipeline::enums::PrimitiveTopology) {
    match x {
        khora_core::renderer::api::pipeline::enums::PrimitiveTopology::PointList => {}
        khora_core::renderer::api::pipeline::enums::PrimitiveTopology::LineList => {}
        khora_core::renderer::api::pipeline::enums::PrimitiveTopology::LineStrip => {}
        khora_core::renderer::api::pipeline::enums::PrimitiveTopology::TriangleList => {}
        khora_core::renderer::api::pipeline::enums::PrimitiveTopology::TriangleStrip => {}
    }
}

fn stencil_operation_variants(x: &khora_core::renderer::api::pipeline::enums::StencilOperation) {
    match x {
        khora_core::renderer::api::pipeline::enums::StencilOperation::Keep => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::Zero => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::Replace => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::Invert => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::IncrementClamp => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::DecrementClamp => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::IncrementWrap => {}
        khora_core::renderer::api::pipeline::enums::StencilOperation::DecrementWrap => {}
    }
}

fn vertex_format_variants(x: &khora_core::renderer::api::pipeline::enums::VertexFormat) {
    match x {
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint8x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint8x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint8x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint8x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Unorm8x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Unorm8x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Snorm8x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Snorm8x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint16x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint16x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint16x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint16x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Unorm16x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Unorm16x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Snorm16x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Snorm16x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float16x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float16x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float32 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float32x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float32x3 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Float32x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint32 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint32x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint32x3 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Uint32x4 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint32 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint32x2 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint32x3 => {}
        khora_core::renderer::api::pipeline::enums::VertexFormat::Sint32x4 => {}
    }
}

fn vertex_step_mode_variants(x: &khora_core::renderer::api::pipeline::enums::VertexStepMode) {
    match x {
        khora_core::renderer::api::pipeline::enums::VertexStepMode::Vertex => {}
        khora_core::renderer::api::pipeline::enums::VertexStepMode::Instance => {}
    }
}

fn pipeline_layout_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>,
) {
    let _ = (&x.label, &x.bind_group_layouts);
}

fn pipeline_layout_id_fields(x: &khora_core::renderer::api::pipeline::layout::PipelineLayoutId) {
    let _ = (&x.0,);
}

fn compute_pipeline_key_fields(x: &khora_core::renderer::api::pipeline::spec::ComputePipelineKey) {
    let _ = (&x.shader, &x.variant);
}

fn compute_pipeline_spec_fields(
    x: &khora_core::renderer::api::pipeline::spec::ComputePipelineSpec,
) {
    let _ = (
        &x.label,
        &x.shader,
        &x.variant,
        &x.bind_group_layouts,
        &x.entry_point,
    );
}

fn layout_cache_key_variants(x: &khora_core::renderer::api::pipeline::spec::LayoutCacheKey) {
    match x {
        khora_core::renderer::api::pipeline::spec::LayoutCacheKey::Named(..) => {}
        khora_core::renderer::api::pipeline::spec::LayoutCacheKey::Inline(..) => {}
    }
}

fn layout_key_variants(x: &khora_core::renderer::api::pipeline::spec::LayoutKey) {
    match x {
        khora_core::renderer::api::pipeline::spec::LayoutKey::Camera => {}
        khora_core::renderer::api::pipeline::spec::LayoutKey::Model => {}
        khora_core::renderer::api::pipeline::spec::LayoutKey::Material => {}
        khora_core::renderer::api::pipeline::spec::LayoutKey::Lighting => {}
        khora_core::renderer::api::pipeline::spec::LayoutKey::LightingBuffer => {}
    }
}

fn layout_spec_variants(x: &khora_core::renderer::api::pipeline::spec::LayoutSpec) {
    match x {
        khora_core::renderer::api::pipeline::spec::LayoutSpec::Named(..) => {}
        khora_core::renderer::api::pipeline::spec::LayoutSpec::Inline { .. } => {}
    }
}

fn pipeline_key_fields(x: &khora_core::renderer::api::pipeline::spec::PipelineKey) {
    let _ = (
        &x.shader,
        &x.variant,
        &x.color_format,
        &x.cull_mode,
        &x.blend,
    );
}

fn pipeline_spec_fields(x: &khora_core::renderer::api::pipeline::spec::PipelineSpec) {
    let _ = (
        &x.label,
        &x.shader,
        &x.variant,
        &x.bind_group_layouts,
        &x.vertex_buffers,
        &x.vs_entry,
        &x.fs_entry,
        &x.primitive,
        &x.depth_stencil,
        &x.color_targets,
        &x.multisample,
    );
}

fn shader_def_scalar_variants(x: &khora_core::renderer::api::shader::variant::ShaderDefScalar) {
    match x {
        khora_core::renderer::api::shader::variant::ShaderDefScalar::Bool(..) => {}
        khora_core::renderer::api::shader::variant::ShaderDefScalar::Int(..) => {}
        khora_core::renderer::api::shader::variant::ShaderDefScalar::UInt(..) => {}
    }
}

fn blend_component_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::BlendComponentDescriptor,
) {
    let _ = (&x.src_factor, &x.dst_factor, &x.operation);
}

fn blend_state_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::BlendStateDescriptor,
) {
    let _ = (&x.color, &x.alpha);
}

fn color_target_state_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor,
) {
    let _ = (&x.format, &x.blend, &x.write_mask);
}

fn depth_bias_state_fields(x: &khora_core::renderer::api::pipeline::state::DepthBiasState) {
    let _ = (&x.constant, &x.slope_scale, &x.clamp);
}

fn depth_stencil_state_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor,
) {
    let _ = (
        &x.format,
        &x.depth_write_enabled,
        &x.depth_compare,
        &x.stencil_front,
        &x.stencil_back,
        &x.stencil_read_mask,
        &x.stencil_write_mask,
        &x.bias,
    );
}

fn primitive_state_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor,
) {
    let _ = (
        &x.topology,
        &x.strip_index_format,
        &x.front_face,
        &x.cull_mode,
        &x.polygon_mode,
        &x.unclipped_depth,
        &x.conservative,
    );
}

fn stencil_face_state_fields(x: &khora_core::renderer::api::pipeline::state::StencilFaceState) {
    let _ = (&x.compare, &x.fail_op, &x.depth_fail_op, &x.depth_pass_op);
}

fn vertex_attribute_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor,
) {
    let _ = (&x.shader_location, &x.format, &x.offset);
}

fn vertex_buffer_layout_descriptor_fields(
    x: &khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>,
) {
    let _ = (&x.array_stride, &x.step_mode, &x.attributes);
}

fn buffer_descriptor_fields(
    x: &khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>,
) {
    let _ = (&x.label, &x.size, &x.usage, &x.mapped_at_creation);
}

fn buffer_id_fields(x: &khora_core::renderer::api::resource::buffer::BufferId) {
    let _ = (&x.0,);
}

fn cpu_shader_source_fields(x: &khora_core::renderer::api::shader::source::CpuShaderSource) {
    let _ = (&x.0,);
}

fn address_mode_variants(x: &khora_core::renderer::api::resource::texture::AddressMode) {
    match x {
        khora_core::renderer::api::resource::texture::AddressMode::Repeat => {}
        khora_core::renderer::api::resource::texture::AddressMode::ClampToEdge => {}
        khora_core::renderer::api::resource::texture::AddressMode::MirrorRepeat => {}
        khora_core::renderer::api::resource::texture::AddressMode::ClampToBorder => {}
    }
}

fn cpu_texture_fields(x: &khora_core::renderer::api::resource::texture::CpuTexture) {
    let _ = (
        &x.pixels,
        &x.size,
        &x.format,
        &x.mip_level_count,
        &x.sample_count,
        &x.dimension,
        &x.usage,
    );
}

fn filter_mode_variants(x: &khora_core::renderer::api::resource::texture::FilterMode) {
    match x {
        khora_core::renderer::api::resource::texture::FilterMode::Nearest => {}
        khora_core::renderer::api::resource::texture::FilterMode::Linear => {}
    }
}

fn image_aspect_variants(x: &khora_core::renderer::api::resource::texture::ImageAspect) {
    match x {
        khora_core::renderer::api::resource::texture::ImageAspect::All => {}
        khora_core::renderer::api::resource::texture::ImageAspect::StencilOnly => {}
        khora_core::renderer::api::resource::texture::ImageAspect::DepthOnly => {}
    }
}

fn mipmap_filter_mode_variants(x: &khora_core::renderer::api::resource::texture::MipmapFilterMode) {
    match x {
        khora_core::renderer::api::resource::texture::MipmapFilterMode::Nearest => {}
        khora_core::renderer::api::resource::texture::MipmapFilterMode::Linear => {}
    }
}

fn sampler_border_color_variants(
    x: &khora_core::renderer::api::resource::texture::SamplerBorderColor,
) {
    match x {
        khora_core::renderer::api::resource::texture::SamplerBorderColor::TransparentBlack => {}
        khora_core::renderer::api::resource::texture::SamplerBorderColor::OpaqueBlack => {}
        khora_core::renderer::api::resource::texture::SamplerBorderColor::OpaqueWhite => {}
    }
}

fn sampler_descriptor_fields(
    x: &khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>,
) {
    let _ = (
        &x.label,
        &x.address_mode_u,
        &x.address_mode_v,
        &x.address_mode_w,
        &x.mag_filter,
        &x.min_filter,
        &x.mipmap_filter,
        &x.lod_min_clamp,
        &x.lod_max_clamp,
        &x.compare,
        &x.anisotropy_clamp,
        &x.border_color,
    );
}

fn sampler_id_fields(x: &khora_core::renderer::api::resource::texture::SamplerId) {
    let _ = (&x.0,);
}

fn texture_descriptor_fields(
    x: &khora_core::renderer::api::resource::texture::TextureDescriptor<'static>,
) {
    let _ = (
        &x.label,
        &x.size,
        &x.mip_level_count,
        &x.sample_count,
        &x.dimension,
        &x.format,
        &x.usage,
        &x.view_formats,
    );
}

fn texture_dimension_variants(x: &khora_core::renderer::api::resource::texture::TextureDimension) {
    match x {
        khora_core::renderer::api::resource::texture::TextureDimension::D1 => {}
        khora_core::renderer::api::resource::texture::TextureDimension::D2 => {}
        khora_core::renderer::api::resource::texture::TextureDimension::D3 => {}
    }
}

fn texture_id_fields(x: &khora_core::renderer::api::resource::texture::TextureId) {
    let _ = (&x.0,);
}

fn texture_view_descriptor_fields(
    x: &khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>,
) {
    let _ = (
        &x.label,
        &x.format,
        &x.dimension,
        &x.aspect,
        &x.base_mip_level,
        &x.mip_level_count,
        &x.base_array_layer,
        &x.array_layer_count,
    );
}

fn texture_view_dimension_variants(
    x: &khora_core::renderer::api::resource::texture::TextureViewDimension,
) {
    match x {
        khora_core::renderer::api::resource::texture::TextureViewDimension::D1 => {}
        khora_core::renderer::api::resource::texture::TextureViewDimension::D2 => {}
        khora_core::renderer::api::resource::texture::TextureViewDimension::D2Array => {}
        khora_core::renderer::api::resource::texture::TextureViewDimension::Cube => {}
        khora_core::renderer::api::resource::texture::TextureViewDimension::CubeArray => {}
        khora_core::renderer::api::resource::texture::TextureViewDimension::D3 => {}
    }
}

fn texture_view_id_fields(x: &khora_core::renderer::api::resource::texture::TextureViewId) {
    let _ = (&x.0,);
}

fn camera_uniform_data_fields(x: &khora_core::renderer::api::resource::view::CameraUniformData) {
    let _ = (&x.view_projection, &x.camera_position);
}

fn view_info_fields(x: &khora_core::renderer::api::resource::view::ViewInfo) {
    let _ = (&x.view_matrix, &x.projection_matrix, &x.camera_position);
}

fn gpu_material_fields(x: &khora_core::renderer::api::material::gpu_material::GpuMaterial) {
    let _ = (
        &x.uniform_buffer,
        &x.base_color_view,
        &x.metallic_roughness_view,
        &x.normal_view,
        &x.emissive_view,
        &x.occlusion_view,
        &x.base_color_texture,
        &x.metallic_roughness_texture,
        &x.normal_texture,
        &x.emissive_texture,
        &x.occlusion_texture,
        &x.sampler,
        &x.bind_group,
        &x.variant,
        &x.double_sided,
        &x.blend,
    );
}

fn culling_uniforms_data_fields(x: &khora_core::renderer::light::uniforms::CullingUniformsData) {
    let _ = (
        &x.view_projection,
        &x.inverse_projection,
        &x.screen_dimensions,
        &x.tile_count,
        &x.num_lights,
        &x.tile_size,
        &x._padding,
    );
}

fn directional_light_uniform_fields(
    x: &khora_core::renderer::light::uniforms::DirectionalLightUniform,
) {
    let _ = (
        &x.direction,
        &x.color,
        &x.shadow_view_proj,
        &x.shadow_params,
    );
}

fn lighting_uniforms_fields(x: &khora_core::renderer::light::uniforms::LightingUniforms) {
    let _ = (
        &x.directional_lights,
        &x.point_lights,
        &x.spot_lights,
        &x.num_directional_lights,
        &x.num_point_lights,
        &x.num_spot_lights,
        &x._padding,
    );
}

fn point_light_uniform_fields(x: &khora_core::renderer::light::uniforms::PointLightUniform) {
    let _ = (&x.position, &x.color, &x.shadow_params);
}

fn spot_light_uniform_fields(x: &khora_core::renderer::light::uniforms::SpotLightUniform) {
    let _ = (
        &x.position,
        &x.direction,
        &x.color,
        &x.params,
        &x.shadow_view_proj,
        &x.shadow_params,
    );
}

fn material_uniforms_fields(x: &khora_core::renderer::api::material::uniforms::MaterialUniforms) {
    let _ = (&x.base_color, &x.emissive, &x.ambient, &x.pbr_factors);
}

fn model_uniforms_fields(x: &khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms) {
    let _ = (&x.model_matrix, &x.normal_matrix);
}

fn gpu_mesh_fields(x: &khora_core::renderer::api::gpu_scene::mesh::GpuMesh) {
    let _ = (
        &x.vertex_buffer,
        &x.index_buffer,
        &x.index_count,
        &x.index_format,
        &x.primitive_topology,
    );
}

fn mesh_fields(x: &khora_core::renderer::api::gpu_scene::mesh::Mesh) {
    let _ = (
        &x.positions,
        &x.normals,
        &x.tex_coords,
        &x.tangents,
        &x.colors,
        &x.indices,
        &x.primitive_type,
        &x.bounding_box,
        &x.vertex_layout,
    );
}

fn render_object_fields(x: &khora_core::renderer::api::gpu_scene::render_object::RenderObject) {
    let _ = (
        &x.pipeline,
        &x.vertex_buffer,
        &x.index_buffer,
        &x.index_count,
    );
}

fn shadow_entries_fields(x: &khora_core::renderer::api::shadow::ShadowEntries) {
    let _ = (&x.0,);
}

fn shadow_entry_variants(x: &khora_core::renderer::api::shadow::ShadowEntry) {
    match x {
        khora_core::renderer::api::shadow::ShadowEntry::Atlas2D { .. } => {}
        khora_core::renderer::api::shadow::ShadowEntry::Cube { .. } => {}
    }
}

fn shadow_frame_fields(x: &khora_core::renderer::api::shadow::ShadowFrame) {
    let _ = (&x.bindings, &x.entries);
}

fn shadow_gpu_bindings_fields(x: &khora_core::renderer::api::shadow::bindings::ShadowGpuBindings) {
    let _ = (&x.atlas_2d, &x.atlas_cube, &x.sampler);
}

fn atlas_rect_fields(x: &khora_core::renderer::api::util::AtlasRect) {
    let _ = (&x.min, &x.max);
}

fn graphics_backend_type_variants(x: &khora_core::renderer::api::device::GraphicsBackendType) {
    match x {
        khora_core::renderer::api::device::GraphicsBackendType::Vulkan => {}
        khora_core::renderer::api::device::GraphicsBackendType::Metal => {}
        khora_core::renderer::api::device::GraphicsBackendType::Dx12 => {}
        khora_core::renderer::api::device::GraphicsBackendType::Dx11 => {}
        khora_core::renderer::api::device::GraphicsBackendType::OpenGL => {}
        khora_core::renderer::api::device::GraphicsBackendType::WebGpu => {}
        khora_core::renderer::api::device::GraphicsBackendType::Unknown => {}
    }
}

fn index_format_variants(x: &khora_core::renderer::api::resource::IndexFormat) {
    match x {
        khora_core::renderer::api::resource::IndexFormat::Uint16 => {}
        khora_core::renderer::api::resource::IndexFormat::Uint32 => {}
    }
}

fn render_strategy_variants(x: &khora_core::renderer::api::device::RenderStrategy) {
    match x {
        khora_core::renderer::api::device::RenderStrategy::Forward => {}
        khora_core::renderer::api::device::RenderStrategy::Deferred => {}
        khora_core::renderer::api::device::RenderStrategy::Custom(..) => {}
    }
}

fn renderer_device_type_variants(x: &khora_core::renderer::api::device::RendererDeviceType) {
    match x {
        khora_core::renderer::api::device::RendererDeviceType::IntegratedGpu => {}
        khora_core::renderer::api::device::RendererDeviceType::DiscreteGpu => {}
        khora_core::renderer::api::device::RendererDeviceType::VirtualGpu => {}
        khora_core::renderer::api::device::RendererDeviceType::Cpu => {}
        khora_core::renderer::api::device::RendererDeviceType::Unknown => {}
    }
}

fn sample_count_variants(x: &khora_core::renderer::api::resource::SampleCount) {
    match x {
        khora_core::renderer::api::resource::SampleCount::X1 => {}
        khora_core::renderer::api::resource::SampleCount::X2 => {}
        khora_core::renderer::api::resource::SampleCount::X4 => {}
        khora_core::renderer::api::resource::SampleCount::X8 => {}
        khora_core::renderer::api::resource::SampleCount::X16 => {}
        khora_core::renderer::api::resource::SampleCount::X32 => {}
        khora_core::renderer::api::resource::SampleCount::X64 => {}
    }
}

fn shader_stage_variants(x: &khora_core::renderer::api::shader::ShaderStage) {
    match x {
        khora_core::renderer::api::shader::ShaderStage::Vertex => {}
        khora_core::renderer::api::shader::ShaderStage::Fragment => {}
        khora_core::renderer::api::shader::ShaderStage::Compute => {}
    }
}

fn texture_color_space_variants(x: &khora_core::renderer::api::resource::TextureColorSpace) {
    match x {
        khora_core::renderer::api::resource::TextureColorSpace::Srgb => {}
        khora_core::renderer::api::resource::TextureColorSpace::Linear => {}
    }
}

fn texture_format_variants(x: &khora_core::renderer::api::resource::TextureFormat) {
    match x {
        khora_core::renderer::api::resource::TextureFormat::R8Unorm => {}
        khora_core::renderer::api::resource::TextureFormat::Rg8Unorm => {}
        khora_core::renderer::api::resource::TextureFormat::Rgba8Unorm => {}
        khora_core::renderer::api::resource::TextureFormat::Rgba8UnormSrgb => {}
        khora_core::renderer::api::resource::TextureFormat::Bgra8UnormSrgb => {}
        khora_core::renderer::api::resource::TextureFormat::R16Float => {}
        khora_core::renderer::api::resource::TextureFormat::Rg16Float => {}
        khora_core::renderer::api::resource::TextureFormat::Rgba16Float => {}
        khora_core::renderer::api::resource::TextureFormat::R32Float => {}
        khora_core::renderer::api::resource::TextureFormat::Rg32Float => {}
        khora_core::renderer::api::resource::TextureFormat::Rgba32Float => {}
        khora_core::renderer::api::resource::TextureFormat::Depth16Unorm => {}
        khora_core::renderer::api::resource::TextureFormat::Depth24Plus => {}
        khora_core::renderer::api::resource::TextureFormat::Depth24PlusStencil8 => {}
        khora_core::renderer::api::resource::TextureFormat::Depth32Float => {}
        khora_core::renderer::api::resource::TextureFormat::Depth32FloatStencil8 => {}
    }
}

fn pipeline_error_variants(x: &khora_core::renderer::error::PipelineError) {
    match x {
        khora_core::renderer::error::PipelineError::LayoutCreationFailed(..) => {}
        khora_core::renderer::error::PipelineError::CompilationFailed { .. } => {}
        khora_core::renderer::error::PipelineError::InvalidShaderModuleForPipeline { .. } => {}
        khora_core::renderer::error::PipelineError::InvalidRenderPipeline { .. } => {}
        khora_core::renderer::error::PipelineError::MissingEntryPointForFragmentShader {
            ..
        } => {}
        khora_core::renderer::error::PipelineError::IncompatibleColorTarget(..) => {}
        khora_core::renderer::error::PipelineError::IncompatibleDepthStencilFormat(..) => {}
        khora_core::renderer::error::PipelineError::FeatureNotSupported(..) => {}
    }
}

fn render_error_variants(x: &khora_core::renderer::error::RenderError) {
    match x {
        khora_core::renderer::error::RenderError::NotInitialized => {}
        khora_core::renderer::error::RenderError::InitializationFailed(..) => {}
        khora_core::renderer::error::RenderError::SurfaceAcquisitionFailed(..) => {}
        khora_core::renderer::error::RenderError::RenderingFailed(..) => {}
        khora_core::renderer::error::RenderError::ResourceError(..) => {}
        khora_core::renderer::error::RenderError::DeviceLost => {}
        khora_core::renderer::error::RenderError::DeviceOutOfMemory(..) => {}
        khora_core::renderer::error::RenderError::Internal(..) => {}
    }
}

fn resource_error_variants(x: &khora_core::renderer::error::ResourceError) {
    match x {
        khora_core::renderer::error::ResourceError::Shader(..) => {}
        khora_core::renderer::error::ResourceError::Pipeline(..) => {}
        khora_core::renderer::error::ResourceError::NotFound => {}
        khora_core::renderer::error::ResourceError::InvalidHandle => {}
        khora_core::renderer::error::ResourceError::BackendError(..) => {}
        khora_core::renderer::error::ResourceError::OutOfBounds => {}
    }
}

fn shader_error_variants(x: &khora_core::renderer::error::ShaderError) {
    match x {
        khora_core::renderer::error::ShaderError::LoadError { .. } => {}
        khora_core::renderer::error::ShaderError::CompilationError { .. } => {}
        khora_core::renderer::error::ShaderError::NotFound { .. } => {}
        khora_core::renderer::error::ShaderError::InvalidEntryPoint { .. } => {}
    }
}

fn forward_plus_tile_config_fields(
    x: &khora_core::renderer::light::forward_plus::ForwardPlusTileConfig,
) {
    let _ = (&x.tile_size, &x.max_lights_per_tile, &x.use_depth_prepass);
}

fn gpu_light_fields(x: &khora_core::renderer::light::forward_plus::GpuLight) {
    let _ = (
        &x.position,
        &x.range,
        &x.color,
        &x.intensity,
        &x.direction,
        &x.light_type,
        &x.inner_cone_cos,
        &x.outer_cone_cos,
        &x.shadow_map_index,
        &x.shadow_bias,
        &x.shadow_normal_bias,
        &x.shadow_far_plane,
    );
}

fn tile_size_variants(x: &khora_core::renderer::light::forward_plus::TileSize) {
    match x {
        khora_core::renderer::light::forward_plus::TileSize::X16 => {}
        khora_core::renderer::light::forward_plus::TileSize::X32 => {}
    }
}

fn directional_light_fields(x: &khora_core::renderer::light::DirectionalLight) {
    let _ = (
        &x.direction,
        &x.color,
        &x.intensity,
        &x.shadow_enabled,
        &x.shadow_bias,
        &x.shadow_normal_bias,
    );
}

fn light_type_variants(x: &khora_core::renderer::light::LightType) {
    match x {
        khora_core::renderer::light::LightType::Directional(..) => {}
        khora_core::renderer::light::LightType::Point(..) => {}
        khora_core::renderer::light::LightType::Spot(..) => {}
    }
}

fn point_light_fields(x: &khora_core::renderer::light::PointLight) {
    let _ = (
        &x.color,
        &x.intensity,
        &x.range,
        &x.shadow_enabled,
        &x.shadow_bias,
        &x.shadow_normal_bias,
    );
}

fn spot_light_fields(x: &khora_core::renderer::light::SpotLight) {
    let _ = (
        &x.direction,
        &x.color,
        &x.intensity,
        &x.range,
        &x.inner_cone_angle,
        &x.outer_cone_angle,
        &x.shadow_enabled,
        &x.shadow_bias,
        &x.shadow_normal_bias,
    );
}

fn frame_targets_fields(x: &khora_core::renderer::traits::FrameTargets) {
    let _ = (&x.color, &x.depth);
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    use std::any::type_name;

    fn graphics_device_trait_items<T: khora_core::renderer::GraphicsDevice>() {
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_shader_module;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_shader_module;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_render_pipeline;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_pipeline_layout;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_render_pipeline;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_compute_pipeline;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_compute_pipeline;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_bind_group_layout;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_bind_group;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_bind_group_layout;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_bind_group;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_buffer;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_buffer_with_data;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_buffer;
        let _ = <T as khora_core::renderer::GraphicsDevice>::write_buffer;
        let _ = <T as khora_core::renderer::GraphicsDevice>::write_buffer_async;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_texture;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_texture;
        let _ = <T as khora_core::renderer::GraphicsDevice>::write_texture;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_texture_view;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_texture_view;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_sampler;
        let _ = <T as khora_core::renderer::GraphicsDevice>::destroy_sampler;
        let _ = <T as khora_core::renderer::GraphicsDevice>::create_command_encoder;
        let _ = <T as khora_core::renderer::GraphicsDevice>::submit_command_buffer;
        let _ = <T as khora_core::renderer::GraphicsDevice>::get_surface_format;
        let _ = <T as khora_core::renderer::GraphicsDevice>::get_surface_size;
        let _ = <T as khora_core::renderer::GraphicsDevice>::get_adapter_info;
        let _ = <T as khora_core::renderer::GraphicsDevice>::supports_feature;
    }

    fn graphics_device_trait_identity<T: khora_core::renderer::traits::GraphicsDevice>() {
        graphics_device_trait_items::<T>();
    }

    fn graphics_device_trait_identity_rev<T: khora_core::renderer::GraphicsDevice>() {
        graphics_device_trait_identity::<T>();
    }

    fn render_system_trait_items<T: khora_core::renderer::RenderSystem>() {
        let _ = <T as khora_core::renderer::RenderSystem>::init;
        let _ = <T as khora_core::renderer::RenderSystem>::resize;
        let _ = <T as khora_core::renderer::RenderSystem>::prepare_frame;
        let _ = <T as khora_core::renderer::RenderSystem>::render;
        let _ = <T as khora_core::renderer::RenderSystem>::get_last_frame_stats;
        let _ = <T as khora_core::renderer::RenderSystem>::supports_feature;
        let _ = <T as khora_core::renderer::RenderSystem>::get_adapter_info;
        let _ = <T as khora_core::renderer::RenderSystem>::graphics_device;
        let _ = <T as khora_core::renderer::RenderSystem>::begin_frame;
        let _ = <T as khora_core::renderer::RenderSystem>::end_frame;
        let _ = <T as khora_core::renderer::RenderSystem>::render_overlay;
        let _ = <T as khora_core::renderer::RenderSystem>::shutdown;
        let _ = <T as khora_core::renderer::RenderSystem>::as_any;
        let _ = <T as khora_core::renderer::RenderSystem>::as_any_mut;
        let _ = <T as khora_core::renderer::RenderSystem>::render_to_viewport;
        let _ = <T as khora_core::renderer::RenderSystem>::set_render_to_viewport;
    }

    fn render_system_trait_identity<T: khora_core::renderer::traits::RenderSystem>() {
        render_system_trait_items::<T>();
    }

    fn render_system_trait_identity_rev<T: khora_core::renderer::RenderSystem>() {
        render_system_trait_identity::<T>();
    }

    fn text_layout_trait_items<T: khora_core::renderer::api::text::TextLayout>() {
        let _ = <T as khora_core::renderer::api::text::TextLayout>::size;
        let _ = <T as khora_core::renderer::api::text::TextLayout>::as_any;
    }

    fn text_renderer_trait_items<T: khora_core::renderer::api::text::TextRenderer>() {
        let _ = <T as khora_core::renderer::api::text::TextRenderer>::layout_text;
        let _ = <T as khora_core::renderer::api::text::TextRenderer>::queue_text;
        let _ = <T as khora_core::renderer::api::text::TextRenderer>::flush;
    }

    fn command_encoder_trait_items<T: khora_core::renderer::traits::CommandEncoder>() {
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::begin_render_pass;
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::begin_compute_pass;
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::begin_profiler_compute_pass;
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::copy_buffer_to_buffer;
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::finish;
        let _ = <T as khora_core::renderer::traits::CommandEncoder>::as_any_mut;
    }

    fn compute_pass_trait_items<T: khora_core::renderer::traits::ComputePass<'static>>() {
        let _ = <T as khora_core::renderer::traits::ComputePass<'static>>::set_pipeline;
        let _ = <T as khora_core::renderer::traits::ComputePass<'static>>::set_bind_group;
        let _ = <T as khora_core::renderer::traits::ComputePass<'static>>::dispatch_workgroups;
    }

    fn gpu_profiler_trait_items<T: khora_core::renderer::traits::GpuProfiler>() {
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::try_read_previous_frame;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::resolve_and_copy;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::copy_to_staging;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::schedule_map_after_submit;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::last_main_pass_ms;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::last_frame_total_ms;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::as_any;
        let _ = <T as khora_core::renderer::traits::GpuProfiler>::as_any_mut;
    }

    fn graphics_backend_selector_trait_items<
        T: khora_core::renderer::traits::GraphicsBackendSelector<u32>,
    >() {
        let _ =
            type_name::<<T as khora_core::renderer::traits::GraphicsBackendSelector<u32>>::Error>();
        let _ = <T as khora_core::renderer::traits::GraphicsBackendSelector<u32>>::select_backend;
        let _ = <T as khora_core::renderer::traits::GraphicsBackendSelector<u32>>::list_adapters;
        let _ =
            <T as khora_core::renderer::traits::GraphicsBackendSelector<u32>>::is_backend_supported;
    }

    fn pipeline_system_trait_items<T: khora_core::renderer::traits::PipelineSystem>() {
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::layout;
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::inline_layout;
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::pipeline;
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::compute_pipeline;
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::set_overlay_source;
        let _ = <T as khora_core::renderer::traits::PipelineSystem>::recompose_dirty;
    }

    fn render_pass_trait_items<T: khora_core::renderer::traits::RenderPass<'static>>() {
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_pipeline;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_bind_group;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_vertex_buffer;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_index_buffer;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::draw;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::draw_indexed;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_viewport;
        let _ = <T as khora_core::renderer::traits::RenderPass<'static>>::set_scissor_rect;
    }
}

#[test]
fn module_renderer_paths_still_resolve() {
    // trait `khora_core::renderer::traits::GraphicsDevice`: see `graphics_device_trait_items`
    // trait `khora_core::renderer::GraphicsDevice`: see `graphics_device_trait_items`
    // trait `khora_core::renderer::traits::RenderSystem`: see `render_system_trait_items`
    // trait `khora_core::renderer::RenderSystem`: see `render_system_trait_items`
    let _ =
        type_name::<khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>>,
    );
    let _ = bind_group_descriptor_fields
        as fn(&khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>);
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupEntry<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupEntry<'static>>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>>,
    );
    let _ = bind_group_entry_fields
        as fn(&khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>);
    let _ = khora_core::renderer::api::command::bind_group::BindGroupEntry::buffer;
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>>();
    is_copy::<khora_core::renderer::api::command::bind_group::BindGroupEntry<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupId>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupId>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindGroupId>,
    );
    let _ =
        bind_group_id_fields as fn(&khora_core::renderer::api::command::bind_group::BindGroupId);
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    is_copy::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    is_partial_eq::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    is_eq::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    is_hash::<khora_core::renderer::api::command::bind_group::BindGroupId>();
    let _ = type_name::<
        khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>,
    >();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupLayoutDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupLayoutDescriptor<'static>>,
        PhantomData::<
            khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>,
        >,
    );
    let _ = bind_group_layout_descriptor_fields
        as fn(&khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>);
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>>(
    );
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupLayoutDescriptor<'static>>(
    );
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry>();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupLayoutEntry>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupLayoutEntry>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry>,
    );
    let _ = bind_group_layout_entry_fields
        as fn(&khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry);
    let _ = khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry::buffer;
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupLayoutEntry>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    let _ = type_name::<khora_core::renderer::api::command::BindGroupLayoutId>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindGroupLayoutId>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>,
    );
    let _ = bind_group_layout_id_fields
        as fn(&khora_core::renderer::api::command::bind_group::BindGroupLayoutId);
    is_debug::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    is_copy::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    is_partial_eq::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    is_eq::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    is_hash::<khora_core::renderer::api::command::bind_group::BindGroupLayoutId>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindingResource>();
    let _ = type_name::<khora_core::renderer::api::command::BindingResource>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindingResource>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindingResource>,
    );
    let _ = binding_resource_variants
        as fn(&khora_core::renderer::api::command::bind_group::BindingResource);
    is_debug::<khora_core::renderer::api::command::bind_group::BindingResource>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindingResource>();
    is_copy::<khora_core::renderer::api::command::bind_group::BindingResource>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BindingType>();
    let _ = type_name::<khora_core::renderer::api::command::BindingType>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BindingType>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BindingType>,
    );
    let _ =
        binding_type_variants as fn(&khora_core::renderer::api::command::bind_group::BindingType);
    is_debug::<khora_core::renderer::api::command::bind_group::BindingType>();
    is_clone::<khora_core::renderer::api::command::bind_group::BindingType>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BufferBinding>();
    let _ = type_name::<khora_core::renderer::api::command::BufferBinding>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BufferBinding>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BufferBinding>,
    );
    let _ =
        buffer_binding_fields as fn(&khora_core::renderer::api::command::bind_group::BufferBinding);
    is_debug::<khora_core::renderer::api::command::bind_group::BufferBinding>();
    is_clone::<khora_core::renderer::api::command::bind_group::BufferBinding>();
    is_copy::<khora_core::renderer::api::command::bind_group::BufferBinding>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    let _ = type_name::<khora_core::renderer::api::command::BufferBindingType>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::BufferBindingType>,
        PhantomData::<khora_core::renderer::api::command::bind_group::BufferBindingType>,
    );
    let _ = buffer_binding_type_variants
        as fn(&khora_core::renderer::api::command::bind_group::BufferBindingType);
    is_debug::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    is_clone::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    is_copy::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    is_partial_eq::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    is_eq::<khora_core::renderer::api::command::bind_group::BufferBindingType>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    let _ = type_name::<khora_core::renderer::api::command::SamplerBindingType>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::SamplerBindingType>,
        PhantomData::<khora_core::renderer::api::command::bind_group::SamplerBindingType>,
    );
    let _ = sampler_binding_type_variants
        as fn(&khora_core::renderer::api::command::bind_group::SamplerBindingType);
    is_debug::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    is_clone::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    is_copy::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    is_partial_eq::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    is_eq::<khora_core::renderer::api::command::bind_group::SamplerBindingType>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    let _ = type_name::<khora_core::renderer::api::command::TextureSampleType>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::TextureSampleType>,
        PhantomData::<khora_core::renderer::api::command::bind_group::TextureSampleType>,
    );
    let _ = texture_sample_type_variants
        as fn(&khora_core::renderer::api::command::bind_group::TextureSampleType);
    is_debug::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    is_clone::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    is_copy::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    is_partial_eq::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    is_eq::<khora_core::renderer::api::command::bind_group::TextureSampleType>();
    let _ = type_name::<
        khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>,
    >();
    let _ = type_name::<khora_core::renderer::api::pipeline::ComputePipelineDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ComputePipelineDescriptor<'static>>,
        PhantomData::<
            khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>,
        >,
    );
    let _ = compute_pipeline_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>);
    is_debug::<khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::pipeline::compute::ComputePipelineDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ComputePipelineId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ComputePipelineId>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ComputePipelineId>,
        PhantomData::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>,
    );
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ComputePipelineId>,
        PhantomData::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>,
    );
    let _ = compute_pipeline_id_fields
        as fn(&khora_core::renderer::api::pipeline::compute::ComputePipelineId);
    is_debug::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_clone::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_copy::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_partial_eq::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_eq::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_hash::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_partial_ord::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    is_ord::<khora_core::renderer::api::pipeline::compute::ComputePipelineId>();
    let _ = type_name::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    let _ = type_name::<khora_core::renderer::api::command::CommandBufferId>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::CommandBufferId>,
        PhantomData::<khora_core::renderer::api::command::draw_command::CommandBufferId>,
    );
    let _ = command_buffer_id_fields
        as fn(&khora_core::renderer::api::command::draw_command::CommandBufferId);
    is_debug::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    is_copy::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    is_clone::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    is_partial_eq::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    is_eq::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    is_hash::<khora_core::renderer::api::command::draw_command::CommandBufferId>();
    let _ = type_name::<khora_core::renderer::api::command::draw_command::DrawCommand>();
    let _ = type_name::<khora_core::renderer::api::command::DrawCommand>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::DrawCommand>,
        PhantomData::<khora_core::renderer::api::command::draw_command::DrawCommand>,
    );
    let _ =
        draw_command_fields as fn(&khora_core::renderer::api::command::draw_command::DrawCommand);
    is_debug::<khora_core::renderer::api::command::draw_command::DrawCommand>();
    is_clone::<khora_core::renderer::api::command::draw_command::DrawCommand>();
    let _ = type_name::<khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::ComputePassDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::ComputePassDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>>,
    );
    let _ = compute_pass_descriptor_fields
        as fn(&khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>);
    is_debug::<khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>>();
    is_default::<khora_core::renderer::api::command::pass::ComputePassDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::pass::LoadOp<f32>>();
    let _ = type_name::<khora_core::renderer::api::command::LoadOp<f32>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::LoadOp<f32>>,
        PhantomData::<khora_core::renderer::api::command::pass::LoadOp<f32>>,
    );
    let _ = load_op_variants as fn(&khora_core::renderer::api::command::pass::LoadOp<f32>);
    is_clone::<khora_core::renderer::api::command::pass::LoadOp<f32>>();
    is_debug::<khora_core::renderer::api::command::pass::LoadOp<f32>>();
    let _ = type_name::<khora_core::renderer::api::command::pass::Operations<f32>>();
    let _ = type_name::<khora_core::renderer::api::command::Operations<f32>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::Operations<f32>>,
        PhantomData::<khora_core::renderer::api::command::pass::Operations<f32>>,
    );
    let _ = operations_fields as fn(&khora_core::renderer::api::command::pass::Operations<f32>);
    is_debug::<khora_core::renderer::api::command::pass::Operations<f32>>();
    let _ = type_name::<khora_core::renderer::api::command::pass::PassTimestampWrites<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::PassTimestampWrites<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::PassTimestampWrites<'static>>,
        PhantomData::<khora_core::renderer::api::command::pass::PassTimestampWrites<'static>>,
    );
    let _ = pass_timestamp_writes_fields
        as fn(&khora_core::renderer::api::command::pass::PassTimestampWrites<'static>);
    is_debug::<khora_core::renderer::api::command::pass::PassTimestampWrites<'static>>();
    is_default::<khora_core::renderer::api::command::pass::PassTimestampWrites<'static>>();
    let _ =
        type_name::<khora_core::renderer::api::command::pass::RenderPassColorAttachment<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::RenderPassColorAttachment<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::RenderPassColorAttachment<'static>>,
        PhantomData::<khora_core::renderer::api::command::pass::RenderPassColorAttachment<'static>>,
    );
    let _ = render_pass_color_attachment_fields
        as fn(&khora_core::renderer::api::command::pass::RenderPassColorAttachment<'static>);
    is_debug::<khora_core::renderer::api::command::pass::RenderPassColorAttachment<'static>>();
    let _ = type_name::<
        khora_core::renderer::api::command::pass::RenderPassDepthStencilAttachment<'static>,
    >();
    let _ = type_name::<
        khora_core::renderer::api::command::RenderPassDepthStencilAttachment<'static>,
    >();
    same_type(
        PhantomData::<khora_core::renderer::api::command::RenderPassDepthStencilAttachment<'static>>,
        PhantomData::<
            khora_core::renderer::api::command::pass::RenderPassDepthStencilAttachment<'static>,
        >,
    );
    let _ = render_pass_depth_stencil_attachment_fields
        as fn(&khora_core::renderer::api::command::pass::RenderPassDepthStencilAttachment<'static>);
    is_debug::<khora_core::renderer::api::command::pass::RenderPassDepthStencilAttachment<'static>>(
    );
    let _ = type_name::<khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::RenderPassDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::RenderPassDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>>,
    );
    let _ = render_pass_descriptor_fields
        as fn(&khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>);
    is_debug::<khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>>();
    is_default::<khora_core::renderer::api::command::pass::RenderPassDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::pass::StoreOp>();
    let _ = type_name::<khora_core::renderer::api::command::StoreOp>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::StoreOp>,
        PhantomData::<khora_core::renderer::api::command::pass::StoreOp>,
    );
    let _ = store_op_variants as fn(&khora_core::renderer::api::command::pass::StoreOp);
    is_clone::<khora_core::renderer::api::command::pass::StoreOp>();
    is_debug::<khora_core::renderer::api::command::pass::StoreOp>();
    is_partial_eq::<khora_core::renderer::api::command::pass::StoreOp>();
    is_eq::<khora_core::renderer::api::command::pass::StoreOp>();
    let _ = khora_core::renderer::api::frame::MAX_FRAMES_IN_FLIGHT;
    let _ = type_name::<khora_core::renderer::api::device::adapter::GraphicsAdapterInfo>();
    let _ = type_name::<khora_core::renderer::api::device::GraphicsAdapterInfo>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::GraphicsAdapterInfo>,
        PhantomData::<khora_core::renderer::api::device::adapter::GraphicsAdapterInfo>,
    );
    let _ = graphics_adapter_info_fields
        as fn(&khora_core::renderer::api::device::adapter::GraphicsAdapterInfo);
    is_debug::<khora_core::renderer::api::device::adapter::GraphicsAdapterInfo>();
    is_clone::<khora_core::renderer::api::device::adapter::GraphicsAdapterInfo>();
    is_default::<khora_core::renderer::api::device::adapter::GraphicsAdapterInfo>();
    let _ = type_name::<khora_core::renderer::api::device::backend::BackendSelectionConfig>();
    let _ = type_name::<khora_core::renderer::api::device::BackendSelectionConfig>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::BackendSelectionConfig>,
        PhantomData::<khora_core::renderer::api::device::backend::BackendSelectionConfig>,
    );
    let _ = backend_selection_config_fields
        as fn(&khora_core::renderer::api::device::backend::BackendSelectionConfig);
    is_debug::<khora_core::renderer::api::device::backend::BackendSelectionConfig>();
    is_clone::<khora_core::renderer::api::device::backend::BackendSelectionConfig>();
    is_default::<khora_core::renderer::api::device::backend::BackendSelectionConfig>();
    let _ = type_name::<khora_core::renderer::api::device::backend::BackendSelectionResult<u32>>();
    let _ = type_name::<khora_core::renderer::api::device::BackendSelectionResult<u32>>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::BackendSelectionResult<u32>>,
        PhantomData::<khora_core::renderer::api::device::backend::BackendSelectionResult<u32>>,
    );
    let _ = backend_selection_result_fields
        as fn(&khora_core::renderer::api::device::backend::BackendSelectionResult<u32>);
    is_debug::<khora_core::renderer::api::device::backend::BackendSelectionResult<u32>>();
    let _ = type_name::<khora_core::renderer::api::frame::render_context::RenderContext<'static>>();
    let _ = type_name::<khora_core::renderer::api::frame::RenderContext<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::frame::RenderContext<'static>>,
        PhantomData::<khora_core::renderer::api::frame::render_context::RenderContext<'static>>,
    );
    let _ = render_context_fields
        as fn(&khora_core::renderer::api::frame::render_context::RenderContext<'static>);
    let _ = khora_core::renderer::api::frame::render_context::RenderContext::new;
    let _ = type_name::<khora_core::renderer::api::frame::frame_context::FrameContext>();
    let _ = type_name::<khora_core::renderer::api::frame::FrameContext>();
    same_type(
        PhantomData::<khora_core::renderer::api::frame::FrameContext>,
        PhantomData::<khora_core::renderer::api::frame::frame_context::FrameContext>,
    );
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::new;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::insert::<u32>;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::get::<u32>;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::contains::<u32>;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::insert_stage::<u32>;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::spawn::<
        std::future::Ready<()>,
    >;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::wait_for_all;
    let _ = khora_core::renderer::api::frame::frame_context::FrameContext::tokio_handle;
    is_send::<khora_core::renderer::api::frame::frame_context::FrameContext>();
    is_sync::<khora_core::renderer::api::frame::frame_context::FrameContext>();
    is_debug::<khora_core::renderer::api::frame::frame_context::FrameContext>();
    let _ = type_name::<khora_core::renderer::api::frame::frame_context::StageHandle<u32>>();
    let _ = type_name::<khora_core::renderer::api::frame::StageHandle<u32>>();
    same_type(
        PhantomData::<khora_core::renderer::api::frame::StageHandle<u32>>,
        PhantomData::<khora_core::renderer::api::frame::frame_context::StageHandle<u32>>,
    );
    let _ = <khora_core::renderer::api::frame::frame_context::StageHandle<u32>>::mark_done;
    let _ = <khora_core::renderer::api::frame::frame_context::StageHandle<u32>>::is_done;
    let _ = <khora_core::renderer::api::frame::frame_context::StageHandle<u32>>::wait;
    is_clone::<khora_core::renderer::api::frame::frame_context::StageHandle<u32>>();
    is_default::<khora_core::renderer::api::frame::frame_context::StageHandle<u32>>();
    let _ = type_name::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    let _ = type_name::<khora_core::renderer::api::device::GpuHook>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::GpuHook>,
        PhantomData::<khora_core::renderer::api::device::gpu_hook::GpuHook>,
    );
    let _ = gpu_hook_variants as fn(&khora_core::renderer::api::device::gpu_hook::GpuHook);
    let _ = khora_core::renderer::api::device::gpu_hook::GpuHook::ALL;
    is_debug::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    is_clone::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    is_copy::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    is_partial_eq::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    is_eq::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    is_hash::<khora_core::renderer::api::device::gpu_hook::GpuHook>();
    let _ = type_name::<khora_core::renderer::api::device::settings::RenderSettings>();
    let _ = type_name::<khora_core::renderer::api::device::RenderSettings>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::RenderSettings>,
        PhantomData::<khora_core::renderer::api::device::settings::RenderSettings>,
    );
    let _ =
        render_settings_fields as fn(&khora_core::renderer::api::device::settings::RenderSettings);
    is_debug::<khora_core::renderer::api::device::settings::RenderSettings>();
    is_clone::<khora_core::renderer::api::device::settings::RenderSettings>();
    is_default::<khora_core::renderer::api::device::settings::RenderSettings>();
    let _ =
        type_name::<khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderModuleDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderModuleDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>>,
    );
    let _ = shader_module_descriptor_fields
        as fn(&khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>);
    is_debug::<khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::shader::module::ShaderModuleDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderModuleId>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderModuleId>,
        PhantomData::<khora_core::renderer::api::shader::module::ShaderModuleId>,
    );
    let _ =
        shader_module_id_fields as fn(&khora_core::renderer::api::shader::module::ShaderModuleId);
    is_debug::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_clone::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_copy::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_partial_eq::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_eq::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_hash::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_partial_ord::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    is_ord::<khora_core::renderer::api::shader::module::ShaderModuleId>();
    let _ = type_name::<khora_core::renderer::api::shader::module::ShaderSourceData<'static>>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderSourceData<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderSourceData<'static>>,
        PhantomData::<khora_core::renderer::api::shader::module::ShaderSourceData<'static>>,
    );
    let _ = shader_source_data_variants
        as fn(&khora_core::renderer::api::shader::module::ShaderSourceData<'static>);
    is_debug::<khora_core::renderer::api::shader::module::ShaderSourceData<'static>>();
    is_clone::<khora_core::renderer::api::shader::module::ShaderSourceData<'static>>();
    let _ = type_name::<khora_core::renderer::api::device::stats::RenderStats>();
    let _ = type_name::<khora_core::renderer::api::device::RenderStats>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::RenderStats>,
        PhantomData::<khora_core::renderer::api::device::stats::RenderStats>,
    );
    let _ = render_stats_fields as fn(&khora_core::renderer::api::device::stats::RenderStats);
    is_debug::<khora_core::renderer::api::device::stats::RenderStats>();
    is_clone::<khora_core::renderer::api::device::stats::RenderStats>();
    is_default::<khora_core::renderer::api::device::stats::RenderStats>();
    let _ = khora_core::renderer::api::ibl::bindings::IBL_BINDING_COUNT;
    let _ = khora_core::renderer::api::ibl::IBL_BINDING_COUNT;
    let _ = type_name::<khora_core::renderer::api::ibl::bindings::IblGpuBindings>();
    let _ = type_name::<khora_core::renderer::api::ibl::IblGpuBindings>();
    same_type(
        PhantomData::<khora_core::renderer::api::ibl::IblGpuBindings>,
        PhantomData::<khora_core::renderer::api::ibl::bindings::IblGpuBindings>,
    );
    let _ =
        ibl_gpu_bindings_fields as fn(&khora_core::renderer::api::ibl::bindings::IblGpuBindings);
    is_debug::<khora_core::renderer::api::ibl::bindings::IblGpuBindings>();
    is_clone::<khora_core::renderer::api::ibl::bindings::IblGpuBindings>();
    is_copy::<khora_core::renderer::api::ibl::bindings::IblGpuBindings>();
    let _ = khora_core::renderer::api::ibl::bindings::fill_ibl_bind_group_entries;
    let _ = khora_core::renderer::api::ibl::fill_ibl_bind_group_entries;
    same_item(
        &khora_core::renderer::api::ibl::fill_ibl_bind_group_entries,
        &khora_core::renderer::api::ibl::bindings::fill_ibl_bind_group_entries,
    );
    let _ = khora_core::renderer::api::ibl::bindings::ibl_bind_group_layout_entries;
    let _ = khora_core::renderer::api::ibl::ibl_bind_group_layout_entries;
    same_item(
        &khora_core::renderer::api::ibl::ibl_bind_group_layout_entries,
        &khora_core::renderer::api::ibl::bindings::ibl_bind_group_layout_entries,
    );
    let _ = khora_core::renderer::api::ibl::bindings::offset::BRDF_LUT;
    let _ = khora_core::renderer::api::ibl::bindings::offset::IRRADIANCE_CUBE;
    let _ = khora_core::renderer::api::ibl::bindings::offset::PREFILTERED_CUBE;
    let _ = khora_core::renderer::api::ibl::bindings::offset::SAMPLER;
    let _ = type_name::<khora_core::renderer::api::material::bindings::MaterialGpuBindings>();
    let _ = type_name::<khora_core::renderer::api::material::MaterialGpuBindings>();
    same_type(
        PhantomData::<khora_core::renderer::api::material::MaterialGpuBindings>,
        PhantomData::<khora_core::renderer::api::material::bindings::MaterialGpuBindings>,
    );
    let _ = material_gpu_bindings_fields
        as fn(&khora_core::renderer::api::material::bindings::MaterialGpuBindings);
    is_debug::<khora_core::renderer::api::material::bindings::MaterialGpuBindings>();
    is_clone::<khora_core::renderer::api::material::bindings::MaterialGpuBindings>();
    is_copy::<khora_core::renderer::api::material::bindings::MaterialGpuBindings>();
    let _ = khora_core::renderer::api::material::bindings::binding::BASE_COLOR_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::binding::EMISSIVE_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::binding::MATERIAL_UNIFORMS;
    let _ = khora_core::renderer::api::material::bindings::binding::METALLIC_ROUGHNESS_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::binding::NORMAL_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::binding::OCCLUSION_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::binding::SAMPLER;
    let _ = khora_core::renderer::api::material::bindings::fill_material_bind_group_entries;
    let _ = khora_core::renderer::api::material::fill_material_bind_group_entries;
    same_item(
        &khora_core::renderer::api::material::fill_material_bind_group_entries,
        &khora_core::renderer::api::material::bindings::fill_material_bind_group_entries,
    );
    let _ = khora_core::renderer::api::material::bindings::flag::HAS_BASE_COLOR_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::flag::HAS_EMISSIVE_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::flag::HAS_METALLIC_ROUGHNESS_TEXTURE;
    let _ = khora_core::renderer::api::material::bindings::flag::HAS_NORMAL_MAP;
    let _ = khora_core::renderer::api::material::bindings::flag::HAS_OCCLUSION_MAP;
    let _ = khora_core::renderer::api::material::bindings::material_bind_group_layout_entries;
    let _ = khora_core::renderer::api::material::material_bind_group_layout_entries;
    same_item(
        &khora_core::renderer::api::material::material_bind_group_layout_entries,
        &khora_core::renderer::api::material::bindings::material_bind_group_layout_entries,
    );
    let _ = khora_core::renderer::api::material::bindings::material_layout_entries_for_variant;
    let _ = khora_core::renderer::api::material::material_layout_entries_for_variant;
    same_item(
        &khora_core::renderer::api::material::material_layout_entries_for_variant,
        &khora_core::renderer::api::material::bindings::material_layout_entries_for_variant,
    );
    let _ =
        type_name::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::MultisampleStateDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::MultisampleStateDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>,
    );
    let _ = multisample_state_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    is_copy::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::descriptor::MultisampleStateDescriptor>();
    let _ = type_name::<
        khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>,
    >();
    let _ = type_name::<khora_core::renderer::api::pipeline::RenderPipelineDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::RenderPipelineDescriptor<'static>>,
        PhantomData::<
            khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>,
        >,
    );
    let _ = render_pipeline_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>);
    is_debug::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>>(
    );
    is_clone::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineDescriptor<'static>>(
    );
    let _ = type_name::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::RenderPipelineId>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::RenderPipelineId>,
        PhantomData::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>,
    );
    let _ = render_pipeline_id_fields
        as fn(&khora_core::renderer::api::pipeline::descriptor::RenderPipelineId);
    is_debug::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_clone::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_copy::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_partial_eq::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_eq::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_hash::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_partial_ord::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    is_ord::<khora_core::renderer::api::pipeline::descriptor::RenderPipelineId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::BlendFactor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::BlendFactor>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::BlendFactor>,
    );
    let _ = blend_factor_variants as fn(&khora_core::renderer::api::pipeline::enums::BlendFactor);
    is_debug::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    is_clone::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    is_copy::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    is_eq::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    is_hash::<khora_core::renderer::api::pipeline::enums::BlendFactor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    let _ = type_name::<khora_core::renderer::api::pipeline::BlendOperation>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::BlendOperation>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::BlendOperation>,
    );
    let _ =
        blend_operation_variants as fn(&khora_core::renderer::api::pipeline::enums::BlendOperation);
    is_debug::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    is_clone::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    is_copy::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    is_eq::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    is_hash::<khora_core::renderer::api::pipeline::enums::BlendOperation>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    let _ = type_name::<khora_core::renderer::api::pipeline::CompareFunction>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::CompareFunction>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::CompareFunction>,
    );
    let _ = compare_function_variants
        as fn(&khora_core::renderer::api::pipeline::enums::CompareFunction);
    is_debug::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_clone::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_copy::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_eq::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_hash::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    is_default::<khora_core::renderer::api::pipeline::enums::CompareFunction>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::CullMode>();
    let _ = type_name::<khora_core::renderer::api::pipeline::CullMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::CullMode>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::CullMode>,
    );
    let _ = cull_mode_variants as fn(&khora_core::renderer::api::pipeline::enums::CullMode);
    is_debug::<khora_core::renderer::api::pipeline::enums::CullMode>();
    is_clone::<khora_core::renderer::api::pipeline::enums::CullMode>();
    is_copy::<khora_core::renderer::api::pipeline::enums::CullMode>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::CullMode>();
    is_eq::<khora_core::renderer::api::pipeline::enums::CullMode>();
    is_hash::<khora_core::renderer::api::pipeline::enums::CullMode>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    let _ = type_name::<khora_core::renderer::api::pipeline::FrontFace>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::FrontFace>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::FrontFace>,
    );
    let _ = front_face_variants as fn(&khora_core::renderer::api::pipeline::enums::FrontFace);
    is_debug::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    is_clone::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    is_copy::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    is_eq::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    is_hash::<khora_core::renderer::api::pipeline::enums::FrontFace>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PolygonMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PolygonMode>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::PolygonMode>,
    );
    let _ = polygon_mode_variants as fn(&khora_core::renderer::api::pipeline::enums::PolygonMode);
    is_debug::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    is_clone::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    is_copy::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    is_eq::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    is_hash::<khora_core::renderer::api::pipeline::enums::PolygonMode>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PrimitiveTopology>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PrimitiveTopology>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>,
    );
    let _ = primitive_topology_variants
        as fn(&khora_core::renderer::api::pipeline::enums::PrimitiveTopology);
    is_debug::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    is_clone::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    is_copy::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    is_eq::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    is_hash::<khora_core::renderer::api::pipeline::enums::PrimitiveTopology>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    let _ = type_name::<khora_core::renderer::api::pipeline::StencilOperation>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::StencilOperation>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::StencilOperation>,
    );
    let _ = stencil_operation_variants
        as fn(&khora_core::renderer::api::pipeline::enums::StencilOperation);
    is_debug::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_clone::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_copy::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_eq::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_hash::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    is_default::<khora_core::renderer::api::pipeline::enums::StencilOperation>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    let _ = type_name::<khora_core::renderer::api::pipeline::VertexFormat>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::VertexFormat>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::VertexFormat>,
    );
    let _ = vertex_format_variants as fn(&khora_core::renderer::api::pipeline::enums::VertexFormat);
    let _ = khora_core::renderer::api::pipeline::enums::VertexFormat::size;
    is_debug::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    is_clone::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    is_copy::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    is_eq::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    is_hash::<khora_core::renderer::api::pipeline::enums::VertexFormat>();
    let _ = type_name::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    let _ = type_name::<khora_core::renderer::api::pipeline::VertexStepMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::VertexStepMode>,
        PhantomData::<khora_core::renderer::api::pipeline::enums::VertexStepMode>,
    );
    let _ = vertex_step_mode_variants
        as fn(&khora_core::renderer::api::pipeline::enums::VertexStepMode);
    is_debug::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    is_clone::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    is_copy::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    is_partial_eq::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    is_eq::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    is_hash::<khora_core::renderer::api::pipeline::enums::VertexStepMode>();
    let _ = type_name::<
        khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>,
    >();
    let _ = type_name::<khora_core::renderer::api::pipeline::PipelineLayoutDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PipelineLayoutDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>>,
    );
    let _ = pipeline_layout_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>);
    is_debug::<khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::pipeline::layout::PipelineLayoutDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PipelineLayoutId>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PipelineLayoutId>,
        PhantomData::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>,
    );
    let _ = pipeline_layout_id_fields
        as fn(&khora_core::renderer::api::pipeline::layout::PipelineLayoutId);
    is_debug::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_clone::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_copy::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_partial_eq::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_eq::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_hash::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_partial_ord::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    is_ord::<khora_core::renderer::api::pipeline::layout::PipelineLayoutId>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ComputePipelineKey>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ComputePipelineKey>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>,
    );
    let _ = compute_pipeline_key_fields
        as fn(&khora_core::renderer::api::pipeline::spec::ComputePipelineKey);
    is_debug::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    is_clone::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    is_partial_eq::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    is_eq::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    is_hash::<khora_core::renderer::api::pipeline::spec::ComputePipelineKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::ComputePipelineSpec>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ComputePipelineSpec>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ComputePipelineSpec>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::ComputePipelineSpec>,
    );
    let _ = compute_pipeline_spec_fields
        as fn(&khora_core::renderer::api::pipeline::spec::ComputePipelineSpec);
    let _ = khora_core::renderer::api::pipeline::spec::ComputePipelineSpec::key;
    is_debug::<khora_core::renderer::api::pipeline::spec::ComputePipelineSpec>();
    is_clone::<khora_core::renderer::api::pipeline::spec::ComputePipelineSpec>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::LayoutCacheKey>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::LayoutCacheKey>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>,
    );
    let _ =
        layout_cache_key_variants as fn(&khora_core::renderer::api::pipeline::spec::LayoutCacheKey);
    is_debug::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    is_clone::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    is_partial_eq::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    is_eq::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    is_hash::<khora_core::renderer::api::pipeline::spec::LayoutCacheKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::LayoutKey>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::LayoutKey>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::LayoutKey>,
    );
    let _ = layout_key_variants as fn(&khora_core::renderer::api::pipeline::spec::LayoutKey);
    let _ = khora_core::renderer::api::pipeline::spec::LayoutKey::entries;
    is_debug::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    is_clone::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    is_copy::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    is_partial_eq::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    is_eq::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    is_hash::<khora_core::renderer::api::pipeline::spec::LayoutKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::LayoutSpec>();
    let _ = type_name::<khora_core::renderer::api::pipeline::LayoutSpec>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::LayoutSpec>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::LayoutSpec>,
    );
    let _ = layout_spec_variants as fn(&khora_core::renderer::api::pipeline::spec::LayoutSpec);
    let _ = khora_core::renderer::api::pipeline::spec::LayoutSpec::cache_key;
    is_debug::<khora_core::renderer::api::pipeline::spec::LayoutSpec>();
    is_clone::<khora_core::renderer::api::pipeline::spec::LayoutSpec>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PipelineKey>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PipelineKey>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::PipelineKey>,
    );
    let _ = pipeline_key_fields as fn(&khora_core::renderer::api::pipeline::spec::PipelineKey);
    is_debug::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    is_clone::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    is_partial_eq::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    is_eq::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    is_hash::<khora_core::renderer::api::pipeline::spec::PipelineKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::spec::PipelineSpec>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PipelineSpec>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PipelineSpec>,
        PhantomData::<khora_core::renderer::api::pipeline::spec::PipelineSpec>,
    );
    let _ = pipeline_spec_fields as fn(&khora_core::renderer::api::pipeline::spec::PipelineSpec);
    let _ = khora_core::renderer::api::pipeline::spec::PipelineSpec::key;
    is_debug::<khora_core::renderer::api::pipeline::spec::PipelineSpec>();
    is_clone::<khora_core::renderer::api::pipeline::spec::PipelineSpec>();
    let _ = type_name::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderDefScalar>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderDefScalar>,
        PhantomData::<khora_core::renderer::api::shader::variant::ShaderDefScalar>,
    );
    let _ = shader_def_scalar_variants
        as fn(&khora_core::renderer::api::shader::variant::ShaderDefScalar);
    is_debug::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    is_clone::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    is_copy::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    is_partial_eq::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    is_eq::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    is_hash::<khora_core::renderer::api::shader::variant::ShaderDefScalar>();
    let _ = type_name::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderVariantKey>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderVariantKey>,
        PhantomData::<khora_core::renderer::api::shader::variant::ShaderVariantKey>,
    );
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::empty;
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::flag;
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::with;
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::is_empty;
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::has_flag;
    let _ = khora_core::renderer::api::shader::variant::ShaderVariantKey::defs;
    is_debug::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    is_clone::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    is_default::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    is_partial_eq::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    is_eq::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    is_hash::<khora_core::renderer::api::shader::variant::ShaderVariantKey>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::BlendComponentDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::BlendComponentDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>,
    );
    let _ = blend_component_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::BlendComponentDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    is_copy::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::state::BlendComponentDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::BlendStateDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::BlendStateDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>,
    );
    let _ = blend_state_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::BlendStateDescriptor);
    let _ = khora_core::renderer::api::pipeline::state::BlendStateDescriptor::alpha_blending;
    is_debug::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    is_copy::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::state::BlendStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ColorTargetStateDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ColorTargetStateDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>,
    );
    let _ = color_target_state_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::state::ColorTargetStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    let _ = type_name::<khora_core::renderer::api::pipeline::ColorWrites>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::ColorWrites>,
        PhantomData::<khora_core::renderer::api::pipeline::state::ColorWrites>,
    );
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::EMPTY;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::ALL_DECLARED;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::from_bits_truncate;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::bits;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::contains;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::intersects;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::insert;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::remove;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::toggle;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::with;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::without;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::R;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::G;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::B;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::A;
    let _ = khora_core::renderer::api::pipeline::state::ColorWrites::ALL;
    is_clone::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_copy::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_eq::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_hash::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_default::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_or::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_and::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_xor::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_not::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_or_assign::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_and_assign::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_bit_xor_assign::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    is_debug::<khora_core::renderer::api::pipeline::state::ColorWrites>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    let _ = type_name::<khora_core::renderer::api::pipeline::DepthBiasState>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::DepthBiasState>,
        PhantomData::<khora_core::renderer::api::pipeline::state::DepthBiasState>,
    );
    let _ =
        depth_bias_state_fields as fn(&khora_core::renderer::api::pipeline::state::DepthBiasState);
    is_debug::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    is_clone::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    is_copy::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    is_default::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::DepthBiasState>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::DepthStencilStateDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::DepthStencilStateDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor>,
    );
    let _ = depth_stencil_state_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::DepthStencilStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::PrimitiveStateDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::PrimitiveStateDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>,
    );
    let _ = primitive_state_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_copy::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    is_default::<khora_core::renderer::api::pipeline::state::PrimitiveStateDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    let _ = type_name::<khora_core::renderer::api::pipeline::StencilFaceState>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::StencilFaceState>,
        PhantomData::<khora_core::renderer::api::pipeline::state::StencilFaceState>,
    );
    let _ = stencil_face_state_fields
        as fn(&khora_core::renderer::api::pipeline::state::StencilFaceState);
    is_debug::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_clone::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_copy::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_default::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_eq::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    is_hash::<khora_core::renderer::api::pipeline::state::StencilFaceState>();
    let _ = type_name::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    let _ = type_name::<khora_core::renderer::api::pipeline::VertexAttributeDescriptor>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::VertexAttributeDescriptor>,
        PhantomData::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>,
    );
    let _ = vertex_attribute_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor);
    is_debug::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    is_clone::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    is_partial_eq::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    is_eq::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    is_hash::<khora_core::renderer::api::pipeline::state::VertexAttributeDescriptor>();
    let _ = type_name::<
        khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>,
    >();
    let _ =
        type_name::<khora_core::renderer::api::pipeline::VertexBufferLayoutDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::pipeline::VertexBufferLayoutDescriptor<'static>>,
        PhantomData::<
            khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>,
        >,
    );
    let _ = vertex_buffer_layout_descriptor_fields
        as fn(&khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>);
    is_debug::<khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::pipeline::state::VertexBufferLayoutDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::BufferDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::BufferDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>>,
    );
    let _ = buffer_descriptor_fields
        as fn(&khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>);
    is_debug::<khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::resource::buffer::BufferDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::buffer::BufferId>();
    let _ = type_name::<khora_core::renderer::api::resource::BufferId>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::BufferId>,
        PhantomData::<khora_core::renderer::api::resource::buffer::BufferId>,
    );
    let _ = buffer_id_fields as fn(&khora_core::renderer::api::resource::buffer::BufferId);
    is_debug::<khora_core::renderer::api::resource::buffer::BufferId>();
    is_clone::<khora_core::renderer::api::resource::buffer::BufferId>();
    is_copy::<khora_core::renderer::api::resource::buffer::BufferId>();
    is_partial_eq::<khora_core::renderer::api::resource::buffer::BufferId>();
    is_eq::<khora_core::renderer::api::resource::buffer::BufferId>();
    is_hash::<khora_core::renderer::api::resource::buffer::BufferId>();
    let _ = type_name::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    let _ = type_name::<khora_core::renderer::api::resource::BufferUsage>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::BufferUsage>,
        PhantomData::<khora_core::renderer::api::resource::buffer::BufferUsage>,
    );
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::EMPTY;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::ALL_DECLARED;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::from_bits_truncate;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::bits;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::contains;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::intersects;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::insert;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::remove;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::toggle;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::with;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::without;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::MAP_READ;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::MAP_WRITE;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::COPY_SRC;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::COPY_DST;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::VERTEX;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::INDEX;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::UNIFORM;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::STORAGE;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::INDIRECT;
    let _ = khora_core::renderer::api::resource::buffer::BufferUsage::QUERY_RESOLVE;
    is_clone::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_copy::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_partial_eq::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_eq::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_hash::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_default::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_or::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_and::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_xor::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_not::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_or_assign::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_and_assign::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_bit_xor_assign::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    is_debug::<khora_core::renderer::api::resource::buffer::BufferUsage>();
    let _ = type_name::<khora_core::renderer::api::shader::source::CpuShaderSource>();
    let _ = type_name::<khora_core::renderer::api::shader::CpuShaderSource>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::CpuShaderSource>,
        PhantomData::<khora_core::renderer::api::shader::source::CpuShaderSource>,
    );
    let _ =
        cpu_shader_source_fields as fn(&khora_core::renderer::api::shader::source::CpuShaderSource);
    is_debug::<khora_core::renderer::api::shader::source::CpuShaderSource>();
    is_clone::<khora_core::renderer::api::shader::source::CpuShaderSource>();
    is_asset::<khora_core::renderer::api::shader::source::CpuShaderSource>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::AddressMode>();
    let _ = type_name::<khora_core::renderer::api::resource::AddressMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::AddressMode>,
        PhantomData::<khora_core::renderer::api::resource::texture::AddressMode>,
    );
    let _ = address_mode_variants as fn(&khora_core::renderer::api::resource::texture::AddressMode);
    is_debug::<khora_core::renderer::api::resource::texture::AddressMode>();
    is_clone::<khora_core::renderer::api::resource::texture::AddressMode>();
    is_copy::<khora_core::renderer::api::resource::texture::AddressMode>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::AddressMode>();
    is_eq::<khora_core::renderer::api::resource::texture::AddressMode>();
    is_hash::<khora_core::renderer::api::resource::texture::AddressMode>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::CpuTexture>();
    let _ = type_name::<khora_core::renderer::api::resource::CpuTexture>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::CpuTexture>,
        PhantomData::<khora_core::renderer::api::resource::texture::CpuTexture>,
    );
    let _ = cpu_texture_fields as fn(&khora_core::renderer::api::resource::texture::CpuTexture);
    let _ = khora_core::renderer::api::resource::texture::CpuTexture::from_rgba32f;
    let _ = khora_core::renderer::api::resource::texture::CpuTexture::to_descriptor;
    let _ = khora_core::renderer::api::resource::texture::CpuTexture::row_size;
    is_debug::<khora_core::renderer::api::resource::texture::CpuTexture>();
    is_asset::<khora_core::renderer::api::resource::texture::CpuTexture>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::FilterMode>();
    let _ = type_name::<khora_core::renderer::api::resource::FilterMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::FilterMode>,
        PhantomData::<khora_core::renderer::api::resource::texture::FilterMode>,
    );
    let _ = filter_mode_variants as fn(&khora_core::renderer::api::resource::texture::FilterMode);
    is_debug::<khora_core::renderer::api::resource::texture::FilterMode>();
    is_clone::<khora_core::renderer::api::resource::texture::FilterMode>();
    is_copy::<khora_core::renderer::api::resource::texture::FilterMode>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::FilterMode>();
    is_eq::<khora_core::renderer::api::resource::texture::FilterMode>();
    is_hash::<khora_core::renderer::api::resource::texture::FilterMode>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::ImageAspect>();
    let _ = type_name::<khora_core::renderer::api::resource::ImageAspect>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::ImageAspect>,
        PhantomData::<khora_core::renderer::api::resource::texture::ImageAspect>,
    );
    let _ = image_aspect_variants as fn(&khora_core::renderer::api::resource::texture::ImageAspect);
    is_debug::<khora_core::renderer::api::resource::texture::ImageAspect>();
    is_clone::<khora_core::renderer::api::resource::texture::ImageAspect>();
    is_copy::<khora_core::renderer::api::resource::texture::ImageAspect>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::ImageAspect>();
    is_eq::<khora_core::renderer::api::resource::texture::ImageAspect>();
    is_hash::<khora_core::renderer::api::resource::texture::ImageAspect>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    let _ = type_name::<khora_core::renderer::api::resource::MipmapFilterMode>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::MipmapFilterMode>,
        PhantomData::<khora_core::renderer::api::resource::texture::MipmapFilterMode>,
    );
    let _ = mipmap_filter_mode_variants
        as fn(&khora_core::renderer::api::resource::texture::MipmapFilterMode);
    is_debug::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    is_clone::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    is_copy::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    is_eq::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    is_hash::<khora_core::renderer::api::resource::texture::MipmapFilterMode>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    let _ = type_name::<khora_core::renderer::api::resource::SamplerBorderColor>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::SamplerBorderColor>,
        PhantomData::<khora_core::renderer::api::resource::texture::SamplerBorderColor>,
    );
    let _ = sampler_border_color_variants
        as fn(&khora_core::renderer::api::resource::texture::SamplerBorderColor);
    is_debug::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    is_clone::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    is_copy::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    is_eq::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    is_hash::<khora_core::renderer::api::resource::texture::SamplerBorderColor>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::SamplerDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::SamplerDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>>,
    );
    let _ = sampler_descriptor_fields
        as fn(&khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>);
    is_debug::<khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::resource::texture::SamplerDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::SamplerId>();
    let _ = type_name::<khora_core::renderer::api::resource::SamplerId>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::SamplerId>,
        PhantomData::<khora_core::renderer::api::resource::texture::SamplerId>,
    );
    let _ = sampler_id_fields as fn(&khora_core::renderer::api::resource::texture::SamplerId);
    is_debug::<khora_core::renderer::api::resource::texture::SamplerId>();
    is_clone::<khora_core::renderer::api::resource::texture::SamplerId>();
    is_copy::<khora_core::renderer::api::resource::texture::SamplerId>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::SamplerId>();
    is_eq::<khora_core::renderer::api::resource::texture::SamplerId>();
    is_hash::<khora_core::renderer::api::resource::texture::SamplerId>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureDescriptor<'static>>,
    );
    let _ = texture_descriptor_fields
        as fn(&khora_core::renderer::api::resource::texture::TextureDescriptor<'static>);
    is_debug::<khora_core::renderer::api::resource::texture::TextureDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureDimension>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureDimension>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureDimension>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureDimension>,
    );
    let _ = texture_dimension_variants
        as fn(&khora_core::renderer::api::resource::texture::TextureDimension);
    is_debug::<khora_core::renderer::api::resource::texture::TextureDimension>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureDimension>();
    is_copy::<khora_core::renderer::api::resource::texture::TextureDimension>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::TextureDimension>();
    is_eq::<khora_core::renderer::api::resource::texture::TextureDimension>();
    is_hash::<khora_core::renderer::api::resource::texture::TextureDimension>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureId>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureId>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureId>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureId>,
    );
    let _ = texture_id_fields as fn(&khora_core::renderer::api::resource::texture::TextureId);
    is_debug::<khora_core::renderer::api::resource::texture::TextureId>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureId>();
    is_copy::<khora_core::renderer::api::resource::texture::TextureId>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::TextureId>();
    is_eq::<khora_core::renderer::api::resource::texture::TextureId>();
    is_hash::<khora_core::renderer::api::resource::texture::TextureId>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureUsage>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureUsage>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureUsage>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureUsage>,
    );
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::EMPTY;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::ALL_DECLARED;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::from_bits_truncate;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::bits;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::contains;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::intersects;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::insert;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::remove;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::toggle;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::with;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::without;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::COPY_SRC;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::COPY_DST;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::TEXTURE_BINDING;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::STORAGE_BINDING;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::RENDER_ATTACHMENT;
    let _ = khora_core::renderer::api::resource::texture::TextureUsage::DEPTH_STENCIL_ATTACHMENT;
    is_clone::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_copy::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_eq::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_hash::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_default::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_or::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_and::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_xor::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_not::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_or_assign::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_and_assign::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_bit_xor_assign::<khora_core::renderer::api::resource::texture::TextureUsage>();
    is_debug::<khora_core::renderer::api::resource::texture::TextureUsage>();
    let _ =
        type_name::<khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureViewDescriptor<'static>>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureViewDescriptor<'static>>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>>,
    );
    let _ = texture_view_descriptor_fields
        as fn(&khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>);
    is_debug::<khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureViewDescriptor<'static>>();
    let _ = type_name::<khora_core::renderer::api::command::bind_group::TextureViewDimension>();
    let _ = type_name::<khora_core::renderer::api::command::TextureViewDimension>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureViewDimension>();
    same_type(
        PhantomData::<khora_core::renderer::api::command::bind_group::TextureViewDimension>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureViewDimension>,
    );
    same_type(
        PhantomData::<khora_core::renderer::api::command::TextureViewDimension>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureViewDimension>,
    );
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureViewDimension>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureViewDimension>,
    );
    let _ = texture_view_dimension_variants
        as fn(&khora_core::renderer::api::resource::texture::TextureViewDimension);
    is_debug::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    is_copy::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    is_eq::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    is_hash::<khora_core::renderer::api::resource::texture::TextureViewDimension>();
    let _ = type_name::<khora_core::renderer::api::resource::texture::TextureViewId>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureViewId>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureViewId>,
        PhantomData::<khora_core::renderer::api::resource::texture::TextureViewId>,
    );
    let _ =
        texture_view_id_fields as fn(&khora_core::renderer::api::resource::texture::TextureViewId);
    is_debug::<khora_core::renderer::api::resource::texture::TextureViewId>();
    is_clone::<khora_core::renderer::api::resource::texture::TextureViewId>();
    is_copy::<khora_core::renderer::api::resource::texture::TextureViewId>();
    is_partial_eq::<khora_core::renderer::api::resource::texture::TextureViewId>();
    is_eq::<khora_core::renderer::api::resource::texture::TextureViewId>();
    is_hash::<khora_core::renderer::api::resource::texture::TextureViewId>();
    let _ = type_name::<khora_core::renderer::api::resource::view::CameraUniformData>();
    let _ = type_name::<khora_core::renderer::api::resource::CameraUniformData>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::CameraUniformData>,
        PhantomData::<khora_core::renderer::api::resource::view::CameraUniformData>,
    );
    let _ = camera_uniform_data_fields
        as fn(&khora_core::renderer::api::resource::view::CameraUniformData);
    let _ = khora_core::renderer::api::resource::view::CameraUniformData::from_view_info;
    let _ = khora_core::renderer::api::resource::view::CameraUniformData::as_bytes;
    is_debug::<khora_core::renderer::api::resource::view::CameraUniformData>();
    is_clone::<khora_core::renderer::api::resource::view::CameraUniformData>();
    is_copy::<khora_core::renderer::api::resource::view::CameraUniformData>();
    is_pod::<khora_core::renderer::api::resource::view::CameraUniformData>();
    is_zeroable::<khora_core::renderer::api::resource::view::CameraUniformData>();
    let _ = type_name::<khora_core::renderer::api::resource::view::ViewInfo>();
    let _ = type_name::<khora_core::renderer::api::resource::ViewInfo>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::ViewInfo>,
        PhantomData::<khora_core::renderer::api::resource::view::ViewInfo>,
    );
    let _ = view_info_fields as fn(&khora_core::renderer::api::resource::view::ViewInfo);
    let _ = khora_core::renderer::api::resource::view::ViewInfo::new;
    let _ = khora_core::renderer::api::resource::view::ViewInfo::view_projection_matrix;
    is_debug::<khora_core::renderer::api::resource::view::ViewInfo>();
    is_clone::<khora_core::renderer::api::resource::view::ViewInfo>();
    is_default::<khora_core::renderer::api::resource::view::ViewInfo>();
    let _ = type_name::<khora_core::renderer::api::material::gpu_material::GpuMaterial>();
    let _ = type_name::<khora_core::renderer::api::material::GpuMaterial>();
    same_type(
        PhantomData::<khora_core::renderer::api::material::GpuMaterial>,
        PhantomData::<khora_core::renderer::api::material::gpu_material::GpuMaterial>,
    );
    let _ =
        gpu_material_fields as fn(&khora_core::renderer::api::material::gpu_material::GpuMaterial);
    let _ = khora_core::renderer::api::material::gpu_material::GpuMaterial::bindings;
    is_debug::<khora_core::renderer::api::material::gpu_material::GpuMaterial>();
    is_clone::<khora_core::renderer::api::material::gpu_material::GpuMaterial>();
    is_asset::<khora_core::renderer::api::material::gpu_material::GpuMaterial>();
    let _ = type_name::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    let _ = type_name::<khora_core::renderer::light::CullingUniformsData>();
    same_type(
        PhantomData::<khora_core::renderer::light::CullingUniformsData>,
        PhantomData::<khora_core::renderer::light::uniforms::CullingUniformsData>,
    );
    let _ = culling_uniforms_data_fields
        as fn(&khora_core::renderer::light::uniforms::CullingUniformsData);
    is_debug::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    is_clone::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    is_copy::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    is_partial_eq::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    is_pod::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    is_zeroable::<khora_core::renderer::light::uniforms::CullingUniformsData>();
    let _ = type_name::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    let _ = type_name::<khora_core::renderer::light::DirectionalLightUniform>();
    same_type(
        PhantomData::<khora_core::renderer::light::DirectionalLightUniform>,
        PhantomData::<khora_core::renderer::light::uniforms::DirectionalLightUniform>,
    );
    let _ = directional_light_uniform_fields
        as fn(&khora_core::renderer::light::uniforms::DirectionalLightUniform);
    is_debug::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    is_clone::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    is_copy::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    is_partial_eq::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    is_pod::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    is_zeroable::<khora_core::renderer::light::uniforms::DirectionalLightUniform>();
    let _ = type_name::<khora_core::renderer::light::uniforms::LightingUniforms>();
    let _ = type_name::<khora_core::renderer::light::LightingUniforms>();
    same_type(
        PhantomData::<khora_core::renderer::light::LightingUniforms>,
        PhantomData::<khora_core::renderer::light::uniforms::LightingUniforms>,
    );
    let _ =
        lighting_uniforms_fields as fn(&khora_core::renderer::light::uniforms::LightingUniforms);
    is_debug::<khora_core::renderer::light::uniforms::LightingUniforms>();
    is_clone::<khora_core::renderer::light::uniforms::LightingUniforms>();
    is_copy::<khora_core::renderer::light::uniforms::LightingUniforms>();
    is_partial_eq::<khora_core::renderer::light::uniforms::LightingUniforms>();
    is_pod::<khora_core::renderer::light::uniforms::LightingUniforms>();
    is_zeroable::<khora_core::renderer::light::uniforms::LightingUniforms>();
    let _ = khora_core::renderer::light::uniforms::MAX_DIRECTIONAL_LIGHTS;
    let _ = khora_core::renderer::light::MAX_DIRECTIONAL_LIGHTS;
    let _ = khora_core::renderer::light::uniforms::MAX_POINT_LIGHTS;
    let _ = khora_core::renderer::light::MAX_POINT_LIGHTS;
    let _ = khora_core::renderer::light::uniforms::MAX_SPOT_LIGHTS;
    let _ = khora_core::renderer::light::MAX_SPOT_LIGHTS;
    let _ = type_name::<khora_core::renderer::light::uniforms::PointLightUniform>();
    let _ = type_name::<khora_core::renderer::light::PointLightUniform>();
    same_type(
        PhantomData::<khora_core::renderer::light::PointLightUniform>,
        PhantomData::<khora_core::renderer::light::uniforms::PointLightUniform>,
    );
    let _ =
        point_light_uniform_fields as fn(&khora_core::renderer::light::uniforms::PointLightUniform);
    is_debug::<khora_core::renderer::light::uniforms::PointLightUniform>();
    is_clone::<khora_core::renderer::light::uniforms::PointLightUniform>();
    is_copy::<khora_core::renderer::light::uniforms::PointLightUniform>();
    is_partial_eq::<khora_core::renderer::light::uniforms::PointLightUniform>();
    is_pod::<khora_core::renderer::light::uniforms::PointLightUniform>();
    is_zeroable::<khora_core::renderer::light::uniforms::PointLightUniform>();
    let _ = type_name::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    let _ = type_name::<khora_core::renderer::light::SpotLightUniform>();
    same_type(
        PhantomData::<khora_core::renderer::light::SpotLightUniform>,
        PhantomData::<khora_core::renderer::light::uniforms::SpotLightUniform>,
    );
    let _ =
        spot_light_uniform_fields as fn(&khora_core::renderer::light::uniforms::SpotLightUniform);
    is_debug::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    is_clone::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    is_copy::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    is_partial_eq::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    is_pod::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    is_zeroable::<khora_core::renderer::light::uniforms::SpotLightUniform>();
    let _ = type_name::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    let _ = type_name::<khora_core::renderer::api::material::MaterialUniforms>();
    same_type(
        PhantomData::<khora_core::renderer::api::material::MaterialUniforms>,
        PhantomData::<khora_core::renderer::api::material::uniforms::MaterialUniforms>,
    );
    let _ = material_uniforms_fields
        as fn(&khora_core::renderer::api::material::uniforms::MaterialUniforms);
    let _ = khora_core::renderer::api::material::uniforms::MaterialUniforms::METALLIC;
    let _ = khora_core::renderer::api::material::uniforms::MaterialUniforms::ROUGHNESS;
    let _ = khora_core::renderer::api::material::uniforms::MaterialUniforms::ALPHA_CUTOFF;
    is_debug::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    is_clone::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    is_copy::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    is_partial_eq::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    is_pod::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    is_zeroable::<khora_core::renderer::api::material::uniforms::MaterialUniforms>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::ModelUniforms>();
    same_type(
        PhantomData::<khora_core::renderer::api::gpu_scene::ModelUniforms>,
        PhantomData::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>,
    );
    let _ = model_uniforms_fields
        as fn(&khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms);
    is_debug::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    is_clone::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    is_copy::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    is_partial_eq::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    is_pod::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    is_zeroable::<khora_core::renderer::api::gpu_scene::model_uniforms::ModelUniforms>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::mesh::GpuMesh>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::GpuMesh>();
    same_type(
        PhantomData::<khora_core::renderer::api::gpu_scene::GpuMesh>,
        PhantomData::<khora_core::renderer::api::gpu_scene::mesh::GpuMesh>,
    );
    let _ = gpu_mesh_fields as fn(&khora_core::renderer::api::gpu_scene::mesh::GpuMesh);
    is_asset::<khora_core::renderer::api::gpu_scene::mesh::GpuMesh>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::mesh::Mesh>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::Mesh>();
    same_type(
        PhantomData::<khora_core::renderer::api::gpu_scene::Mesh>,
        PhantomData::<khora_core::renderer::api::gpu_scene::mesh::Mesh>,
    );
    let _ = mesh_fields as fn(&khora_core::renderer::api::gpu_scene::mesh::Mesh);
    let _ = khora_core::renderer::api::gpu_scene::mesh::Mesh::vertex_size;
    let _ = khora_core::renderer::api::gpu_scene::mesh::Mesh::create_vertex_buffer;
    is_debug::<khora_core::renderer::api::gpu_scene::mesh::Mesh>();
    is_asset::<khora_core::renderer::api::gpu_scene::mesh::Mesh>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::render_object::RenderObject>();
    let _ = type_name::<khora_core::renderer::api::gpu_scene::RenderObject>();
    same_type(
        PhantomData::<khora_core::renderer::api::gpu_scene::RenderObject>,
        PhantomData::<khora_core::renderer::api::gpu_scene::render_object::RenderObject>,
    );
    let _ = render_object_fields
        as fn(&khora_core::renderer::api::gpu_scene::render_object::RenderObject);
    is_debug::<khora_core::renderer::api::gpu_scene::render_object::RenderObject>();
    is_clone::<khora_core::renderer::api::gpu_scene::render_object::RenderObject>();
    let _ = type_name::<khora_core::renderer::api::shader::defs::ShaderDefs>();
    let _ = type_name::<khora_core::renderer::api::ShaderDefs>();
    same_type(
        PhantomData::<khora_core::renderer::api::ShaderDefs>,
        PhantomData::<khora_core::renderer::api::shader::defs::ShaderDefs>,
    );
    let _ = khora_core::renderer::api::shader::defs::ShaderDefs::MAX_DIRECTIONAL_LIGHTS;
    let _ = khora_core::renderer::api::shader::defs::ShaderDefs::MAX_POINT_LIGHTS;
    let _ = khora_core::renderer::api::shader::defs::ShaderDefs::MAX_SPOT_LIGHTS;
    let _ = khora_core::renderer::api::shader::defs::ShaderDefs::MAX_LIGHTS_PER_TILE;
    let _ = khora_core::renderer::api::shader::defs::ShaderDefs::SHADOW_CUBE_NEAR;
    let _ = type_name::<khora_core::renderer::api::shadow::ShadowEntries>();
    let _ = shadow_entries_fields as fn(&khora_core::renderer::api::shadow::ShadowEntries);
    let _ = khora_core::renderer::api::shadow::ShadowEntries::insert;
    let _ = khora_core::renderer::api::shadow::ShadowEntries::get;
    let _ = khora_core::renderer::api::shadow::ShadowEntries::len;
    let _ = khora_core::renderer::api::shadow::ShadowEntries::is_empty;
    is_debug::<khora_core::renderer::api::shadow::ShadowEntries>();
    is_default::<khora_core::renderer::api::shadow::ShadowEntries>();
    is_clone::<khora_core::renderer::api::shadow::ShadowEntries>();
    let _ = type_name::<khora_core::renderer::api::shadow::ShadowEntry>();
    let _ = shadow_entry_variants as fn(&khora_core::renderer::api::shadow::ShadowEntry);
    is_debug::<khora_core::renderer::api::shadow::ShadowEntry>();
    is_clone::<khora_core::renderer::api::shadow::ShadowEntry>();
    let _ = type_name::<khora_core::renderer::api::shadow::ShadowFrame>();
    let _ = shadow_frame_fields as fn(&khora_core::renderer::api::shadow::ShadowFrame);
    let _ = khora_core::renderer::api::shadow::ShadowFrame::bindings;
    let _ = khora_core::renderer::api::shadow::ShadowFrame::entry;
    is_debug::<khora_core::renderer::api::shadow::ShadowFrame>();
    is_clone::<khora_core::renderer::api::shadow::ShadowFrame>();
    is_default::<khora_core::renderer::api::shadow::ShadowFrame>();
    let _ = type_name::<khora_core::renderer::api::shadow::bindings::ShadowGpuBindings>();
    let _ = type_name::<khora_core::renderer::api::shadow::ShadowGpuBindings>();
    same_type(
        PhantomData::<khora_core::renderer::api::shadow::ShadowGpuBindings>,
        PhantomData::<khora_core::renderer::api::shadow::bindings::ShadowGpuBindings>,
    );
    let _ = shadow_gpu_bindings_fields
        as fn(&khora_core::renderer::api::shadow::bindings::ShadowGpuBindings);
    is_debug::<khora_core::renderer::api::shadow::bindings::ShadowGpuBindings>();
    is_clone::<khora_core::renderer::api::shadow::bindings::ShadowGpuBindings>();
    is_copy::<khora_core::renderer::api::shadow::bindings::ShadowGpuBindings>();
    let _ = khora_core::renderer::api::shadow::bindings::binding::ATLAS_2D;
    let _ = khora_core::renderer::api::shadow::bindings::binding::ATLAS_CUBE;
    let _ = khora_core::renderer::api::shadow::bindings::binding::LIGHTING_UNIFORMS;
    let _ = khora_core::renderer::api::shadow::bindings::binding::SAMPLER;
    let _ = khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries;
    let _ = khora_core::renderer::api::shadow::fill_shadow_bind_group_entries;
    same_item(
        &khora_core::renderer::api::shadow::fill_shadow_bind_group_entries,
        &khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries,
    );
    let _ = khora_core::renderer::api::shadow::bindings::shadow_bind_group_layout_entries;
    let _ = khora_core::renderer::api::shadow::shadow_bind_group_layout_entries;
    same_item(
        &khora_core::renderer::api::shadow::shadow_bind_group_layout_entries,
        &khora_core::renderer::api::shadow::bindings::shadow_bind_group_layout_entries,
    );
    // trait `khora_core::renderer::api::text::TextLayout`: see `text_layout_trait_items`
    // trait `khora_core::renderer::api::text::TextRenderer`: see `text_renderer_trait_items`
    let _ = type_name::<khora_core::renderer::api::util::AtlasRect>();
    let _ = atlas_rect_fields as fn(&khora_core::renderer::api::util::AtlasRect);
    is_debug::<khora_core::renderer::api::util::AtlasRect>();
    is_clone::<khora_core::renderer::api::util::AtlasRect>();
    is_copy::<khora_core::renderer::api::util::AtlasRect>();
    is_default::<khora_core::renderer::api::util::AtlasRect>();
    is_partial_eq::<khora_core::renderer::api::util::AtlasRect>();
    let _ = type_name::<khora_core::renderer::api::util::TextureAtlas>();
    let _ = khora_core::renderer::api::util::TextureAtlas::new;
    let _ = khora_core::renderer::api::util::TextureAtlas::allocate_and_upload;
    let _ = khora_core::renderer::api::util::TextureAtlas::texture;
    let _ = khora_core::renderer::api::util::TextureAtlas::view;
    let _ = khora_core::renderer::api::util::TextureAtlas::size;
    let _ = type_name::<khora_core::renderer::api::device::GraphicsBackendType>();
    let _ = type_name::<khora_core::renderer::api::device::GraphicsBackendType>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::GraphicsBackendType>,
        PhantomData::<khora_core::renderer::api::device::GraphicsBackendType>,
    );
    let _ = graphics_backend_type_variants
        as fn(&khora_core::renderer::api::device::GraphicsBackendType);
    is_debug::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_clone::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_copy::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_partial_eq::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_eq::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_hash::<khora_core::renderer::api::device::GraphicsBackendType>();
    is_default::<khora_core::renderer::api::device::GraphicsBackendType>();
    let _ = type_name::<khora_core::renderer::api::resource::IndexFormat>();
    let _ = type_name::<khora_core::renderer::api::resource::IndexFormat>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::IndexFormat>,
        PhantomData::<khora_core::renderer::api::resource::IndexFormat>,
    );
    let _ = index_format_variants as fn(&khora_core::renderer::api::resource::IndexFormat);
    is_debug::<khora_core::renderer::api::resource::IndexFormat>();
    is_clone::<khora_core::renderer::api::resource::IndexFormat>();
    is_copy::<khora_core::renderer::api::resource::IndexFormat>();
    is_partial_eq::<khora_core::renderer::api::resource::IndexFormat>();
    is_eq::<khora_core::renderer::api::resource::IndexFormat>();
    is_hash::<khora_core::renderer::api::resource::IndexFormat>();
    let _ = type_name::<khora_core::renderer::api::device::RenderStrategy>();
    let _ = type_name::<khora_core::renderer::api::device::RenderStrategy>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::RenderStrategy>,
        PhantomData::<khora_core::renderer::api::device::RenderStrategy>,
    );
    let _ = render_strategy_variants as fn(&khora_core::renderer::api::device::RenderStrategy);
    is_debug::<khora_core::renderer::api::device::RenderStrategy>();
    is_clone::<khora_core::renderer::api::device::RenderStrategy>();
    is_copy::<khora_core::renderer::api::device::RenderStrategy>();
    is_partial_eq::<khora_core::renderer::api::device::RenderStrategy>();
    is_eq::<khora_core::renderer::api::device::RenderStrategy>();
    let _ = type_name::<khora_core::renderer::api::device::RendererDeviceType>();
    let _ = type_name::<khora_core::renderer::api::device::RendererDeviceType>();
    same_type(
        PhantomData::<khora_core::renderer::api::device::RendererDeviceType>,
        PhantomData::<khora_core::renderer::api::device::RendererDeviceType>,
    );
    let _ =
        renderer_device_type_variants as fn(&khora_core::renderer::api::device::RendererDeviceType);
    is_debug::<khora_core::renderer::api::device::RendererDeviceType>();
    is_clone::<khora_core::renderer::api::device::RendererDeviceType>();
    is_copy::<khora_core::renderer::api::device::RendererDeviceType>();
    is_partial_eq::<khora_core::renderer::api::device::RendererDeviceType>();
    is_eq::<khora_core::renderer::api::device::RendererDeviceType>();
    is_hash::<khora_core::renderer::api::device::RendererDeviceType>();
    is_default::<khora_core::renderer::api::device::RendererDeviceType>();
    let _ = type_name::<khora_core::renderer::api::resource::SampleCount>();
    let _ = type_name::<khora_core::renderer::api::resource::SampleCount>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::SampleCount>,
        PhantomData::<khora_core::renderer::api::resource::SampleCount>,
    );
    let _ = sample_count_variants as fn(&khora_core::renderer::api::resource::SampleCount);
    is_debug::<khora_core::renderer::api::resource::SampleCount>();
    is_clone::<khora_core::renderer::api::resource::SampleCount>();
    is_copy::<khora_core::renderer::api::resource::SampleCount>();
    is_partial_eq::<khora_core::renderer::api::resource::SampleCount>();
    is_eq::<khora_core::renderer::api::resource::SampleCount>();
    is_hash::<khora_core::renderer::api::resource::SampleCount>();
    is_default::<khora_core::renderer::api::resource::SampleCount>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderStage>();
    let _ = type_name::<khora_core::renderer::api::shader::ShaderStage>();
    same_type(
        PhantomData::<khora_core::renderer::api::shader::ShaderStage>,
        PhantomData::<khora_core::renderer::api::shader::ShaderStage>,
    );
    let _ = shader_stage_variants as fn(&khora_core::renderer::api::shader::ShaderStage);
    is_debug::<khora_core::renderer::api::shader::ShaderStage>();
    is_clone::<khora_core::renderer::api::shader::ShaderStage>();
    is_copy::<khora_core::renderer::api::shader::ShaderStage>();
    is_partial_eq::<khora_core::renderer::api::shader::ShaderStage>();
    is_eq::<khora_core::renderer::api::shader::ShaderStage>();
    is_hash::<khora_core::renderer::api::shader::ShaderStage>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureColorSpace>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureColorSpace>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureColorSpace>,
        PhantomData::<khora_core::renderer::api::resource::TextureColorSpace>,
    );
    let _ =
        texture_color_space_variants as fn(&khora_core::renderer::api::resource::TextureColorSpace);
    is_debug::<khora_core::renderer::api::resource::TextureColorSpace>();
    is_clone::<khora_core::renderer::api::resource::TextureColorSpace>();
    is_copy::<khora_core::renderer::api::resource::TextureColorSpace>();
    is_partial_eq::<khora_core::renderer::api::resource::TextureColorSpace>();
    is_eq::<khora_core::renderer::api::resource::TextureColorSpace>();
    is_hash::<khora_core::renderer::api::resource::TextureColorSpace>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureFormat>();
    let _ = type_name::<khora_core::renderer::api::resource::TextureFormat>();
    same_type(
        PhantomData::<khora_core::renderer::api::resource::TextureFormat>,
        PhantomData::<khora_core::renderer::api::resource::TextureFormat>,
    );
    let _ = texture_format_variants as fn(&khora_core::renderer::api::resource::TextureFormat);
    let _ = khora_core::renderer::api::resource::TextureFormat::with_color_space;
    let _ = khora_core::renderer::api::resource::TextureFormat::bytes_per_pixel;
    is_debug::<khora_core::renderer::api::resource::TextureFormat>();
    is_clone::<khora_core::renderer::api::resource::TextureFormat>();
    is_copy::<khora_core::renderer::api::resource::TextureFormat>();
    is_partial_eq::<khora_core::renderer::api::resource::TextureFormat>();
    is_eq::<khora_core::renderer::api::resource::TextureFormat>();
    is_hash::<khora_core::renderer::api::resource::TextureFormat>();
    let _ = type_name::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    let _ = type_name::<khora_core::renderer::api::util::ShaderStageFlags>();
    same_type(
        PhantomData::<khora_core::renderer::api::util::ShaderStageFlags>,
        PhantomData::<khora_core::renderer::api::util::flags::ShaderStageFlags>,
    );
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::EMPTY;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::ALL_DECLARED;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::from_bits_truncate;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::bits;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::contains;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::intersects;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::insert;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::remove;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::toggle;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::with;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::without;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::VERTEX;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::FRAGMENT;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::COMPUTE;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::VERTEX_FRAGMENT;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::ALL;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::NONE;
    let _ = khora_core::renderer::api::util::flags::ShaderStageFlags::from_stage;
    is_clone::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_copy::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_partial_eq::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_eq::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_hash::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_default::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_or::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_and::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_xor::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_not::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_or_assign::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_and_assign::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_bit_xor_assign::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    is_debug::<khora_core::renderer::api::util::flags::ShaderStageFlags>();
    let _ = khora_core::renderer::api::util::half_float::F16_MAX;
    let _ = khora_core::renderer::api::util::F16_MAX;
    let _ = khora_core::renderer::api::util::half_float::f32_to_f16_bits;
    let _ = khora_core::renderer::api::util::f32_to_f16_bits;
    same_item(
        &khora_core::renderer::api::util::f32_to_f16_bits,
        &khora_core::renderer::api::util::half_float::f32_to_f16_bits,
    );
    let _ = type_name::<khora_core::renderer::error::PipelineError>();
    let _ = type_name::<khora_core::renderer::PipelineError>();
    same_type(
        PhantomData::<khora_core::renderer::PipelineError>,
        PhantomData::<khora_core::renderer::error::PipelineError>,
    );
    let _ = pipeline_error_variants as fn(&khora_core::renderer::error::PipelineError);
    is_debug::<khora_core::renderer::error::PipelineError>();
    is_display::<khora_core::renderer::error::PipelineError>();
    is_error::<khora_core::renderer::error::PipelineError>();
    is_from_pipeline_error::<khora_core::renderer::error::ResourceError>();
    let _ = type_name::<khora_core::renderer::error::RenderError>();
    let _ = type_name::<khora_core::renderer::RenderError>();
    same_type(
        PhantomData::<khora_core::renderer::RenderError>,
        PhantomData::<khora_core::renderer::error::RenderError>,
    );
    let _ = render_error_variants as fn(&khora_core::renderer::error::RenderError);
    let _ = khora_core::renderer::error::RenderError::is_fatal;
    is_debug::<khora_core::renderer::error::RenderError>();
    is_display::<khora_core::renderer::error::RenderError>();
    is_error::<khora_core::renderer::error::RenderError>();
    is_from_resource_error::<khora_core::renderer::error::RenderError>();
    let _ = type_name::<khora_core::renderer::error::ResourceError>();
    let _ = type_name::<khora_core::renderer::ResourceError>();
    same_type(
        PhantomData::<khora_core::renderer::ResourceError>,
        PhantomData::<khora_core::renderer::error::ResourceError>,
    );
    let _ = resource_error_variants as fn(&khora_core::renderer::error::ResourceError);
    is_debug::<khora_core::renderer::error::ResourceError>();
    is_display::<khora_core::renderer::error::ResourceError>();
    is_error::<khora_core::renderer::error::ResourceError>();
    is_from_shader_error::<khora_core::renderer::error::ResourceError>();
    let _ = type_name::<khora_core::renderer::error::ShaderError>();
    let _ = type_name::<khora_core::renderer::ShaderError>();
    same_type(
        PhantomData::<khora_core::renderer::ShaderError>,
        PhantomData::<khora_core::renderer::error::ShaderError>,
    );
    let _ = shader_error_variants as fn(&khora_core::renderer::error::ShaderError);
    is_debug::<khora_core::renderer::error::ShaderError>();
    is_display::<khora_core::renderer::error::ShaderError>();
    is_error::<khora_core::renderer::error::ShaderError>();
    let _ = type_name::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    let _ = type_name::<khora_core::renderer::ForwardPlusTileConfig>();
    same_type(
        PhantomData::<khora_core::renderer::ForwardPlusTileConfig>,
        PhantomData::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>,
    );
    let _ = forward_plus_tile_config_fields
        as fn(&khora_core::renderer::light::forward_plus::ForwardPlusTileConfig);
    let _ = khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::new;
    let _ = khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::high_light_count;
    let _ = khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::low_overhead;
    let _ = khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::tile_dimensions;
    let _ = khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::total_tiles;
    let _ =
        khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::light_index_buffer_size;
    let _ =
        khora_core::renderer::light::forward_plus::ForwardPlusTileConfig::light_grid_buffer_size;
    is_debug::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    is_clone::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    is_copy::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    is_partial_eq::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    is_eq::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    is_default::<khora_core::renderer::light::forward_plus::ForwardPlusTileConfig>();
    let _ = type_name::<khora_core::renderer::light::forward_plus::GpuLight>();
    let _ = type_name::<khora_core::renderer::GpuLight>();
    same_type(
        PhantomData::<khora_core::renderer::GpuLight>,
        PhantomData::<khora_core::renderer::light::forward_plus::GpuLight>,
    );
    let _ = gpu_light_fields as fn(&khora_core::renderer::light::forward_plus::GpuLight);
    let _ = khora_core::renderer::light::forward_plus::GpuLight::TYPE_DIRECTIONAL;
    let _ = khora_core::renderer::light::forward_plus::GpuLight::TYPE_POINT;
    let _ = khora_core::renderer::light::forward_plus::GpuLight::TYPE_SPOT;
    let _ = khora_core::renderer::light::forward_plus::GpuLight::from_parts;
    is_debug::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_clone::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_copy::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_partial_eq::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_pod::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_zeroable::<khora_core::renderer::light::forward_plus::GpuLight>();
    is_default::<khora_core::renderer::light::forward_plus::GpuLight>();
    let _ = type_name::<khora_core::renderer::light::forward_plus::TileSize>();
    let _ = type_name::<khora_core::renderer::TileSize>();
    same_type(
        PhantomData::<khora_core::renderer::TileSize>,
        PhantomData::<khora_core::renderer::light::forward_plus::TileSize>,
    );
    let _ = tile_size_variants as fn(&khora_core::renderer::light::forward_plus::TileSize);
    let _ = khora_core::renderer::light::forward_plus::TileSize::pixels;
    let _ = khora_core::renderer::light::forward_plus::TileSize::tile_count;
    is_debug::<khora_core::renderer::light::forward_plus::TileSize>();
    is_clone::<khora_core::renderer::light::forward_plus::TileSize>();
    is_copy::<khora_core::renderer::light::forward_plus::TileSize>();
    is_partial_eq::<khora_core::renderer::light::forward_plus::TileSize>();
    is_eq::<khora_core::renderer::light::forward_plus::TileSize>();
    is_hash::<khora_core::renderer::light::forward_plus::TileSize>();
    is_default::<khora_core::renderer::light::forward_plus::TileSize>();
    let _ = type_name::<khora_core::renderer::light::DirectionalLight>();
    let _ = type_name::<khora_core::renderer::DirectionalLight>();
    same_type(
        PhantomData::<khora_core::renderer::DirectionalLight>,
        PhantomData::<khora_core::renderer::light::DirectionalLight>,
    );
    let _ = directional_light_fields as fn(&khora_core::renderer::light::DirectionalLight);
    is_debug::<khora_core::renderer::light::DirectionalLight>();
    is_clone::<khora_core::renderer::light::DirectionalLight>();
    is_copy::<khora_core::renderer::light::DirectionalLight>();
    is_partial_eq::<khora_core::renderer::light::DirectionalLight>();
    is_serialize::<khora_core::renderer::light::DirectionalLight>();
    is_deserialize_owned::<khora_core::renderer::light::DirectionalLight>();
    is_default::<khora_core::renderer::light::DirectionalLight>();
    let _ = type_name::<khora_core::renderer::light::LightType>();
    let _ = type_name::<khora_core::renderer::LightType>();
    same_type(
        PhantomData::<khora_core::renderer::LightType>,
        PhantomData::<khora_core::renderer::light::LightType>,
    );
    let _ = light_type_variants as fn(&khora_core::renderer::light::LightType);
    is_debug::<khora_core::renderer::light::LightType>();
    is_clone::<khora_core::renderer::light::LightType>();
    is_copy::<khora_core::renderer::light::LightType>();
    is_partial_eq::<khora_core::renderer::light::LightType>();
    is_serialize::<khora_core::renderer::light::LightType>();
    is_deserialize_owned::<khora_core::renderer::light::LightType>();
    is_default::<khora_core::renderer::light::LightType>();
    let _ = type_name::<khora_core::renderer::light::PointLight>();
    let _ = type_name::<khora_core::renderer::PointLight>();
    same_type(
        PhantomData::<khora_core::renderer::PointLight>,
        PhantomData::<khora_core::renderer::light::PointLight>,
    );
    let _ = point_light_fields as fn(&khora_core::renderer::light::PointLight);
    is_debug::<khora_core::renderer::light::PointLight>();
    is_clone::<khora_core::renderer::light::PointLight>();
    is_copy::<khora_core::renderer::light::PointLight>();
    is_partial_eq::<khora_core::renderer::light::PointLight>();
    is_serialize::<khora_core::renderer::light::PointLight>();
    is_deserialize_owned::<khora_core::renderer::light::PointLight>();
    is_default::<khora_core::renderer::light::PointLight>();
    let _ = type_name::<khora_core::renderer::light::SpotLight>();
    let _ = type_name::<khora_core::renderer::SpotLight>();
    same_type(
        PhantomData::<khora_core::renderer::SpotLight>,
        PhantomData::<khora_core::renderer::light::SpotLight>,
    );
    let _ = spot_light_fields as fn(&khora_core::renderer::light::SpotLight);
    is_debug::<khora_core::renderer::light::SpotLight>();
    is_clone::<khora_core::renderer::light::SpotLight>();
    is_copy::<khora_core::renderer::light::SpotLight>();
    is_partial_eq::<khora_core::renderer::light::SpotLight>();
    is_serialize::<khora_core::renderer::light::SpotLight>();
    is_deserialize_owned::<khora_core::renderer::light::SpotLight>();
    is_default::<khora_core::renderer::light::SpotLight>();
    // trait `khora_core::renderer::traits::CommandEncoder`: see `command_encoder_trait_items`
    // trait `khora_core::renderer::traits::ComputePass`: see `compute_pass_trait_items`
    let _ = type_name::<khora_core::renderer::traits::FrameTargets>();
    let _ = frame_targets_fields as fn(&khora_core::renderer::traits::FrameTargets);
    is_debug::<khora_core::renderer::traits::FrameTargets>();
    is_clone::<khora_core::renderer::traits::FrameTargets>();
    is_copy::<khora_core::renderer::traits::FrameTargets>();
    // trait `khora_core::renderer::traits::GpuProfiler`: see `gpu_profiler_trait_items`
    // trait `khora_core::renderer::traits::GraphicsBackendSelector`: see `graphics_backend_selector_trait_items`
    // trait `khora_core::renderer::traits::PipelineSystem`: see `pipeline_system_trait_items`
    // trait `khora_core::renderer::traits::RenderPass`: see `render_pass_trait_items`
}
