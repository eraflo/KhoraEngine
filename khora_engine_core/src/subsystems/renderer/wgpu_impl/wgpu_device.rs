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
use crate::core::monitoring::{self as core_monitoring};
use crate::math::dimension::{self as math_dim};
use crate::subsystems::renderer::api::buffer_types::{self as api_buf};
use crate::subsystems::renderer::api::common_types::RendererAdapterInfo;
use crate::subsystems::renderer::api::pipeline_types::{
    RenderPipelineDescriptor, RenderPipelineId,
};
use crate::subsystems::renderer::api::shader_types::{
    ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData,
};
use crate::subsystems::renderer::api::texture_types::{self as api_tex};
use crate::subsystems::renderer::error::{PipelineError, ResourceError, ShaderError};
use crate::subsystems::renderer::traits::graphics_device::GraphicsDevice;
use crate::subsystems::renderer::{RendererBackendType, RendererDeviceType};

use std::borrow::Cow;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Poll, Waker};
use wgpu;

struct MapAsyncFutureState {
    result: Mutex<Option<Result<(), ResourceError>>>,
    // The Waker to wake up the Future when the result is ready
    waker: Mutex<Option<Waker>>,
}

// Custom Future implementation to wrap the MapAsyncFutureState
struct MapAsyncOperationFuture {
    state: Arc<MapAsyncFutureState>,
}

impl Future for MapAsyncOperationFuture {
    type Output = Result<(), ResourceError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut result_guard = self.state.result.lock().unwrap();
        if let Some(res) = result_guard.take() {
            Poll::Ready(res)
        } else {
            // Store the waker so the callback can wake this Future later
            let mut waker_guard = self.state.waker.lock().unwrap();
            *waker_guard = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

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
pub(crate) struct WgpuBufferEntry {
    pub(crate) wgpu_buffer: Arc<wgpu::Buffer>,
    pub(crate) size: u64, // To track VRAM accurately on destruction
}

#[derive(Debug)]
pub(crate) struct WgpuTextureEntry {
    pub(crate) wgpu_texture: Arc<wgpu::Texture>,
    pub(crate) size: u64, // To track VRAM accurately on destruction
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuTextureViewEntry {
    pub(crate) wgpu_view: Arc<wgpu::TextureView>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuSamplerEntry {
    pub(crate) wgpu_sampler: Arc<wgpu::Sampler>,
}

#[derive(Debug)]
pub struct WgpuDevice {
    context: Arc<Mutex<WgpuGraphicsContext>>,
    shader_modules: Mutex<HashMap<ShaderModuleId, WgpuShaderModuleEntry>>,
    pipelines: Mutex<HashMap<RenderPipelineId, WgpuRenderPipelineEntry>>,
    buffers: Mutex<HashMap<api_buf::BufferId, WgpuBufferEntry>>,
    textures: Mutex<HashMap<api_tex::TextureId, WgpuTextureEntry>>,
    texture_views: Mutex<HashMap<api_tex::TextureViewId, WgpuTextureViewEntry>>,
    samplers: Mutex<HashMap<api_tex::SamplerId, WgpuSamplerEntry>>,

    next_shader_id: AtomicUsize,
    next_pipeline_id: AtomicUsize,
    next_buffer_id: AtomicUsize,
    next_texture_id: AtomicUsize,
    next_texture_view_id: AtomicUsize,
    next_sampler_id: AtomicUsize,

    // VRAM Tracking
    vram_allocated_bytes: AtomicUsize,
    vram_peak_bytes: AtomicU64,
}

impl WgpuDevice {
    pub fn new(context: Arc<Mutex<WgpuGraphicsContext>>) -> Self {
        Self {
            context,
            shader_modules: Mutex::new(HashMap::new()),
            pipelines: Mutex::new(HashMap::new()),
            buffers: Mutex::new(HashMap::new()),
            textures: Mutex::new(HashMap::new()),
            texture_views: Mutex::new(HashMap::new()),
            samplers: Mutex::new(HashMap::new()),
            next_shader_id: AtomicUsize::new(0),
            next_pipeline_id: AtomicUsize::new(0),
            next_buffer_id: AtomicUsize::new(0),
            next_texture_id: AtomicUsize::new(0),
            next_texture_view_id: AtomicUsize::new(0),
            next_sampler_id: AtomicUsize::new(0),
            vram_allocated_bytes: AtomicUsize::new(0),
            vram_peak_bytes: AtomicU64::new(0),
        }
    }

    // --- ID Generation Helpers ---

    fn generate_shader_id(&self) -> ShaderModuleId {
        ShaderModuleId(self.next_shader_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_pipeline_id(&self) -> RenderPipelineId {
        RenderPipelineId(self.next_pipeline_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_buffer_id(&self) -> api_buf::BufferId {
        api_buf::BufferId(self.next_buffer_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_texture_id(&self) -> api_tex::TextureId {
        api_tex::TextureId(self.next_texture_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_texture_view_id(&self) -> api_tex::TextureViewId {
        api_tex::TextureViewId(self.next_texture_view_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_sampler_id(&self) -> api_tex::SamplerId {
        api_tex::SamplerId(self.next_sampler_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Helper function to execute an operation with the wgpu::Device locked.
    /// Returns a Result to propagate lock errors or operation errors.
    fn with_wgpu_device<F, R>(&self, operation: F) -> Result<R, ResourceError>
    where
        F: FnOnce(&wgpu::Device) -> Result<R, ResourceError>,
    {
        let context_guard = self.context.lock().map_err(|e| {
            ResourceError::BackendError(format!("Failed to lock WgpuGraphicsContext: {e}"))
        })?;
        operation(&context_guard.device)
    }

    /// Helper to calculate texture size in bytes
    fn calculate_texture_size_in_bytes(descriptor: &api_tex::TextureDescriptor) -> u64 {
        // This is a simplified calculation. Real engines consider block compression, padding, etc.
        let bytes_per_pixel = descriptor.format.bytes_per_pixel();
        let num_pixels = descriptor.size.width as u64
            * descriptor.size.height as u64
            * descriptor.size.depth_or_array_layers as u64;
        num_pixels * bytes_per_pixel as u64
    }
}

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
        let mut modules_guard = self.shader_modules.lock().map_err(|e| {
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
        let shader_modules_map = self.shader_modules.lock().map_err(|e| {
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
                    step_mode: vb_layout_desc.step_mode.into(),
                    attributes: attributes_for_this_layout,
                },
            )
            .collect();

        // 3. Converts primitive state
        let primitive_state = wgpu::PrimitiveState {
            topology: descriptor.primitive_state.topology.into(),
            strip_index_format: descriptor
                .primitive_state
                .strip_index_format
                .map(|f| f.into()),
            front_face: descriptor.primitive_state.front_face.into(),
            cull_mode: descriptor.primitive_state.cull_mode.map(|m| m.into()),
            polygon_mode: descriptor.primitive_state.polygon_mode.into(),
            unclipped_depth: descriptor.primitive_state.unclipped_depth,
            conservative: descriptor.primitive_state.conservative,
        };

        // 4. Convert depth stencil state
        let depth_stencil_state =
            descriptor
                .depth_stencil_state
                .as_ref()
                .map(|ds| wgpu::DepthStencilState {
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
        let color_target_states: Vec<Option<wgpu::ColorTargetState>> = descriptor
            .color_target_states
            .iter()
            .map(|cts| {
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
            })
            .collect();

        // 6. Convert multisample state
        let multisample_state = wgpu::MultisampleState {
            count: descriptor.multisample_state.count.into(),
            mask: descriptor.multisample_state.mask as u64,
            alpha_to_coverage_enabled: descriptor.multisample_state.alpha_to_coverage_enabled,
        };

        // 7. Create pipeline layout and render pipeline
        let (wgpu_render_pipeline_arc, id) = self.with_wgpu_device(|device| {
            let pipeline_layout_label = descriptor.label.as_deref().map(|s| format!("{s}_Layout"));
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

        let mut pipelines_guard = self
            .pipelines
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (pipelines): {e}")))?;
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
                .map(|s| s.as_ref())
                .unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        let mut pipelines_guard = self
            .pipelines
            .lock()
            .map_err(|e| ResourceError::BackendError(format!("Mutex poisoned (pipelines): {e}")))?;

        if pipelines_guard.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed render pipeline with ID: {id:?}");
            Ok(())
        } else {
            Err(PipelineError::InvalidRenderPipeline { id }.into())
        }
    }

    // --- Buffer Operations ---

    fn create_buffer(
        &self,
        descriptor: &api_buf::BufferDescriptor,
    ) -> Result<api_buf::BufferId, ResourceError> {
        let context = self.context.lock().unwrap();
        let device = &context.device;

        // Create the buffer using the wgpu device
        let wgpu_buffer_descriptor = wgpu::BufferDescriptor {
            label: descriptor.label.as_deref(),
            size: descriptor.size,
            usage: wgpu::BufferUsages::from_bits_truncate(descriptor.usage.bits()),
            mapped_at_creation: descriptor.mapped_at_creation,
        };

        let wgpu_buffer = device.create_buffer(&wgpu_buffer_descriptor);
        let id = self.generate_buffer_id();

        // Track VRAM usage
        self.vram_allocated_bytes
            .fetch_add(descriptor.size as usize, Ordering::Relaxed);
        let current_vram = self.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the buffer into the map
        self.buffers.lock().unwrap().insert(
            id,
            WgpuBufferEntry {
                wgpu_buffer: Arc::new(wgpu_buffer),
                size: descriptor.size,
            },
        );

        log::info!(
            "WgpuDevice: Created buffer '{:?}' with ID: {:?}, size: {} bytes",
            descriptor
                .label
                .as_ref()
                .map(|s| s.as_ref())
                .unwrap_or_default(),
            id,
            descriptor.size
        );
        Ok(id)
    }

    fn destroy_buffer(&self, id: api_buf::BufferId) -> Result<(), ResourceError> {
        let mut buffers = self.buffers.lock().unwrap();

        // Remove the buffer from the map and track VRAM usage
        if let Some(entry) = buffers.remove(&id) {
            self.vram_allocated_bytes
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
        let buffers = self.buffers.lock().unwrap();
        let entry = buffers.get(&id).ok_or(ResourceError::NotFound)?;
        let context = self.context.lock().unwrap();
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
        let buffers_guard = self.buffers.lock().unwrap();
        let entry_wgpu_buffer = match buffers_guard.get(&id) {
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

            // Signal the Future that the operation is complete
            *future_state_for_callback.result.lock().unwrap() = Some(final_result);

            // Wake the Future if a waker was stored
            if let Some(waker) = future_state_for_callback.waker.lock().unwrap().take() {
                waker.wake();
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
        let context = self.context.lock().unwrap();
        let device = &context.device;

        let wgpu_texture_descriptor = wgpu::TextureDescriptor {
            label: descriptor.label.as_deref(),
            size: descriptor.size.into(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count.into(),
            dimension: descriptor.dimension.into(),
            format: descriptor.format.into(),
            usage: wgpu::TextureUsages::from_bits_truncate(descriptor.usage.bits()),
            view_formats: &descriptor
                .view_formats
                .iter()
                .map(|&f| f.into())
                .collect::<Vec<_>>(),
        };

        // Create the texture using the wgpu device with the specified descriptor
        let wgpu_texture = device.create_texture(&wgpu_texture_descriptor);
        let id = self.generate_texture_id();
        let size_in_bytes = Self::calculate_texture_size_in_bytes(descriptor);

        // Track VRAM usage
        self.vram_allocated_bytes
            .fetch_add(size_in_bytes as usize, Ordering::Relaxed);
        let current_vram = self.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the texture into the map
        self.textures.lock().unwrap().insert(
            id,
            WgpuTextureEntry {
                wgpu_texture: Arc::new(wgpu_texture),
                size: size_in_bytes,
            },
        );

        log::info!(
            "WgpuDevice: Created texture '{:?}' with ID: {:?}, size: {} bytes (VRAM)",
            descriptor
                .label
                .as_ref()
                .map(|s| s.as_ref())
                .unwrap_or_default(),
            id,
            size_in_bytes
        );
        Ok(id)
    }

    fn destroy_texture(&self, id: api_tex::TextureId) -> Result<(), ResourceError> {
        let mut textures = self.textures.lock().unwrap();

        // Remove the texture from the map and track VRAM usage
        if let Some(entry) = textures.remove(&id) {
            self.vram_allocated_bytes
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
        offset: math_dim::Origin3D,
        size: math_dim::Extent3D,
    ) -> Result<(), ResourceError> {
        let textures = self.textures.lock().unwrap();
        let entry = textures.get(&texture_id).ok_or(ResourceError::NotFound)?;
        let context = self.context.lock().unwrap();

        context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &entry.wgpu_texture,
                mip_level: 0, // Assuming base mip for now
                origin: offset.into(),
                aspect: wgpu::TextureAspect::All, // Assuming all aspects for now
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row,
                rows_per_image: None, // Assuming 2D or 3D without specific rows per image info
            },
            size.into(),
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
        let textures = self.textures.lock().unwrap();
        let texture_entry = textures.get(&texture_id).ok_or(ResourceError::NotFound)?;

        let wgpu_view_descriptor = wgpu::TextureViewDescriptor {
            label: descriptor.label.as_deref(),
            format: descriptor.format.map(|f| f.into()),
            dimension: descriptor.dimension.map(|d| d.into()),
            aspect: descriptor.aspect.into(),
            base_mip_level: descriptor.base_mip_level,
            mip_level_count: descriptor.mip_level_count,
            base_array_layer: descriptor.base_array_layer,
            array_layer_count: descriptor.array_layer_count,
            usage: None, // Not used in this context
        };

        // Create the texture view using the wgpu texture
        let wgpu_view = Arc::new(
            texture_entry
                .wgpu_texture
                .create_view(&wgpu_view_descriptor),
        );
        let id = self.generate_texture_view_id();
        self.texture_views
            .lock()
            .unwrap()
            .insert(id, WgpuTextureViewEntry { wgpu_view });
        log::info!(
            "WgpuDevice: Created texture view '{:?}' for texture ID: {:?} with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s| s.as_ref())
                .unwrap_or_default(),
            texture_id,
            id
        );
        Ok(id)
    }

    fn destroy_texture_view(&self, id: api_tex::TextureViewId) -> Result<(), ResourceError> {
        let mut texture_views = self.texture_views.lock().unwrap();

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
        let context = self.context.lock().unwrap();
        let device = &context.device;

        let wgpu_sampler_descriptor = wgpu::SamplerDescriptor {
            label: descriptor.label.as_deref(),
            address_mode_u: descriptor.address_mode_u.into(),
            address_mode_v: descriptor.address_mode_v.into(),
            address_mode_w: descriptor.address_mode_w.into(),
            mag_filter: descriptor.mag_filter.into(),
            min_filter: descriptor.min_filter.into(),
            mipmap_filter: descriptor.mipmap_filter.into(),
            lod_min_clamp: descriptor.lod_min_clamp,
            lod_max_clamp: descriptor.lod_max_clamp,
            compare: descriptor.compare.map(|f| f.into()),
            anisotropy_clamp: descriptor.anisotropy_clamp,
            border_color: descriptor.border_color.map(|c| c.into()),
        };

        // Create the sampler using the wgpu device
        let wgpu_sampler = Arc::new(device.create_sampler(&wgpu_sampler_descriptor));
        let id = self.generate_sampler_id();
        self.samplers
            .lock()
            .unwrap()
            .insert(id, WgpuSamplerEntry { wgpu_sampler });
        log::info!(
            "WgpuDevice: Created sampler '{:?}' with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s| s.as_ref())
                .unwrap_or_default(),
            id
        );
        Ok(id)
    }

    fn destroy_sampler(&self, id: api_tex::SamplerId) -> Result<(), ResourceError> {
        let mut samplers = self.samplers.lock().unwrap();

        // Remove the sampler from the map
        if samplers.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed sampler with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
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
                    "WgpuDevice: Unsupported feature_name query in supports_feature: {feature_name}"
                );
                false
            }
        }
    }
}

impl core_monitoring::ResourceMonitor for WgpuDevice {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Borrowed("WgpuDevice_VRAM_Monitor") // Simple to begin (TODO: use dynamic adapter name)
    }

    fn resource_type(&self) -> core_monitoring::MonitoredResourceType {
        core_monitoring::MonitoredResourceType::Vram
    }

    fn get_usage_report(&self) -> core_monitoring::ResourceUsageReport {
        core_monitoring::ResourceUsageReport {
            current_bytes: self.vram_allocated_bytes.load(Ordering::Relaxed) as u64,
            peak_bytes: Some(self.vram_peak_bytes.load(Ordering::Relaxed)),
            total_capacity_bytes: None, //  Difficult to determine in WGPU
        }
    }
}
