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

use khora_core::renderer::api::device::{GraphicsBackendType, RendererDeviceType};
use khora_core::renderer::api::resource::{IndexFormat, TextureFormat};
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::{GraphicsDevice, PipelineError, ResourceError, ShaderError};

use crate::graphics::wgpu::command::WgpuCommandEncoder;
use crate::graphics::wgpu::conversions::IntoWgpu;

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
use khora_core::renderer::api::command as api_cmd;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::enums::{CompareFunction, CullMode};
use khora_core::renderer::api::pipeline::ComputePipelineId;
use khora_core::renderer::api::pipeline::{
    PipelineLayoutDescriptor, PipelineLayoutId, RenderPipelineDescriptor, RenderPipelineId,
};
use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::api::resource::texture::{self as api_tex};
use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData};

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

    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
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

    fn create_render_pipeline(
        &self,
        descriptor: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        log::debug!(
            "WgpuDevice: Creating render pipeline with label: {:?}",
            descriptor.label
        );

        // 1. The shader modules and the layout, cloned out of the registry.
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

    fn create_pipeline_layout(
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

    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        if self.with_registry(|r| r.pipelines.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed render pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(PipelineError::InvalidRenderPipeline { id }.into())
        }
    }

    fn create_compute_pipeline(
        &self,
        descriptor: &khora_core::renderer::api::pipeline::ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        // 1. The shader module and the layout, cloned out of the registry.
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

        // 2. Create compute pipeline
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

        // 3. Store and return ID
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

    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
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

    // --- Buffer Operations ---

    fn create_buffer(
        &self,
        descriptor: &api_buf::BufferDescriptor,
    ) -> Result<api_buf::BufferId, ResourceError> {
        let wgpu_buffer_descriptor = buffer_descriptor(descriptor);
        let wgpu_buffer = self.wgpu_device().create_buffer(&wgpu_buffer_descriptor);
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
        self.with_registry(|r| {
            r.buffers.insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: descriptor.size,
                },
            )
        })?;

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
        let wgpu_buffer =
            self.wgpu_device()
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
        self.with_registry(|r| {
            r.buffers.insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: buffer_size,
                },
            )
        })?;

        log::debug!(
            "WgpuDevice: Created buffer '{:?}' with initial data. ID: {:?}, size: {} bytes",
            descriptor.label.as_deref().unwrap_or_default(),
            id,
            buffer_size
        );
        Ok(id)
    }

    fn destroy_buffer(&self, id: api_buf::BufferId) -> Result<(), ResourceError> {
        // Remove the buffer from the map and track VRAM usage
        if let Some(entry) = self.with_registry(|r| r.buffers.remove(&id))? {
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
        // 1. The buffer, cloned out of the registry.
        let wgpu_buffer = self
            .with_registry(|r| r.buffers.get(&id).map(|e| Arc::clone(&e.wgpu_buffer)))?
            .ok_or(ResourceError::NotFound)?;

        // 2. Check the bounds
        let buffer_size = wgpu_buffer.size();
        let end_offset = offset + data.len() as u64;
        if end_offset > buffer_size {
            return Err(ResourceError::OutOfBounds);
        }

        // 3. Write directly. No padding, no allocation. It's simple and efficient.
        self.wgpu_queue().write_buffer(&wgpu_buffer, offset, data);

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
        let entry_wgpu_buffer: Arc<wgpu::Buffer> =
            match self.with_registry(|r| r.buffers.get(&id).map(|e| Arc::clone(&e.wgpu_buffer))) {
                Ok(Some(buffer)) => buffer,
                Ok(None) => return Box::new(async { Err(ResourceError::NotFound) }),
                Err(error) => return Box::new(async { Err(error) }),
            };

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
        let wgpu_texture = self.wgpu_device().create_texture(&wgpu_texture_descriptor);
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
        self.with_registry(|r| {
            r.textures.insert(
                id,
                WgpuTextureEntry {
                    wgpu_texture: Arc::new(wgpu_texture),
                    size: size_in_bytes,
                },
            )
        })?;

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
        // Remove the texture from the map and track VRAM usage
        if let Some(entry) = self.with_registry(|r| r.textures.remove(&id))? {
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
        let wgpu_texture = self
            .with_registry(|r| {
                r.textures
                    .get(&texture_id)
                    .map(|e| Arc::clone(&e.wgpu_texture))
            })?
            .ok_or(ResourceError::NotFound)?;

        self.wgpu_queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &wgpu_texture,
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
        let wgpu_texture = self
            .with_registry(|r| {
                r.textures
                    .get(&texture_id)
                    .map(|e| Arc::clone(&e.wgpu_texture))
            })?
            .ok_or(ResourceError::NotFound)?;

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
        let wgpu_view = Arc::new(wgpu_texture.create_view(&wgpu_view_descriptor));
        let id = self.generate_texture_view_id();
        self.with_registry(|r| {
            r.texture_views.insert(
                id,
                WgpuTextureViewEntry {
                    wgpu_view,
                    source_texture_id: Some(texture_id),
                },
            )
        })?;
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
        // Remove the texture view from the map
        if self
            .with_registry(|r| r.texture_views.remove(&id))?
            .is_some()
        {
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
        let wgpu_sampler = Arc::new(self.wgpu_device().create_sampler(&wgpu_sampler_descriptor));
        let id = self.generate_sampler_id();
        self.with_registry(|r| r.samplers.insert(id, WgpuSamplerEntry { wgpu_sampler }))?;
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
        // Remove the sampler from the map
        if self.with_registry(|r| r.samplers.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed sampler with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn get_surface_format(&self) -> Option<TextureFormat> {
        self.surface().format
    }

    fn get_surface_size(&self) -> (u32, u32) {
        self.surface().size
    }

    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        self.internal
            .adapter
            .get()
            .cloned()
            .unwrap_or_else(|| GraphicsAdapterInfo {
                name: "<undescribed adapter>".to_owned(),
                backend_type: GraphicsBackendType::Unknown,
                device_type: RendererDeviceType::Unknown,
            })
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let features = self.wgpu_device().features();
        match feature_name {
            "gpu_timestamps" => features.contains(wgpu::Features::TIMESTAMP_QUERY),
            "texture_compression_bc" => features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            "polygon_mode_line" => features.contains(wgpu::Features::POLYGON_MODE_LINE),
            "polygon_mode_point" => features.contains(wgpu::Features::POLYGON_MODE_POINT),
            _ => {
                log::warn!(
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {feature_name}"
                );
                false
            }
        }
    }

    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        let descriptor = wgpu::CommandEncoderDescriptor { label };
        let encoder = self.wgpu_device().create_command_encoder(&descriptor);

        Box::new(WgpuCommandEncoder {
            encoder: Some(encoder), // Wrap in Option to be `take`n in finish()
            device: self.clone(),   // Clone the Arc handle
        })
    }

    fn submit_command_buffer(&self, command_buffer_id: api_cmd::CommandBufferId) {
        let buffer =
            match self.with_registry(|r| r.pending_command_buffers.remove(&command_buffer_id)) {
                Ok(Some(buffer)) => buffer,
                Ok(None) => {
                    log::error!(
                        "Attempted to submit a CommandBufferId ({:?}) that does not exist.",
                        command_buffer_id
                    );
                    return;
                }
                Err(e) => {
                    log::error!(
                        "WgpuDevice::submit_command_buffer: {e}, dropping {:?}",
                        command_buffer_id
                    );
                    return;
                }
            };
        self.wgpu_queue().submit(std::iter::once(buffer));
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
        self.with_registry(|r| {
            r.bind_group_layouts
                .insert(id, WgpuBindGroupLayoutEntry { wgpu_layout })
        })?;

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

        // Resource storage to hold Arcs alive
        enum ResourceArc {
            Buffer(u32, Arc<wgpu::Buffer>, u64, Option<std::num::NonZeroU64>),
            TextureView(u32, Arc<wgpu::TextureView>),
            Sampler(u32, Arc<wgpu::Sampler>),
        }

        // The layout and every bound resource, cloned out of the registry in
        // one pass.
        let (wgpu_layout, resource_arcs) = self
            .with_registry(|r| {
                let layout = r
                    .bind_group_layouts
                    .get(&descriptor.layout)
                    .map(|e| Arc::clone(&e.wgpu_layout))?;
                let resources = descriptor
                    .entries
                    .iter()
                    .map(|entry| match &entry.resource {
                        BindingResource::Buffer(binding) => {
                            r.buffers.get(&binding.buffer).map(|e| {
                                ResourceArc::Buffer(
                                    entry.binding,
                                    Arc::clone(&e.wgpu_buffer),
                                    binding.offset,
                                    binding.size,
                                )
                            })
                        }
                        BindingResource::TextureView(view_id) => {
                            r.texture_views.get(view_id).map(|e| {
                                ResourceArc::TextureView(entry.binding, Arc::clone(&e.wgpu_view))
                            })
                        }
                        BindingResource::Sampler(sampler_id) => {
                            r.samplers.get(sampler_id).map(|e| {
                                ResourceArc::Sampler(entry.binding, Arc::clone(&e.wgpu_sampler))
                            })
                        }
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((layout, resources))
            })?
            .ok_or(ResourceError::NotFound)?;

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
        self.with_registry(|r| {
            r.bind_groups
                .insert(id, WgpuBindGroupEntry { wgpu_bind_group })
        })?;

        log::debug!(
            "WgpuDevice: Created bind group '{:?}' with ID: {:?}",
            descriptor.label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        if self
            .with_registry(|r| r.bind_group_layouts.remove(&id))?
            .is_some()
        {
            log::debug!("WgpuDevice: Destroyed bind group layout with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        if self.with_registry(|r| r.bind_groups.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed bind group with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }
}
