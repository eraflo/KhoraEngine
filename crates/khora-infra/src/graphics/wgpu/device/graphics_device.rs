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

//! `GraphicsDevice` for the wgpu device: resource creation, writes,
//! command submission.

use khora_core::renderer::api::util::{
    GraphicsBackendType, IndexFormat, RendererDeviceType, TextureFormat,
};
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::{GraphicsDevice, PipelineError, ResourceError, ShaderError};

use crate::graphics::wgpu::command::WgpuCommandEncoder;
use crate::graphics::wgpu::conversions::{from_wgpu_texture_format, IntoWgpu};

use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use wgpu;
use wgpu::util::DeviceExt;

use super::entries::{
    WgpuBindGroupEntry, WgpuBindGroupLayoutEntry, WgpuBufferEntry, WgpuComputePipelineEntry,
    WgpuPipelineLayoutEntry, WgpuRenderPipelineEntry, WgpuSamplerEntry, WgpuShaderModuleEntry,
    WgpuTextureEntry, WgpuTextureViewEntry,
};
use super::map_async::{MapAsyncFutureState, MapAsyncOperationFuture};
use super::{buffer_descriptor, WgpuDevice};
use khora_core::math::dimension;
use khora_core::renderer::api::command::{
    self as api_cmd, BindGroupId, BindGroupLayoutId, ComputePipelineId,
};
use khora_core::renderer::api::core::{
    GraphicsAdapterInfo, ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData,
};
use khora_core::renderer::api::pipeline::enums::{CompareFunction, CullMode};
use khora_core::renderer::api::pipeline::{
    PipelineLayoutDescriptor, PipelineLayoutId, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::api::resource::texture::{self as api_tex};

impl GraphicsDevice for WgpuDevice {
    // --- Shader Module Operations ---

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
        let mut modules_guard = self.internal.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {e}"))
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
        let mut modules_guard = self.internal.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {e}"))
        })?;

        if modules_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed shader module with ID: {id:?}");
            Ok(())
        } else {
            Err(ShaderError::NotFound { id }.into())
        }
    }

    // -- Render Pipeline Operations ---

    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        log::debug!(
            "WgpuDevice: Creating render pipeline with label: {:?}",
            descriptor.label
        );

        // 1. Get the shader modules from the context
        let shader_modules_map = self.internal.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {e}"))
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
                        format: attr_desc.format.into_wgpu(),
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
                    step_mode: vb_layout_desc.step_mode.into_wgpu(),
                    attributes: attributes_for_this_layout,
                },
            )
            .collect();

        // 3. Converts primitive state
        let primitive_state = wgpu::PrimitiveState {
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
        };

        // 4. Convert depth stencil state
        let depth_stencil_state =
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
                });

        // 5. Convert color target states
        let color_target_states: Vec<Option<wgpu::ColorTargetState>> = descriptor
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
            .collect();

        // 6. Convert multisample state
        let multisample_state = wgpu::MultisampleState {
            count: descriptor.multisample_state.count.into_wgpu(),
            mask: descriptor.multisample_state.mask as u64,
            alpha_to_coverage_enabled: descriptor.multisample_state.alpha_to_coverage_enabled,
        };

        // 7. Create pipeline layout and render pipeline
        let (wgpu_render_pipeline_arc, id) = self.with_wgpu_device(|device| {
            // Look up pipeline layout if provided
            let layout_entry_opt = if let Some(layout_id) = descriptor.layout {
                // Lock order is Context -> ShaderModules -> PipelineLayouts.
                // create_render_pipeline locks shader_modules upstream, then
                // we acquire pipeline_layouts here — the strict ordering
                // prevents deadlock with other GPU paths.
                let layouts = self.internal.pipeline_layouts.lock().map_err(|e| {
                    ResourceError::BackendError(format!("Mutex poisoned (pipeline_layouts): {e}"))
                })?;
                layouts.get(&layout_id).map(|e| Arc::clone(&e.wgpu_layout))
            } else {
                None
            };

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
                    let entry_point_cow = descriptor.fragment_entry_point.as_ref().ok_or_else(|| {
                        log::error!(
                            "Logic error: Fragment shader module {:?} present but no entry point provided for pipeline {:?}.",
                            descriptor.fragment_shader_module,
                            descriptor.label
                        );
                        // We're in the `fs_wgpu_module_opt = Some(_)` branch,
                        // so by construction `descriptor.fragment_shader_module`
                        // is also `Some` — but defensively fall back to a
                        // sentinel ID rather than panic if that invariant ever
                        // breaks (R7: no `unwrap()` on GPU paths).
                        ResourceError::Pipeline(PipelineError::MissingEntryPointForFragmentShader {
                            pipeline_label: descriptor.label.as_deref().map(String::from),
                            shader_id: descriptor
                                .fragment_shader_module
                                .unwrap_or(ShaderModuleId(usize::MAX)),
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
                multiview_mask: None,
                cache: None,
            };

            // Future improvement: wrap in push_error_scope/pop_error_scope for richer diagnostics
            let pipeline = device.create_render_pipeline(&wgpu_pipeline_descriptor);
            let new_id = self.generate_pipeline_id();
            Ok((Arc::new(pipeline), new_id))
        })?;

        let mut pipelines_guard =
            self.internal.pipelines.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (pipelines): {e}"))
            })?;
        pipelines_guard.insert(
            id,
            WgpuRenderPipelineEntry {
                wgpu_pipeline: wgpu_render_pipeline_arc,
            },
        );

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

    fn create_pipeline_layout(
        &self,
        descriptor: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        let bg_layouts_guard = self.internal.bind_group_layouts.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_group_layouts): {e}"))
        })?;

        let mut wgpu_bind_group_layouts = Vec::with_capacity(descriptor.bind_group_layouts.len());
        for id in descriptor.bind_group_layouts {
            let entry = bg_layouts_guard.get(id).ok_or(ResourceError::NotFound)?;
            wgpu_bind_group_layouts.push(&entry.wgpu_layout);
        }

        let wgpu_layout = self.with_wgpu_device(|device| {
            let wgpu_desc = wgpu::PipelineLayoutDescriptor {
                label: descriptor.label.as_deref(),
                bind_group_layouts: &wgpu_bind_group_layouts
                    .iter()
                    .map(|l: &&Arc<wgpu::BindGroupLayout>| Some(l.as_ref()))
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

        self.internal
            .pipeline_layouts
            .lock()
            .map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (pipeline_layouts): {e}"))
            })?
            .insert(id, WgpuPipelineLayoutEntry { wgpu_layout });

        log::debug!(
            "WgpuDevice: Created pipeline layout '{:?}' with ID: {:?}",
            descriptor.label,
            id
        );
        Ok(id)
    }

    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        let mut pipelines_guard =
            self.internal.pipelines.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (pipelines): {e}"))
            })?;

        if pipelines_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed render pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(PipelineError::InvalidRenderPipeline { id }.into())
        }
    }

    fn create_compute_pipeline(
        &self,
        descriptor: &api_cmd::ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;
        let device = &context.device;

        // 1. Get shader module
        let shader_modules_guard = self.internal.shader_modules.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (shader_modules): {e}"))
        })?;

        let shader_entry =
            shader_modules_guard
                .get(&descriptor.shader_module)
                .ok_or(ShaderError::NotFound {
                    id: descriptor.shader_module,
                })?;

        // 2. Look up pipeline layout if provided
        let layout_entry_opt = if let Some(layout_id) = descriptor.layout {
            let layouts = self.internal.pipeline_layouts.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (pipeline_layouts): {e}"))
            })?;
            layouts.get(&layout_id).map(|e| Arc::clone(&e.wgpu_layout))
        } else {
            None
        };

        // 3. Create compute pipeline
        let wgpu_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: descriptor.label.as_deref(),
            layout: layout_entry_opt.as_deref(),
            module: &shader_entry.wgpu_module,
            entry_point: Some(descriptor.entry_point.as_ref()),
            compilation_options: Default::default(),
            cache: None,
        });

        // 3. Store and return ID
        let mut compute_pipelines_guard = self.internal.compute_pipelines.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (compute_pipelines): {e}"))
        })?;

        let id = ComputePipelineId(
            self.internal
                .next_compute_pipeline_id
                .fetch_add(1, Ordering::Relaxed),
        );

        compute_pipelines_guard.insert(
            id,
            WgpuComputePipelineEntry {
                wgpu_pipeline: Arc::new(wgpu_pipeline),
            },
        );

        Ok(id)
    }

    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        let mut compute_pipelines_guard = self.internal.compute_pipelines.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (compute_pipelines): {e}"))
        })?;

        if compute_pipelines_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed compute pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::BackendError(format!(
                "Invalid compute pipeline ID: {id:?}"
            )))
        }
    }

    // --- Buffer Operations ---

    fn create_buffer(
        &self,
        descriptor: &api_buf::BufferDescriptor,
    ) -> Result<api_buf::BufferId, ResourceError> {
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;
        let device = &context.device;

        let wgpu_buffer_descriptor = buffer_descriptor(descriptor);
        let wgpu_buffer = device.create_buffer(&wgpu_buffer_descriptor);
        let id = self.generate_buffer_id();

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(descriptor.size as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the buffer into the map
        self.internal
            .buffers
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (buffers): {e}")))?
            .insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: descriptor.size,
                },
            );

        log::debug!(
            "WgpuDevice: Created buffer '{:?}' with ID: {:?}, size: {} bytes",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id,
            descriptor.size
        );
        Ok(id)
    }

    fn create_buffer_with_data(
        &self,
        descriptor: &api_buf::BufferDescriptor,
        data: &[u8],
    ) -> Result<api_buf::BufferId, ResourceError> {
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;

        let wgpu_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: descriptor.label.as_deref(),
                contents: data,
                usage: descriptor.usage.into_wgpu(),
            });

        // Use the existing ID generation and storage system.
        let id = self.generate_buffer_id();
        let buffer_size = data.len() as u64;

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(buffer_size as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Store the buffer
        self.internal
            .buffers
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (buffers): {e}")))?
            .insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: buffer_size,
                },
            );

        log::debug!(
            "WgpuDevice: Created buffer '{:?}' with initial data. ID: {:?}, size: {} bytes",
            descriptor.label.as_deref().unwrap_or_default(),
            id,
            buffer_size
        );
        Ok(id)
    }

    fn destroy_buffer(&self, id: api_buf::BufferId) -> Result<(), ResourceError> {
        let mut buffers =
            self.internal.buffers.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (buffers): {e}"))
            })?;

        // Remove the buffer from the map and track VRAM usage
        if let Some(entry) = buffers.remove(&id) {
            self.internal
                .vram_allocated_bytes
                .fetch_sub(entry.size as usize, Ordering::Relaxed);
            log::debug!("WgpuDevice: Destroyed buffer with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn write_buffer(
        &self,
        id: api_buf::BufferId,
        offset: u64,
        data: &[u8],
    ) -> Result<(), ResourceError> {
        // 1. Get the resources
        let buffers =
            self.internal.buffers.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (buffers): {e}"))
            })?;
        let entry = buffers.get(&id).ok_or(ResourceError::NotFound)?;
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;

        // 2. Check the bounds
        let buffer_size = entry.wgpu_buffer.size();
        let end_offset = offset + data.len() as u64;
        if end_offset > buffer_size {
            return Err(ResourceError::OutOfBounds);
        }

        // 3. Write directly. No padding, no allocation. It's simple and efficient.
        context.queue.write_buffer(&entry.wgpu_buffer, offset, data);

        log::debug!(
            "WgpuDevice: Wrote {} bytes to buffer ID: {:?} at offset {}",
            data.len(),
            id,
            offset
        );

        Ok(())
    }

    fn write_buffer_async<'a>(
        &'a self,
        id: api_buf::BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        let buffers_guard = match self.internal.buffers.lock() {
            Ok(g) => g,
            Err(_) => {
                return Box::new(async {
                    Err(ResourceError::BackendError(
                        "write_buffer_async: buffers mutex poisoned".to_owned(),
                    ))
                });
            }
        };
        let entry_wgpu_buffer: Arc<wgpu::Buffer> = match buffers_guard.get(&id) {
            Some(e) => Arc::clone(&e.wgpu_buffer),
            None => return Box::new(async { Err(ResourceError::NotFound) }),
        };
        drop(buffers_guard); // Drop the lock to avoid deadlocks

        // Data needs to be owned by the future to be 'static.
        // This involves a copy, which is a common trade-off for true async operations.
        let owned_data = data.to_vec(); // One copy, owned by the future

        // Create a shared state for the Future and the WGPU callback
        let shared_state = Arc::new(MapAsyncFutureState {
            result: Mutex::new(None),
            waker: Mutex::new(None),
        });

        // Clones and moves for the `map_async` callback's `move` closure
        let future_state_for_callback = Arc::clone(&shared_state);
        let buffer_id_for_callback = id;
        let entry_wgpu_buffer_for_callback = Arc::clone(&entry_wgpu_buffer);
        let owned_data_for_callback = owned_data;

        // Schedule the WGPU map_async operation
        let buffer_slice =
            entry_wgpu_buffer.slice(offset..(offset + owned_data_for_callback.len() as u64));

        buffer_slice.map_async(wgpu::MapMode::Write, move |result| {
            // This closure runs on WGPU's internal callback thread/executor
            let final_result = if let Err(e) = result {
                log::error!(
                    "Failed to map buffer asynchronously for ID {buffer_id_for_callback:?}: {e:?}"
                );
                Err(ResourceError::BackendError(format!(
                    "WGPU map_async failed: {e:?}"
                )))
            } else {
                // Mapping was successful. Now perform the actual data copy.
                // Re-slice the buffer from the entry_wgpu_buffer_for_callback as the original buffer_slice
                // would have been dropped or moved.
                let buffer_slice_for_copy = entry_wgpu_buffer_for_callback
                    .slice(offset..(offset + owned_data_for_callback.len() as u64));
                let mut mapped_range = buffer_slice_for_copy.get_mapped_range_mut();
                mapped_range.copy_from_slice(&owned_data_for_callback); // This is the actual data write
                drop(mapped_range); // Explicitly unmap the buffer by dropping the guard
                entry_wgpu_buffer_for_callback.unmap(); // Ensure WGPU knows it's unmapped
                log::debug!(
                    "WgpuDevice: Async map_async copy complete for buffer ID: {buffer_id_for_callback:?}"
                );
                Ok(())
            };

            // Signal the Future that the operation is complete. If the
            // mutex is poisoned the future is unreachable anyway — log and
            // give up rather than panic on the callback thread.
            match future_state_for_callback.result.lock() {
                Ok(mut g) => *g = Some(final_result),
                Err(_) => {
                    log::error!(
                        "write_buffer_async callback for {buffer_id_for_callback:?}: \
                         result mutex poisoned"
                    );
                    return;
                }
            }

            // Wake the Future if a waker was stored.
            match future_state_for_callback.waker.lock() {
                Ok(mut g) => {
                    if let Some(waker) = g.take() {
                        waker.wake();
                    }
                }
                Err(_) => {
                    log::error!(
                        "write_buffer_async callback for {buffer_id_for_callback:?}: \
                         waker mutex poisoned"
                    );
                }
            }
        });

        // Return a Future that will wait for the callback to complete
        Box::new(MapAsyncOperationFuture {
            state: shared_state,
        })
    }

    fn create_texture(
        &self,
        descriptor: &api_tex::TextureDescriptor,
    ) -> Result<api_tex::TextureId, ResourceError> {
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;
        let device = &context.device;

        let wgpu_texture_descriptor = wgpu::TextureDescriptor {
            label: descriptor.label.as_deref(),
            size: descriptor.size.into_wgpu(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count.into_wgpu(),
            dimension: descriptor.dimension.into_wgpu(),
            format: descriptor.format.into_wgpu(),
            usage: descriptor.usage.into_wgpu(),
            view_formats: &descriptor
                .view_formats
                .iter()
                .map(|&f: &TextureFormat| f.into_wgpu())
                .collect::<Vec<_>>(),
        };

        // Create the texture using the wgpu device with the specified descriptor
        let wgpu_texture = device.create_texture(&wgpu_texture_descriptor);
        let id = self.generate_texture_id();
        let size_in_bytes = Self::calculate_texture_size_in_bytes(descriptor);

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(size_in_bytes as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the texture into the map
        self.internal
            .textures
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (textures): {e}")))?
            .insert(
                id,
                WgpuTextureEntry {
                    wgpu_texture: Arc::new(wgpu_texture),
                    size: size_in_bytes,
                },
            );

        log::debug!(
            "WgpuDevice: Created texture '{:?}' with ID: {:?}, size: {} bytes (VRAM)",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id,
            size_in_bytes
        );
        Ok(id)
    }

    fn destroy_texture(&self, id: api_tex::TextureId) -> Result<(), ResourceError> {
        let mut textures =
            self.internal.textures.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (textures): {e}"))
            })?;

        // Remove the texture from the map and track VRAM usage
        if let Some(entry) = textures.remove(&id) {
            self.internal
                .vram_allocated_bytes
                .fetch_sub(entry.size as usize, Ordering::Relaxed);
            log::debug!("WgpuDevice: Destroyed texture with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn write_texture(
        &self,
        texture_id: api_tex::TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: dimension::Origin3D,
        size: dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        let textures =
            self.internal.textures.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (textures): {e}"))
            })?;
        let entry = textures.get(&texture_id).ok_or(ResourceError::NotFound)?;
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;

        context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &entry.wgpu_texture,
                mip_level: 0, // Assuming base mip for now
                origin: offset.into_wgpu(),
                aspect: wgpu::TextureAspect::All, // Assuming all aspects for now
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row,
                rows_per_image: None, // Assuming 2D or 3D without specific rows per image info
            },
            size.into_wgpu(),
        );
        log::debug!(
            "WgpuDevice: Wrote {} bytes to texture ID: {:?} at offset {:?}",
            data.len(),
            texture_id,
            offset
        );
        Ok(())
    }

    fn create_texture_view(
        &self,
        texture_id: api_tex::TextureId,
        descriptor: &api_tex::TextureViewDescriptor,
    ) -> Result<api_tex::TextureViewId, ResourceError> {
        let textures =
            self.internal.textures.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (textures): {e}"))
            })?;
        let texture_entry = textures.get(&texture_id).ok_or(ResourceError::NotFound)?;

        let wgpu_view_descriptor = wgpu::TextureViewDescriptor {
            label: descriptor.label.as_deref(),
            format: descriptor.format.map(|f: TextureFormat| f.into_wgpu()),
            dimension: descriptor
                .dimension
                .map(|d: api_tex::TextureViewDimension| d.into_wgpu()),
            aspect: descriptor.aspect.into_wgpu(),
            base_mip_level: descriptor.base_mip_level,
            mip_level_count: descriptor.mip_level_count,
            base_array_layer: descriptor.base_array_layer,
            array_layer_count: descriptor.array_layer_count,
            usage: None,
        };

        // Create the texture view using the wgpu texture
        let wgpu_view = Arc::new(
            texture_entry
                .wgpu_texture
                .create_view(&wgpu_view_descriptor),
        );
        let id = self.generate_texture_view_id();
        self.internal
            .texture_views
            .lock()
            .map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (texture_views): {e}"))
            })?
            .insert(
                id,
                WgpuTextureViewEntry {
                    wgpu_view,
                    source_texture_id: Some(texture_id),
                },
            );
        log::info!(
            "WgpuDevice: Created texture view '{:?}' for texture ID: {:?} with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            texture_id,
            id
        );
        Ok(id)
    }

    fn destroy_texture_view(&self, id: api_tex::TextureViewId) -> Result<(), ResourceError> {
        let mut texture_views = self.internal.texture_views.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (texture_views): {e}"))
        })?;

        // Remove the texture view from the map
        if texture_views.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed texture view with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn create_sampler(
        &self,
        descriptor: &api_tex::SamplerDescriptor,
    ) -> Result<api_tex::SamplerId, ResourceError> {
        let context =
            self.internal.context.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (context): {e}"))
            })?;
        let device = &context.device;

        let wgpu_sampler_descriptor = wgpu::SamplerDescriptor {
            label: descriptor.label.as_deref(),
            address_mode_u: descriptor.address_mode_u.into_wgpu(),
            address_mode_v: descriptor.address_mode_v.into_wgpu(),
            address_mode_w: descriptor.address_mode_w.into_wgpu(),
            mag_filter: descriptor.mag_filter.into_wgpu(),
            min_filter: descriptor.min_filter.into_wgpu(),
            mipmap_filter: descriptor.mipmap_filter.into_wgpu(),
            lod_min_clamp: descriptor.lod_min_clamp,
            lod_max_clamp: descriptor.lod_max_clamp,
            compare: descriptor.compare.map(|f: CompareFunction| f.into_wgpu()),
            anisotropy_clamp: descriptor.anisotropy_clamp,
            border_color: descriptor
                .border_color
                .and_then(|c: api_tex::SamplerBorderColor| c.into_wgpu()),
        };

        // Create the sampler using the wgpu device
        let wgpu_sampler = Arc::new(device.create_sampler(&wgpu_sampler_descriptor));
        let id = self.generate_sampler_id();
        self.internal
            .samplers
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (samplers): {e}")))?
            .insert(id, WgpuSamplerEntry { wgpu_sampler });
        log::debug!(
            "WgpuDevice: Created sampler '{:?}' with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_sampler(&self, id: api_tex::SamplerId) -> Result<(), ResourceError> {
        let mut samplers =
            self.internal.samplers.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (samplers): {e}"))
            })?;

        // Remove the sampler from the map
        if samplers.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed sampler with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn get_surface_format(&self) -> Option<TextureFormat> {
        let context = match self.internal.context.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!("WgpuDevice::get_surface_format: context mutex poisoned");
                return None;
            }
        };
        from_wgpu_texture_format(context.surface_config.format)
    }

    fn get_surface_size(&self) -> (u32, u32) {
        match self.internal.context.lock() {
            Ok(context) => (context.surface_config.width, context.surface_config.height),
            Err(_) => {
                log::error!("WgpuDevice::get_surface_size: context mutex poisoned");
                (0, 0)
            }
        }
    }

    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        let context_guard = match self.internal.context.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::get_adapter_info: context mutex poisoned, returning defaults"
                );
                return GraphicsAdapterInfo {
                    name: "<poisoned context>".to_owned(),
                    backend_type: GraphicsBackendType::Unknown,
                    device_type: RendererDeviceType::Unknown,
                };
            }
        };
        GraphicsAdapterInfo {
            name: context_guard.adapter_name.clone(),
            backend_type: match context_guard.adapter_backend {
                wgpu::Backend::Vulkan => GraphicsBackendType::Vulkan,
                wgpu::Backend::Metal => GraphicsBackendType::Metal,
                wgpu::Backend::Dx12 => GraphicsBackendType::Dx12,
                wgpu::Backend::Gl => GraphicsBackendType::OpenGL,
                wgpu::Backend::BrowserWebGpu => GraphicsBackendType::WebGpu,
                wgpu::Backend::Noop => GraphicsBackendType::Unknown,
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
        let context_guard = match self.internal.context.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::supports_feature: context mutex poisoned, reporting unsupported"
                );
                return false;
            }
        };
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
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {feature_name}"
                );
                false
            }
        }
    }

    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        let context_guard = match self.internal.context.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::create_command_encoder: context mutex poisoned — \
                     returning a dead encoder; subsequent `finish()` will return None"
                );
                // `WgpuCommandEncoder` already represents a dead state via
                // `encoder: None` (its `finish()` fails fast with `None`),
                // so the rest of the frame skips this submission cleanly.
                return Box::new(WgpuCommandEncoder {
                    encoder: None,
                    device: self.clone(),
                });
            }
        };
        let descriptor = wgpu::CommandEncoderDescriptor { label };
        let encoder = context_guard.device.create_command_encoder(&descriptor);

        Box::new(WgpuCommandEncoder {
            encoder: Some(encoder), // Wrap in Option to be `take`n in finish()
            device: self.clone(),   // Clone the Arc handle
        })
    }

    fn submit_command_buffer(&self, command_buffer_id: api_cmd::CommandBufferId) {
        let mut guard = match self.internal.pending_command_buffers.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::submit_command_buffer: pending_command_buffers mutex poisoned, \
                     dropping {:?}",
                    command_buffer_id
                );
                return;
            }
        };

        // Remove the command buffer from the map. If it doesn't exist, it's a logic error.
        if let Some(buffer) = guard.remove(&command_buffer_id) {
            let context_guard = match self.internal.context.lock() {
                Ok(g) => g,
                Err(_) => {
                    log::error!(
                        "WgpuDevice::submit_command_buffer: context mutex poisoned, \
                         dropping {:?}",
                        command_buffer_id
                    );
                    return;
                }
            };
            let idx = context_guard.queue.submit(std::iter::once(buffer));
            // Store the submission index so we can wait on it before the next acquire.
            match self.internal.last_submission_index.lock() {
                Ok(mut g) => *g = Some(idx),
                Err(_) => log::error!(
                    "WgpuDevice::submit_command_buffer: last_submission_index mutex poisoned"
                ),
            }
        } else {
            log::error!(
                "Attempted to submit a CommandBufferId ({:?}) that does not exist.",
                command_buffer_id
            );
        }
    }

    // --- Bind Group Operations ---

    fn create_bind_group_layout(
        &self,
        descriptor: &api_cmd::BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        use api_cmd::{BindingType, BufferBindingType};

        let wgpu_entries: Vec<wgpu::BindGroupLayoutEntry> = descriptor
            .entries
            .iter()
            .map(|entry| {
                let ty = match &entry.ty {
                    BindingType::Buffer {
                        ty,
                        has_dynamic_offset,
                        min_binding_size,
                    } => {
                        let buffer_ty = match ty {
                            BufferBindingType::Uniform => wgpu::BufferBindingType::Uniform,
                            BufferBindingType::Storage { read_only } => {
                                wgpu::BufferBindingType::Storage {
                                    read_only: *read_only,
                                }
                            }
                        };
                        wgpu::BindingType::Buffer {
                            ty: buffer_ty,
                            has_dynamic_offset: *has_dynamic_offset,
                            min_binding_size: *min_binding_size,
                        }
                    }
                    BindingType::Texture {
                        sample_type,
                        view_dimension,
                        multisampled,
                    } => wgpu::BindingType::Texture {
                        sample_type: (*sample_type).into_wgpu(),
                        view_dimension: (*view_dimension).into_wgpu(),
                        multisampled: *multisampled,
                    },
                    BindingType::Sampler(sampler_ty) => {
                        wgpu::BindingType::Sampler((*sampler_ty).into_wgpu())
                    }
                };

                wgpu::BindGroupLayoutEntry {
                    binding: entry.binding,
                    visibility: entry.visibility.into_wgpu(),
                    ty,
                    count: None,
                }
            })
            .collect();

        let wgpu_layout = self.with_wgpu_device(|device| {
            let wgpu_descriptor = wgpu::BindGroupLayoutDescriptor {
                label: descriptor.label,
                entries: &wgpu_entries,
            };
            Ok(Arc::new(device.create_bind_group_layout(&wgpu_descriptor)))
        })?;

        let id = self.generate_bind_group_layout_id();
        let mut layouts = self.internal.bind_group_layouts.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_group_layouts): {e}"))
        })?;
        layouts.insert(id, WgpuBindGroupLayoutEntry { wgpu_layout });

        log::debug!(
            "WgpuDevice: Created bind group layout '{:?}' with ID: {:?}",
            descriptor.label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn create_bind_group(
        &self,
        descriptor: &api_cmd::BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError> {
        use api_cmd::BindingResource;

        // Get the bind group layout
        let layouts = self.internal.bind_group_layouts.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_group_layouts): {e}"))
        })?;
        let layout_entry = layouts
            .get(&descriptor.layout)
            .ok_or(ResourceError::NotFound)?;
        let wgpu_layout = Arc::clone(&layout_entry.wgpu_layout);
        drop(layouts);

        // Resource storage to hold Arcs alive
        enum ResourceArc {
            Buffer(u32, Arc<wgpu::Buffer>, u64, Option<std::num::NonZeroU64>),
            TextureView(u32, Arc<wgpu::TextureView>),
            Sampler(u32, Arc<wgpu::Sampler>),
        }

        let mut resource_arcs = Vec::with_capacity(descriptor.entries.len());

        {
            let buffers_lock = self.internal.buffers.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (buffers): {e}"))
            })?;
            let texture_views_lock = self.internal.texture_views.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (texture_views): {e}"))
            })?;
            let samplers_lock = self.internal.samplers.lock().map_err(|e| {
                ResourceError::BackendError(format!("Mutex poisoned (samplers): {e}"))
            })?;

            for entry in descriptor.entries {
                match &entry.resource {
                    BindingResource::Buffer(buffer_binding) => {
                        let buffer_entry = buffers_lock
                            .get(&buffer_binding.buffer)
                            .ok_or(ResourceError::NotFound)?;
                        resource_arcs.push(ResourceArc::Buffer(
                            entry.binding,
                            Arc::clone(&buffer_entry.wgpu_buffer),
                            buffer_binding.offset,
                            buffer_binding.size,
                        ));
                    }
                    BindingResource::TextureView(view_id) => {
                        let view_entry = texture_views_lock
                            .get(view_id)
                            .ok_or(ResourceError::NotFound)?;
                        resource_arcs.push(ResourceArc::TextureView(
                            entry.binding,
                            Arc::clone(&view_entry.wgpu_view),
                        ));
                    }
                    BindingResource::Sampler(sampler_id) => {
                        let sampler_entry = samplers_lock
                            .get(sampler_id)
                            .ok_or(ResourceError::NotFound)?;
                        resource_arcs.push(ResourceArc::Sampler(
                            entry.binding,
                            Arc::clone(&sampler_entry.wgpu_sampler),
                        ));
                    }
                }
            }
        }

        // Now create wgpu entries with references to the Arc'd resources
        let wgpu_entries: Vec<wgpu::BindGroupEntry> = resource_arcs
            .iter()
            .map(|resource| match resource {
                ResourceArc::Buffer(binding, buffer, offset, size) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: buffer.as_ref(),
                        offset: *offset,
                        size: *size,
                    }),
                },
                ResourceArc::TextureView(binding, view) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::TextureView(view.as_ref()),
                },
                ResourceArc::Sampler(binding, sampler) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::Sampler(sampler.as_ref()),
                },
            })
            .collect();

        let wgpu_bind_group = self.with_wgpu_device(|device| {
            let wgpu_descriptor = wgpu::BindGroupDescriptor {
                label: descriptor.label,
                layout: &wgpu_layout,
                entries: &wgpu_entries,
            };
            Ok(Arc::new(device.create_bind_group(&wgpu_descriptor)))
        })?;

        let id = self.generate_bind_group_id();
        let mut bind_groups = self.internal.bind_groups.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_groups): {e}"))
        })?;
        bind_groups.insert(id, WgpuBindGroupEntry { wgpu_bind_group });

        log::debug!(
            "WgpuDevice: Created bind group '{:?}' with ID: {:?}",
            descriptor.label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        let mut layouts = self.internal.bind_group_layouts.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_group_layouts): {e}"))
        })?;

        if layouts.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed bind group layout with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        let mut bind_groups = self.internal.bind_groups.lock().map_err(|e| {
            ResourceError::BackendError(format!("Mutex poisoned (bind_groups): {e}"))
        })?;

        if bind_groups.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed bind group with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }
}
