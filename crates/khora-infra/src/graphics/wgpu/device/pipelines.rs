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

//! The wgpu device's shader modules and pipelines: creating them from the
//! engine's descriptors, registering them under an id, removing them.

use khora_core::renderer::api::pipeline::enums::CullMode;
use khora_core::renderer::api::pipeline::{
    ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
    RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::IndexFormat;
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData};
use khora_core::renderer::{PipelineError, ResourceError, ShaderError};

use crate::graphics::wgpu::conversions::IntoWgpu;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use wgpu;

use super::entries::{
    WgpuComputePipelineEntry, WgpuPipelineLayoutEntry, WgpuRenderPipelineEntry,
    WgpuShaderModuleEntry,
};
use super::WgpuDevice;

impl WgpuDevice {
    // --- Shader Module Operations ---

    /// Compiles a WGSL shader module and registers it under a new id.
    pub(super) fn compile_shader_module(
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
                "WgpuDevice: Creating wgpu::ShaderModule with label: {:?}",
                label
            );
            let wgpu_descriptor = wgpu::ShaderModuleDescriptor {
                label,
                source: wgpu_source,
            };
            Ok(Arc::new(device.create_shader_module(wgpu_descriptor)))
        })?;

        // Create a new shader module entry and insert it into the shader_modules map
        let id = self.generate_shader_id();
        self.with_registry(|r| {
            r.shader_modules.insert(
                id,
                WgpuShaderModuleEntry {
                    wgpu_module: wgpu_module_arc,
                },
            )
        })?;

        log::info!(
            "WgpuDevice: Successfully created shader module '{:?}' with ID: {:?}",
            label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    /// Removes a shader module from the registry.
    pub(super) fn remove_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        if self
            .with_registry(|r| r.shader_modules.remove(&id))?
            .is_some()
        {
            log::debug!("WgpuDevice: Destroyed shader module with ID: {id:?}");
            Ok(())
        } else {
            Err(ShaderError::NotFound { id }.into())
        }
    }

    // -- Render Pipeline Operations ---

    /// Builds a render pipeline from its shader modules, its layout and its
    /// fixed-function states, and registers it under a new id.
    pub(super) fn build_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        log::debug!(
            "WgpuDevice: Creating render pipeline with label: {:?}",
            descriptor.label
        );

        // The shader modules and the layout, cloned out of the registry.
        let missing_module = |id| {
            ResourceError::Pipeline(PipelineError::InvalidShaderModuleForPipeline {
                id,
                pipeline_label: descriptor.label.as_deref().map(String::from),
            })
        };
        let (vs_module, fs_module, layout_arc) = self.with_registry(|r| {
            let module = |id| {
                r.shader_modules
                    .get(&id)
                    .map(|e| Arc::clone(&e.wgpu_module))
            };
            (
                module(descriptor.vertex_shader_module),
                descriptor.fragment_shader_module.map(|id| (id, module(id))),
                descriptor.layout.and_then(|id| {
                    r.pipeline_layouts
                        .get(&id)
                        .map(|e| Arc::clone(&e.wgpu_layout))
                }),
            )
        })?;
        let vs_module = vs_module.ok_or_else(|| missing_module(descriptor.vertex_shader_module))?;
        let vs_wgpu_module: &Arc<wgpu::ShaderModule> = &vs_module;
        let fs_module = match fs_module {
            Some((id, module)) => Some(module.ok_or_else(|| missing_module(id))?),
            None => None,
        };
        let fs_wgpu_module_opt = fs_module.as_ref();

        // Convert vertex buffers layout
        let wgpu_vertex_attributes_storage = vertex_attributes(descriptor);
        let wgpu_vertex_buffers_layouts =
            vertex_buffer_layouts(descriptor, &wgpu_vertex_attributes_storage);

        // 3.–6. The fixed-function states.
        let primitive_state = primitive_state(descriptor);
        let depth_stencil_state = depth_stencil_state(descriptor);
        let color_target_states = color_target_states(descriptor);
        let multisample_state = wgpu::MultisampleState {
            count: descriptor.multisample_state.count.into_wgpu(),
            mask: descriptor.multisample_state.mask as u64,
            alpha_to_coverage_enabled: descriptor.multisample_state.alpha_to_coverage_enabled,
        };

        // Create pipeline layout and render pipeline
        let (wgpu_render_pipeline_arc, id) = self.with_wgpu_device(|device| {
            let layout_entry_opt = layout_arc.clone();

            let wgpu_pipeline_descriptor = wgpu::RenderPipelineDescriptor {
                label: descriptor.label.as_deref(),
                layout: layout_entry_opt.as_deref(),
                vertex: wgpu::VertexState {
                    module: vs_wgpu_module,
                    entry_point: Some(descriptor.vertex_entry_point.as_ref()),
                    buffers: &wgpu_vertex_buffers_layouts,
                    compilation_options: Default::default(),
                },
                fragment: if let Some(fs_module) = fs_wgpu_module_opt {
                    let entry_point = fragment_entry_point(descriptor)?;
                    Some(wgpu::FragmentState {
                        module: fs_module,
                        entry_point: Some(entry_point),
                        targets: &color_target_states,
                        compilation_options: Default::default(),
                    })
                } else {
                    None
                },
                primitive: primitive_state,
                depth_stencil: depth_stencil_state,
                multisample: multisample_state,
                multiview_mask: None,
                cache: None,
            };

            // Future improvement: wrap in push_error_scope/pop_error_scope for richer diagnostics
            let pipeline = device.create_render_pipeline(&wgpu_pipeline_descriptor);
            let new_id = self.generate_pipeline_id();
            Ok((Arc::new(pipeline), new_id))
        })?;

        self.with_registry(|r| {
            r.pipelines.insert(
                id,
                WgpuRenderPipelineEntry {
                    wgpu_pipeline: wgpu_render_pipeline_arc,
                },
            )
        })?;

        log::info!(
            "WgpuDevice: Successfully created render pipeline '{:?}' with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id
        );
        Ok(id)
    }

    /// Builds a pipeline layout from registered bind group layouts and
    /// registers it under a new id.
    pub(super) fn build_pipeline_layout(
        &self,
        descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        let wgpu_bind_group_layouts: Vec<Arc<wgpu::BindGroupLayout>> = self
            .with_registry(|r| {
                descriptor
                    .bind_group_layouts
                    .iter()
                    .map(|id| {
                        r.bind_group_layouts
                            .get(id)
                            .map(|e| Arc::clone(&e.wgpu_layout))
                    })
                    .collect::<Option<Vec<_>>>()
            })?
            .ok_or(ResourceError::NotFound)?;

        let wgpu_layout = self.with_wgpu_device(|device| {
            let wgpu_desc = wgpu::PipelineLayoutDescriptor {
                label: descriptor.label.as_deref(),
                bind_group_layouts: &wgpu_bind_group_layouts
                    .iter()
                    .map(|l: &Arc<wgpu::BindGroupLayout>| Some(l.as_ref()))
                    .collect::<Vec<_>>(),
                immediate_size: 0,
            };
            Ok(Arc::new(device.create_pipeline_layout(&wgpu_desc)))
        })?;

        let id = PipelineLayoutId(
            self.internal
                .next_pipeline_layout_id
                .fetch_add(1, Ordering::Relaxed),
        );

        self.with_registry(|r| {
            r.pipeline_layouts
                .insert(id, WgpuPipelineLayoutEntry { wgpu_layout })
        })?;

        log::debug!(
            "WgpuDevice: Created pipeline layout '{:?}' with ID: {:?}",
            descriptor.label,
            id
        );
        Ok(id)
    }

    /// Removes a render pipeline from the registry.
    pub(super) fn remove_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        if self.with_registry(|r| r.pipelines.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed render pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(PipelineError::InvalidRenderPipeline { id }.into())
        }
    }

    /// Builds a compute pipeline from its shader module and layout and
    /// registers it under a new id.
    pub(super) fn build_compute_pipeline(
        &self,
        descriptor: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        // The shader module and the layout, cloned out of the registry.
        let (shader_module, layout_entry_opt) = self.with_registry(|r| {
            (
                r.shader_modules
                    .get(&descriptor.shader_module)
                    .map(|e| Arc::clone(&e.wgpu_module)),
                descriptor.layout.and_then(|id| {
                    r.pipeline_layouts
                        .get(&id)
                        .map(|e| Arc::clone(&e.wgpu_layout))
                }),
            )
        })?;
        let shader_module = shader_module.ok_or(ShaderError::NotFound {
            id: descriptor.shader_module,
        })?;

        // Create compute pipeline
        let wgpu_pipeline =
            self.wgpu_device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: descriptor.label.as_deref(),
                    layout: layout_entry_opt.as_deref(),
                    module: &shader_module,
                    entry_point: Some(descriptor.entry_point.as_ref()),
                    compilation_options: Default::default(),
                    cache: None,
                });

        // Store and return ID
        let id = ComputePipelineId(
            self.internal
                .next_compute_pipeline_id
                .fetch_add(1, Ordering::Relaxed),
        );
        self.with_registry(|r| {
            r.compute_pipelines.insert(
                id,
                WgpuComputePipelineEntry {
                    wgpu_pipeline: Arc::new(wgpu_pipeline),
                },
            )
        })?;

        Ok(id)
    }

    /// Removes a compute pipeline from the registry.
    pub(super) fn remove_compute_pipeline(
        &self,
        id: ComputePipelineId,
    ) -> Result<(), ResourceError> {
        if self
            .with_registry(|r| r.compute_pipelines.remove(&id))?
            .is_some()
        {
            log::debug!("WgpuDevice: Destroyed compute pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::BackendError(format!(
                "Invalid compute pipeline ID: {id:?}"
            )))
        }
    }
}

/// The wgpu attributes of each vertex buffer the pipeline reads, one list
/// per buffer: the storage the buffer layouts borrow.
fn vertex_attributes(descriptor: &RenderPipelineDescriptor) -> Vec<Vec<wgpu::VertexAttribute>> {
    descriptor
        .vertex_buffers_layout
        .as_ref()
        .iter()
        .map(|vb_layout_desc| {
            vb_layout_desc
                .attributes
                .as_ref()
                .iter()
                .map(|attr_desc| wgpu::VertexAttribute {
                    format: attr_desc.format.into_wgpu(),
                    offset: attr_desc.offset,
                    shader_location: attr_desc.shader_location,
                })
                .collect()
        })
        .collect()
}

/// The wgpu layout of each vertex buffer, borrowing its attributes from
/// `attributes` (built by [`vertex_attributes`], in the same order).
fn vertex_buffer_layouts<'a>(
    descriptor: &RenderPipelineDescriptor,
    attributes: &'a [Vec<wgpu::VertexAttribute>],
) -> Vec<wgpu::VertexBufferLayout<'a>> {
    descriptor
        .vertex_buffers_layout
        .as_ref()
        .iter()
        .zip(attributes.iter())
        .map(
            |(vb_layout_desc, attributes_for_this_layout)| wgpu::VertexBufferLayout {
                array_stride: vb_layout_desc.array_stride,
                step_mode: vb_layout_desc.step_mode.into_wgpu(),
                attributes: attributes_for_this_layout,
            },
        )
        .collect()
}

/// The pipeline's primitive state, converted to wgpu.
fn primitive_state(descriptor: &RenderPipelineDescriptor) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: descriptor.primitive_state.topology.into_wgpu(),
        strip_index_format: descriptor
            .primitive_state
            .strip_index_format
            .map(|f: IndexFormat| f.into_wgpu()),
        front_face: descriptor.primitive_state.front_face.into_wgpu(),
        cull_mode: descriptor
            .primitive_state
            .cull_mode
            .and_then(|m: CullMode| m.into_wgpu()),
        polygon_mode: descriptor.primitive_state.polygon_mode.into_wgpu(),
        unclipped_depth: descriptor.primitive_state.unclipped_depth,
        conservative: descriptor.primitive_state.conservative,
    }
}

/// The pipeline's depth-stencil state, converted to wgpu; `None` when it
/// has none.
fn depth_stencil_state(descriptor: &RenderPipelineDescriptor) -> Option<wgpu::DepthStencilState> {
    descriptor
        .depth_stencil_state
        .as_ref()
        .map(|ds| wgpu::DepthStencilState {
            format: ds.format.into_wgpu(),
            depth_write_enabled: Some(ds.depth_write_enabled),
            depth_compare: Some(ds.depth_compare.into_wgpu()),
            stencil: wgpu::StencilState {
                front: wgpu::StencilFaceState {
                    compare: ds.stencil_front.compare.into_wgpu(),
                    fail_op: ds.stencil_front.fail_op.into_wgpu(),
                    depth_fail_op: ds.stencil_front.depth_fail_op.into_wgpu(),
                    pass_op: ds.stencil_front.depth_pass_op.into_wgpu(),
                },
                back: wgpu::StencilFaceState {
                    compare: ds.stencil_back.compare.into_wgpu(),
                    fail_op: ds.stencil_back.fail_op.into_wgpu(),
                    depth_fail_op: ds.stencil_back.depth_fail_op.into_wgpu(),
                    pass_op: ds.stencil_back.depth_pass_op.into_wgpu(),
                },
                read_mask: ds.stencil_read_mask,
                write_mask: ds.stencil_write_mask,
            },
            bias: wgpu::DepthBiasState {
                constant: ds.bias.constant,
                slope_scale: ds.bias.slope_scale,
                clamp: ds.bias.clamp,
            },
        })
}

/// The pipeline's color targets, converted to wgpu.
fn color_target_states(
    descriptor: &RenderPipelineDescriptor,
) -> Vec<Option<wgpu::ColorTargetState>> {
    descriptor
        .color_target_states
        .iter()
        .map(|cts| {
            Some(wgpu::ColorTargetState {
                format: cts.format.into_wgpu(),
                blend: cts.blend.map(|b| wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: b.color.src_factor.into_wgpu(),
                        dst_factor: b.color.dst_factor.into_wgpu(),
                        operation: b.color.operation.into_wgpu(),
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: b.alpha.src_factor.into_wgpu(),
                        dst_factor: b.alpha.dst_factor.into_wgpu(),
                        operation: b.alpha.operation.into_wgpu(),
                    },
                }),
                write_mask: wgpu::ColorWrites::from_bits_truncate(cts.write_mask.bits() as u32), // Bitflags conversion
            })
        })
        .collect()
}

/// The fragment shader's entry point, for a pipeline that has a fragment
/// shader module.
fn fragment_entry_point<'d>(
    descriptor: &'d RenderPipelineDescriptor,
) -> Result<&'d str, ResourceError> {
    descriptor.fragment_entry_point.as_deref().ok_or_else(|| {
        log::error!(
            "Logic error: Fragment shader module {:?} present but no entry point provided for pipeline {:?}.",
            descriptor.fragment_shader_module,
            descriptor.label
        );
        // Called only when the fragment module is present, so by
        // construction `descriptor.fragment_shader_module` is also `Some` —
        // but defensively fall back to a sentinel ID rather than panic if
        // that invariant ever breaks (no `unwrap()` on GPU paths).
        ResourceError::Pipeline(PipelineError::MissingEntryPointForFragmentShader {
            pipeline_label: descriptor.label.as_deref().map(String::from),
            shader_id: descriptor
                .fragment_shader_module
                .unwrap_or(ShaderModuleId(usize::MAX)),
        })
    })
}
