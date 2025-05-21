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

use super::wgpu_graphic_context::WgpuGraphicsContext;
use crate::subsystems::renderer::api::common_types::{
    IndexFormat, RendererAdapterInfo, RendererBackendType, RendererDeviceType, SampleCount,
    TextureFormat,
};
use crate::subsystems::renderer::api::pipeline_types::{
    self as api_pipe, ColorTargetStateDescriptor, RenderPipelineDescriptor, RenderPipelineId,
};
use crate::subsystems::renderer::api::shader_types::{
    ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData,
};
use crate::subsystems::renderer::error::{PipelineError, ResourceError, ShaderError};
use crate::subsystems::renderer::traits::graphics_device::GraphicsDevice;

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use wgpu;

#[allow(dead_code)]
#[derive(Debug)]
struct WgpuShaderModuleEntry {
    wgpu_module: Arc<wgpu::ShaderModule>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct WgpuRenderPipelineEntry {
    wgpu_pipeline: Arc<wgpu::RenderPipeline>,
}

#[derive(Debug)]
pub struct WgpuDevice {
    context: Arc<Mutex<WgpuGraphicsContext>>,
    shader_modules: Mutex<HashMap<ShaderModuleId, WgpuShaderModuleEntry>>,
    pipelines: Mutex<HashMap<RenderPipelineId, WgpuRenderPipelineEntry>>,
    next_shader_id: AtomicUsize,
    next_pipeline_id: AtomicUsize,
}

impl WgpuDevice {
    pub fn new(context: Arc<Mutex<WgpuGraphicsContext>>) -> Self {
        Self {
            context,
            shader_modules: Mutex::new(HashMap::new()),
            pipelines: Mutex::new(HashMap::new()),
            next_shader_id: AtomicUsize::new(0),
            next_pipeline_id: AtomicUsize::new(0),
        }
    }

    fn generate_shader_id(&self) -> ShaderModuleId {
        ShaderModuleId(self.next_shader_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_pipeline_id(&self) -> RenderPipelineId {
        RenderPipelineId(self.next_pipeline_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Helper function to execute an operation with the wgpu::Device locked.
    /// Returns a Result to propagate lock errors or operation errors.
    fn with_wgpu_device<F, R>(&self, operation: F) -> Result<R, ResourceError>
    where
        F: FnOnce(&wgpu::Device) -> Result<R, ResourceError>,
    {
        let context_guard = self.context.lock().map_err(|e| {
            ResourceError::BackendError(format!("Failed to lock WgpuGraphicsContext: {}", e))
        })?;
        operation(&context_guard.device)
    }

    // --- Convert Helpers ---

    /// Converts a VertexFormat from the API to the wgpu format.
    fn convert_vertex_format(api_format: api_pipe::VertexFormat) -> wgpu::VertexFormat {
        match api_format {
            api_pipe::VertexFormat::Uint8x2 => wgpu::VertexFormat::Uint8x2,
            api_pipe::VertexFormat::Uint8x4 => wgpu::VertexFormat::Uint8x4,
            api_pipe::VertexFormat::Sint8x2 => wgpu::VertexFormat::Sint8x2,
            api_pipe::VertexFormat::Sint8x4 => wgpu::VertexFormat::Sint8x4,
            api_pipe::VertexFormat::Unorm8x2 => wgpu::VertexFormat::Unorm8x2,
            api_pipe::VertexFormat::Unorm8x4 => wgpu::VertexFormat::Unorm8x4,
            api_pipe::VertexFormat::Snorm8x2 => wgpu::VertexFormat::Snorm8x2,
            api_pipe::VertexFormat::Snorm8x4 => wgpu::VertexFormat::Snorm8x4,
            api_pipe::VertexFormat::Uint16x2 => wgpu::VertexFormat::Uint16x2,
            api_pipe::VertexFormat::Uint16x4 => wgpu::VertexFormat::Uint16x4,
            api_pipe::VertexFormat::Sint16x2 => wgpu::VertexFormat::Sint16x2,
            api_pipe::VertexFormat::Sint16x4 => wgpu::VertexFormat::Sint16x4,
            api_pipe::VertexFormat::Unorm16x2 => wgpu::VertexFormat::Unorm16x2,
            api_pipe::VertexFormat::Unorm16x4 => wgpu::VertexFormat::Unorm16x4,
            api_pipe::VertexFormat::Snorm16x2 => wgpu::VertexFormat::Snorm16x2,
            api_pipe::VertexFormat::Snorm16x4 => wgpu::VertexFormat::Snorm16x4,
            api_pipe::VertexFormat::Float16x2 => wgpu::VertexFormat::Float16x2,
            api_pipe::VertexFormat::Float16x4 => wgpu::VertexFormat::Float16x4,
            api_pipe::VertexFormat::Float32 => wgpu::VertexFormat::Float32,
            api_pipe::VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
            api_pipe::VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
            api_pipe::VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
            api_pipe::VertexFormat::Uint32 => wgpu::VertexFormat::Uint32,
            api_pipe::VertexFormat::Uint32x2 => wgpu::VertexFormat::Uint32x2,
            api_pipe::VertexFormat::Uint32x3 => wgpu::VertexFormat::Uint32x3,
            api_pipe::VertexFormat::Uint32x4 => wgpu::VertexFormat::Uint32x4,
            api_pipe::VertexFormat::Sint32 => wgpu::VertexFormat::Sint32,
            api_pipe::VertexFormat::Sint32x2 => wgpu::VertexFormat::Sint32x2,
            api_pipe::VertexFormat::Sint32x3 => wgpu::VertexFormat::Sint32x3,
            api_pipe::VertexFormat::Sint32x4 => wgpu::VertexFormat::Sint32x4,
        }
    }

    /// Converts a TextureFormat from the API to the wgpu format.
    fn convert_texture_format(api_format: TextureFormat) -> Option<wgpu::TextureFormat> {
        match api_format {
            TextureFormat::R8Unorm => Some(wgpu::TextureFormat::R8Unorm),
            TextureFormat::Rg8Unorm => Some(wgpu::TextureFormat::Rg8Unorm),
            TextureFormat::Rgba8Unorm => Some(wgpu::TextureFormat::Rgba8Unorm),
            TextureFormat::Rgba8UnormSrgb => Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            TextureFormat::Bgra8UnormSrgb => Some(wgpu::TextureFormat::Bgra8UnormSrgb),
            TextureFormat::R16Float => Some(wgpu::TextureFormat::R16Float),
            TextureFormat::Rg16Float => Some(wgpu::TextureFormat::Rg16Float),
            TextureFormat::Rgba16Float => Some(wgpu::TextureFormat::Rgba16Float),
            TextureFormat::R32Float => Some(wgpu::TextureFormat::R32Float),
            TextureFormat::Rg32Float => Some(wgpu::TextureFormat::Rg32Float),
            TextureFormat::Rgba32Float => Some(wgpu::TextureFormat::Rgba32Float),
            TextureFormat::Depth16Unorm => Some(wgpu::TextureFormat::Depth16Unorm),
            TextureFormat::Depth24Plus => Some(wgpu::TextureFormat::Depth24Plus),
            TextureFormat::Depth24PlusStencil8 => Some(wgpu::TextureFormat::Depth24PlusStencil8),
            TextureFormat::Depth32Float => Some(wgpu::TextureFormat::Depth32Float),
            TextureFormat::Depth32FloatStencil8 => Some(wgpu::TextureFormat::Depth32FloatStencil8),
        }
    }

    /// Converts an IndexFormat from the API to the wgpu format.
    fn convert_index_format(api_format: IndexFormat) -> wgpu::IndexFormat {
        match api_format {
            IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        }
    }

    /// Converts a CompareFunction from the API to the wgpu format.
    fn convert_compare_function(api_func: api_pipe::CompareFunction) -> wgpu::CompareFunction {
        match api_func {
            api_pipe::CompareFunction::Never => wgpu::CompareFunction::Never,
            api_pipe::CompareFunction::Less => wgpu::CompareFunction::Less,
            api_pipe::CompareFunction::Equal => wgpu::CompareFunction::Equal,
            api_pipe::CompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
            api_pipe::CompareFunction::Greater => wgpu::CompareFunction::Greater,
            api_pipe::CompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
            api_pipe::CompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            api_pipe::CompareFunction::Always => wgpu::CompareFunction::Always,
        }
    }

    /// Converts a StencilOperation from the API to the wgpu format.
    fn convert_stencil_operation(api_op: api_pipe::StencilOperation) -> wgpu::StencilOperation {
        match api_op {
            api_pipe::StencilOperation::Keep => wgpu::StencilOperation::Keep,
            api_pipe::StencilOperation::Zero => wgpu::StencilOperation::Zero,
            api_pipe::StencilOperation::Replace => wgpu::StencilOperation::Replace,
            api_pipe::StencilOperation::Invert => wgpu::StencilOperation::Invert,
            api_pipe::StencilOperation::IncrementClamp => wgpu::StencilOperation::IncrementClamp,
            api_pipe::StencilOperation::DecrementClamp => wgpu::StencilOperation::DecrementClamp,
            api_pipe::StencilOperation::IncrementWrap => wgpu::StencilOperation::IncrementWrap,
            api_pipe::StencilOperation::DecrementWrap => wgpu::StencilOperation::DecrementWrap,
        }
    }

    /// Converts a BlendFactor from the API to the wgpu format.
    fn convert_blend_factor(api_factor: api_pipe::BlendFactor) -> wgpu::BlendFactor {
        match api_factor {
            api_pipe::BlendFactor::One => wgpu::BlendFactor::One,
            api_pipe::BlendFactor::Zero => wgpu::BlendFactor::Zero,
            api_pipe::BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
            api_pipe::BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        }
    }

    /// Converts a BlendOperation from the API to the wgpu format.
    fn convert_blend_operation(api_op: api_pipe::BlendOperation) -> wgpu::BlendOperation {
        match api_op {
            api_pipe::BlendOperation::Add => wgpu::BlendOperation::Add,
            api_pipe::BlendOperation::Subtract => wgpu::BlendOperation::Subtract,
            api_pipe::BlendOperation::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
            api_pipe::BlendOperation::Min => wgpu::BlendOperation::Min,
            api_pipe::BlendOperation::Max => wgpu::BlendOperation::Max,
        }
    }

    /// Converts a SampleCount from the API to the wgpu format.
    fn convert_sample_count(api_count: SampleCount) -> u32 {
        match api_count {
            SampleCount::X1 => 1,
            SampleCount::X2 => 2,
            SampleCount::X4 => 4,
            SampleCount::X8 => 8,
            SampleCount::X16 => 16,
            SampleCount::X32 => 32,
            SampleCount::X64 => 64,
        }
    }

    /// Converts a ColorWrites from the API to the wgpu format.
    fn convert_color_writes(api_writes: api_pipe::ColorWrites) -> wgpu::ColorWrites {
        let mut wgpu_writes = wgpu::ColorWrites::empty();
        if api_writes.contains(api_pipe::ColorWrites::R) {
            wgpu_writes |= wgpu::ColorWrites::RED;
        }
        if api_writes.contains(api_pipe::ColorWrites::G) {
            wgpu_writes |= wgpu::ColorWrites::GREEN;
        }
        if api_writes.contains(api_pipe::ColorWrites::B) {
            wgpu_writes |= wgpu::ColorWrites::BLUE;
        }
        if api_writes.contains(api_pipe::ColorWrites::A) {
            wgpu_writes |= wgpu::ColorWrites::ALPHA;
        }
        wgpu_writes
    }

    /// Converts an Option<CullMode> from the API to Option<wgpu::Face>.
    fn convert_cull_mode_option(api_cull_mode: Option<api_pipe::CullMode>) -> Option<wgpu::Face> {
        match api_cull_mode {
            Some(api_pipe::CullMode::Front) => Some(wgpu::Face::Front),
            Some(api_pipe::CullMode::Back) => Some(wgpu::Face::Back),
            Some(api_pipe::CullMode::None) => None,
            None => None,
        }
    }
}

impl GraphicsDevice for WgpuDevice {
    fn create_shader_module(
        &self,
        descriptor: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        let wgpu_source = match &descriptor.source {
            ShaderSourceData::Wgsl(cow_str) => wgpu::ShaderSource::Wgsl(cow_str.clone()),
        };

        let label = descriptor.label;

        // Create the shader module using the wgpu device
        let wgpu_module_arc = self.with_wgpu_device(|device| {
            log::debug!(
                "WgpuDevice: Creating wgpu::ShaderModule with label: {:?}, stage: {:?}, entry: {}",
                label,
                descriptor.stage,
                descriptor.entry_point
            );
            let wgpu_descriptor = wgpu::ShaderModuleDescriptor {
                label,
                source: wgpu_source,
            };
            Ok(Arc::new(device.create_shader_module(wgpu_descriptor)))
        })?;

        // Create a new shader module entry and insert it into the shader_modules map
        let id = self.generate_shader_id();
        let mut modules_guard = self.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {}", e))
        })?;
        modules_guard.insert(
            id,
            WgpuShaderModuleEntry {
                wgpu_module: wgpu_module_arc,
            },
        );

        log::info!(
            "WgpuDevice: Successfully created shader module '{:?}' with ID: {:?}",
            label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        let mut modules_guard = self.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {}", e))
        })?;

        if modules_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed shader module with ID: {:?}", id);
            Ok(())
        } else {
            Err(ShaderError::NotFound { id }.into())
        }
    }

    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        log::debug!(
            "WgpuDevice: Creating render pipeline with label: {:?}",
            descriptor.label
        );

        // 1. Get the shader modules from the context
        let shader_modules_map = self.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {}", e))
        })?;

        let vs_module_entry = shader_modules_map
            .get(&descriptor.vertex_shader_module)
            .ok_or_else(|| {
                ResourceError::Pipeline(PipelineError::InvalidShaderModuleForPipeline {
                    id: descriptor.vertex_shader_module,
                    pipeline_label: descriptor.label.as_deref().map(String::from),
                })
            })?;
        let vs_wgpu_module: &Arc<wgpu::ShaderModule> = &vs_module_entry.wgpu_module;

        let fs_wgpu_module_opt = if let Some(fs_id) = descriptor.fragment_shader_module {
            let fs_module_entry = shader_modules_map.get(&fs_id).ok_or_else(|| {
                ResourceError::Pipeline(PipelineError::InvalidShaderModuleForPipeline {
                    id: fs_id,
                    pipeline_label: descriptor.label.as_deref().map(String::from),
                })
            })?;
            Some(&fs_module_entry.wgpu_module)
        } else {
            None
        };

        // 2. Convert vertex buffers layout
        let wgpu_vertex_attributes_storage: Vec<Vec<wgpu::VertexAttribute>> = descriptor
            .vertex_buffers_layout
            .as_ref()
            .iter()
            .map(|vb_layout_desc| {
                vb_layout_desc
                    .attributes
                    .as_ref()
                    .iter()
                    .map(|attr_desc| wgpu::VertexAttribute {
                        format: Self::convert_vertex_format(attr_desc.format),
                        offset: attr_desc.offset,
                        shader_location: attr_desc.shader_location,
                    })
                    .collect()
            })
            .collect();

        let wgpu_vertex_buffers_layouts: Vec<wgpu::VertexBufferLayout> = descriptor
            .vertex_buffers_layout
            .as_ref()
            .iter()
            .zip(wgpu_vertex_attributes_storage.iter())
            .map(
                |(vb_layout_desc, attributes_for_this_layout)| wgpu::VertexBufferLayout {
                    array_stride: vb_layout_desc.array_stride,
                    step_mode: match vb_layout_desc.step_mode {
                        api_pipe::VertexStepMode::Vertex => wgpu::VertexStepMode::Vertex,
                        api_pipe::VertexStepMode::Instance => wgpu::VertexStepMode::Instance,
                    },
                    attributes: attributes_for_this_layout,
                },
            )
            .collect();

        // 3. Converts primitive state
        let primitive_state = wgpu::PrimitiveState {
            topology: match descriptor.primitive_state.topology {
                api_pipe::PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
                api_pipe::PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
                api_pipe::PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
                api_pipe::PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
                api_pipe::PrimitiveTopology::TriangleStrip => {
                    wgpu::PrimitiveTopology::TriangleStrip
                }
            },
            strip_index_format: descriptor
                .primitive_state
                .strip_index_format
                .map(Self::convert_index_format),
            front_face: match descriptor.primitive_state.front_face {
                api_pipe::FrontFace::Ccw => wgpu::FrontFace::Ccw,
                api_pipe::FrontFace::Cw => wgpu::FrontFace::Cw,
            },
            cull_mode: Self::convert_cull_mode_option(descriptor.primitive_state.cull_mode),
            unclipped_depth: descriptor.primitive_state.unclipped_depth,
            polygon_mode: match descriptor.primitive_state.polygon_mode {
                api_pipe::PolygonMode::Fill => wgpu::PolygonMode::Fill,
                api_pipe::PolygonMode::Line => wgpu::PolygonMode::Line,
                api_pipe::PolygonMode::Point => wgpu::PolygonMode::Point,
            },
            conservative: descriptor.primitive_state.conservative,
        };

        // 4. Convert depth stencil state
        let depth_stencil_state: Option<wgpu::DepthStencilState> =
            if let Some(ds_desc) = descriptor.depth_stencil_state.as_ref() {
                let wgpu_format =
                    Self::convert_texture_format(ds_desc.format).ok_or_else(|| {
                        ResourceError::Pipeline(PipelineError::IncompatibleDepthStencilFormat(
                            format!("{:?}", ds_desc.format),
                        ))
                    })?;

                Some(wgpu::DepthStencilState {
                    format: wgpu_format,
                    depth_write_enabled: ds_desc.depth_write_enabled,
                    depth_compare: Self::convert_compare_function(ds_desc.depth_compare),
                    stencil: wgpu::StencilState {
                        front: wgpu::StencilFaceState {
                            compare: Self::convert_compare_function(ds_desc.stencil_front.compare),
                            fail_op: Self::convert_stencil_operation(ds_desc.stencil_front.fail_op),
                            depth_fail_op: Self::convert_stencil_operation(
                                ds_desc.stencil_front.depth_fail_op,
                            ),
                            pass_op: Self::convert_stencil_operation(
                                ds_desc.stencil_front.depth_pass_op,
                            ),
                        },
                        back: wgpu::StencilFaceState {
                            compare: Self::convert_compare_function(ds_desc.stencil_back.compare),
                            fail_op: Self::convert_stencil_operation(ds_desc.stencil_back.fail_op),
                            depth_fail_op: Self::convert_stencil_operation(
                                ds_desc.stencil_back.depth_fail_op,
                            ),
                            pass_op: Self::convert_stencil_operation(
                                ds_desc.stencil_back.depth_pass_op,
                            ),
                        },
                        read_mask: ds_desc.stencil_read_mask,
                        write_mask: ds_desc.stencil_write_mask,
                    },
                    bias: wgpu::DepthBiasState {
                        constant: ds_desc.bias.constant,
                        slope_scale: ds_desc.bias.slope_scale,
                        clamp: ds_desc.bias.clamp,
                    },
                })
            } else {
                None
            };

        // 5. Convert color target states
        let wgpu_color_targets_cow: Vec<Option<wgpu::ColorTargetState>> = descriptor
            .color_target_states
            .as_ref()
            .iter()
            .map(|ct_desc: &ColorTargetStateDescriptor| {
                let wgpu_format =
                    Self::convert_texture_format(ct_desc.format).ok_or_else(|| {
                        ResourceError::Pipeline(PipelineError::IncompatibleColorTarget(format!(
                            "{:?}",
                            ct_desc.format
                        )))
                    })?;

                Ok(Some(wgpu::ColorTargetState {
                    format: wgpu_format,
                    blend: ct_desc.blend.map(|b_desc| wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: Self::convert_blend_factor(b_desc.color.src_factor),
                            dst_factor: Self::convert_blend_factor(b_desc.color.dst_factor),
                            operation: Self::convert_blend_operation(b_desc.color.operation),
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: Self::convert_blend_factor(b_desc.alpha.src_factor),
                            dst_factor: Self::convert_blend_factor(b_desc.alpha.dst_factor),
                            operation: Self::convert_blend_operation(b_desc.alpha.operation),
                        },
                    }),
                    write_mask: Self::convert_color_writes(ct_desc.write_mask),
                }))
            })
            .collect::<Result<Vec<_>, ResourceError>>()?;

        // 6. Convert multisample state
        let multisample_state = wgpu::MultisampleState {
            count: Self::convert_sample_count(descriptor.multisample_state.count),
            mask: descriptor.multisample_state.mask as u64,
            alpha_to_coverage_enabled: descriptor.multisample_state.alpha_to_coverage_enabled,
        };

        // 7. Create pipeline layout and render pipeline
        let (wgpu_render_pipeline_arc, id) = self.with_wgpu_device(|device| {
            let pipeline_layout_label = descriptor.label.as_deref().map(|s| format!("{}_Layout", s));
            let wgpu_pipeline_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: pipeline_layout_label.as_deref(),
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                });

            let wgpu_pipeline_descriptor = wgpu::RenderPipelineDescriptor {
                label: descriptor.label.as_deref(),
                layout: Some(&wgpu_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: vs_wgpu_module,
                    entry_point: Some(descriptor.vertex_entry_point.as_ref()),
                    buffers: &wgpu_vertex_buffers_layouts,
                    compilation_options: Default::default(),
                },
                fragment: if let Some(fs_module) = fs_wgpu_module_opt {
                    let entry_point_cow = descriptor.fragment_entry_point.as_ref().ok_or_else(|| {
                        log::error!(
                            "Logic error: Fragment shader module {:?} present but no entry point provided for pipeline {:?}.",
                            descriptor.fragment_shader_module,
                            descriptor.label
                        );
                        ResourceError::Pipeline(PipelineError::MissingEntryPointForFragmentShader {
                            pipeline_label: descriptor.label.as_deref().map(String::from),
                            shader_id: descriptor.fragment_shader_module.unwrap()
                        })
                    })?;

                    Some(wgpu::FragmentState {
                        module: fs_module,
                        entry_point: Some(entry_point_cow.as_ref()),
                        targets: &wgpu_color_targets_cow,
                        compilation_options: Default::default(),
                    })
                } else {
                    None
                },
                primitive: primitive_state,
                depth_stencil: depth_stencil_state,
                multisample: multisample_state,
                multiview: None,
                cache: None,
            };

            // TODO: push_error_scope / pop_error_scope for better error handling
            let pipeline = device.create_render_pipeline(&wgpu_pipeline_descriptor);
            let new_id = self.generate_pipeline_id();
            Ok((Arc::new(pipeline), new_id))
        })?;

        let mut pipelines_guard = self.pipelines.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (pipelines): {}", e))
        })?;
        pipelines_guard.insert(
            id,
            WgpuRenderPipelineEntry {
                wgpu_pipeline: wgpu_render_pipeline_arc,
            },
        );

        log::info!(
            "WgpuDevice: Successfully created render pipeline '{:?}' with ID: {:?}",
            descriptor.label.as_deref().unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        let mut pipelines_guard = self.pipelines.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (pipelines): {}", e))
        })?;

        if pipelines_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed render pipeline with ID: {:?}", id);
            Ok(())
        } else {
            Err(PipelineError::InvalidRenderPipeline { id }.into())
        }
    }

    fn get_adapter_info(&self) -> RendererAdapterInfo {
        let context_guard = self
            .context
            .lock()
            .expect("WgpuDevice: Mutex poisoned (context) on get_adapter_info");
        RendererAdapterInfo {
            name: context_guard.adapter_name.clone(),
            backend_type: match context_guard.adapter_backend {
                wgpu::Backend::Vulkan => RendererBackendType::Vulkan,
                wgpu::Backend::Metal => RendererBackendType::Metal,
                wgpu::Backend::Dx12 => RendererBackendType::Dx12,
                wgpu::Backend::Gl => RendererBackendType::OpenGl,
                wgpu::Backend::BrowserWebGpu => RendererBackendType::WebGpu,
                _ => RendererBackendType::Unknown,
            },
            device_type: match context_guard.adapter_device_type {
                wgpu::DeviceType::IntegratedGpu => RendererDeviceType::IntegratedGpu,
                wgpu::DeviceType::DiscreteGpu => RendererDeviceType::DiscreteGpu,
                wgpu::DeviceType::VirtualGpu => RendererDeviceType::VirtualGpu,
                wgpu::DeviceType::Cpu => RendererDeviceType::Cpu,
                _ => RendererDeviceType::Unknown,
            },
        }
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let context_guard = self
            .context
            .lock()
            .expect("WgpuDevice: Mutex poisoned (context) on supports_feature");
        match feature_name {
            "gpu_timestamps" => context_guard
                .active_device_features
                .contains(wgpu::Features::TIMESTAMP_QUERY),
            "texture_compression_bc" => context_guard
                .active_device_features
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            "polygon_mode_line" => context_guard
                .active_device_features
                .contains(wgpu::Features::POLYGON_MODE_LINE),
            "polygon_mode_point" => context_guard
                .active_device_features
                .contains(wgpu::Features::POLYGON_MODE_POINT),
            _ => {
                log::warn!(
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {}",
                    feature_name
                );
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::renderer::api::pipeline_types::VertexFormat;

    #[test]
    fn test_convert_vertex_format_all_cases() {
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint8x2),
            wgpu::VertexFormat::Uint8x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint8x4),
            wgpu::VertexFormat::Uint8x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint8x2),
            wgpu::VertexFormat::Sint8x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint8x4),
            wgpu::VertexFormat::Sint8x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Unorm8x2),
            wgpu::VertexFormat::Unorm8x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Unorm8x4),
            wgpu::VertexFormat::Unorm8x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Snorm8x2),
            wgpu::VertexFormat::Snorm8x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Snorm8x4),
            wgpu::VertexFormat::Snorm8x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint16x2),
            wgpu::VertexFormat::Uint16x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint16x4),
            wgpu::VertexFormat::Uint16x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint16x2),
            wgpu::VertexFormat::Sint16x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint16x4),
            wgpu::VertexFormat::Sint16x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Unorm16x2),
            wgpu::VertexFormat::Unorm16x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Unorm16x4),
            wgpu::VertexFormat::Unorm16x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Snorm16x2),
            wgpu::VertexFormat::Snorm16x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Snorm16x4),
            wgpu::VertexFormat::Snorm16x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float16x2),
            wgpu::VertexFormat::Float16x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float16x4),
            wgpu::VertexFormat::Float16x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float32),
            wgpu::VertexFormat::Float32
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float32x2),
            wgpu::VertexFormat::Float32x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float32x3),
            wgpu::VertexFormat::Float32x3
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Float32x4),
            wgpu::VertexFormat::Float32x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint32),
            wgpu::VertexFormat::Uint32
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint32x2),
            wgpu::VertexFormat::Uint32x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint32x3),
            wgpu::VertexFormat::Uint32x3
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Uint32x4),
            wgpu::VertexFormat::Uint32x4
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint32),
            wgpu::VertexFormat::Sint32
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint32x2),
            wgpu::VertexFormat::Sint32x2
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint32x3),
            wgpu::VertexFormat::Sint32x3
        );
        assert_eq!(
            WgpuDevice::convert_vertex_format(VertexFormat::Sint32x4),
            wgpu::VertexFormat::Sint32x4
        );
    }

    #[test]
    fn test_convert_cull_mode() {
        assert_eq!(
            WgpuDevice::convert_cull_mode_option(Some(api_pipe::CullMode::Back)),
            Some(wgpu::Face::Back)
        );
        assert_eq!(WgpuDevice::convert_cull_mode_option(None), None);
    }
}
