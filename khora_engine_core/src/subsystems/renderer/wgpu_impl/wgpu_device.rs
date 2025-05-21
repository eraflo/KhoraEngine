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

use super::conversions::{self, *};

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
                        format: attr_desc.format.into(),
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
            topology: descriptor.primitive_state.topology.into(),
            strip_index_format: descriptor.primitive_state.strip_index_format.map(|f| f.into()),
            front_face: descriptor.primitive_state.front_face.into(),
            cull_mode: descriptor.primitive_state.cull_mode.map(|m| m.into()),
            polygon_mode: descriptor.primitive_state.polygon_mode.into(),
            unclipped_depth: descriptor.primitive_state.unclipped_depth,
            conservative: descriptor.primitive_state.conservative,
        };

        // 4. Convert depth stencil state
        let depth_stencil_state = descriptor.depth_stencil_state.as_ref().map(|ds| wgpu::DepthStencilState {
            format: ds.format.into(),
            depth_write_enabled: ds.depth_write_enabled,
            depth_compare: ds.depth_compare.into(),
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: ds.stencil_front.compare.into(),
                    fail_op: ds.stencil_front.fail_op.into(),
                    depth_fail_op: ds.stencil_front.depth_fail_op.into(),
                    pass_op: ds.stencil_front.depth_pass_op.into(),
                },
                back: wgpu::StencilFaceState {
                    compare: ds.stencil_back.compare.into(),
                    fail_op: ds.stencil_back.fail_op.into(),
                    depth_fail_op: ds.stencil_back.depth_fail_op.into(),
                    pass_op: ds.stencil_back.depth_pass_op.into(),
                },
                read_mask: ds.stencil_read_mask,
                write_mask: ds.stencil_write_mask,
            },
            bias: wgpu::DepthBiasState {
                constant: ds.bias.constant,
                slope_scale: ds.bias.slope_scale,
                clamp: ds.bias.clamp,
            },
        });

        // 5. Convert color target states
        let color_target_states: Vec<Option<wgpu::ColorTargetState>> = descriptor.color_target_states.iter().map(|cts| {
            Some(wgpu::ColorTargetState {
                format: cts.format.into(),
                blend: cts.blend.map(|b| wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: b.color.src_factor.into(),
                        dst_factor: b.color.dst_factor.into(),
                        operation: b.color.operation.into(),
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: b.alpha.src_factor.into(),
                        dst_factor: b.alpha.dst_factor.into(),
                        operation: b.alpha.operation.into(),
                    },
                }),
                write_mask: wgpu::ColorWrites::from_bits_truncate(cts.write_mask.bits() as u32), // Bitflags conversion
            })
        }).collect();

        // 6. Convert multisample state
        let multisample_state = wgpu::MultisampleState {
            count: descriptor.multisample_state.count.into(),
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
                        targets: &color_target_states,
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