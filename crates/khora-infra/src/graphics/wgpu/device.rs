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

use khora_core::telemetry::{
    MonitoredResourceType, ResourceMonitor, ResourceUsageReport, VramProvider,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use wgpu;
use wgpu::util::DeviceExt;

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
use khora_core::renderer::api::util::{
    GraphicsBackendType, IndexFormat, RendererDeviceType, TextureFormat,
};
use khora_core::renderer::traits::CommandEncoder;
use khora_core::renderer::{GraphicsDevice, PipelineError, ResourceError, ShaderError};

use crate::graphics::wgpu::command::WgpuCommandEncoder;
use crate::graphics::wgpu::conversions::{from_wgpu_texture_format, IntoWgpu};

use super::context::WgpuGraphicsContext;
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
pub(crate) struct WgpuRenderPipelineEntry {
    pub(crate) wgpu_pipeline: Arc<wgpu::RenderPipeline>,
}

#[derive(Debug)]
pub(crate) struct WgpuComputePipelineEntry {
    pub(crate) wgpu_pipeline: Arc<wgpu::ComputePipeline>,
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
#[derive(Debug, Clone)]
pub(crate) struct WgpuTextureViewEntry {
    pub(crate) wgpu_view: Arc<wgpu::TextureView>,
    pub(crate) source_texture_id: Option<api_tex::TextureId>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuSamplerEntry {
    pub(crate) wgpu_sampler: Arc<wgpu::Sampler>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuBindGroupLayoutEntry {
    pub(crate) wgpu_layout: Arc<wgpu::BindGroupLayout>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct WgpuBindGroupEntry {
    pub(crate) wgpu_bind_group: Arc<wgpu::BindGroup>,
}

#[derive(Debug)]
pub(crate) struct WgpuPipelineLayoutEntry {
    pub(crate) wgpu_layout: Arc<wgpu::PipelineLayout>,
}

/// The internal, non-clonable state of the WgpuDevice.
/// This struct holds all the GPU resources and state, protected by an Arc.
#[derive(Debug)]
pub struct WgpuDeviceInternal {
    context: Arc<Mutex<WgpuGraphicsContext>>,
    shader_modules: Mutex<HashMap<ShaderModuleId, WgpuShaderModuleEntry>>,
    pipelines: Mutex<HashMap<RenderPipelineId, WgpuRenderPipelineEntry>>,
    compute_pipelines: Mutex<HashMap<ComputePipelineId, WgpuComputePipelineEntry>>,
    buffers: Mutex<HashMap<api_buf::BufferId, WgpuBufferEntry>>,
    textures: Mutex<HashMap<api_tex::TextureId, WgpuTextureEntry>>,
    texture_views: Mutex<HashMap<api_tex::TextureViewId, WgpuTextureViewEntry>>,
    samplers: Mutex<HashMap<api_tex::SamplerId, WgpuSamplerEntry>>,
    bind_group_layouts: Mutex<HashMap<BindGroupLayoutId, WgpuBindGroupLayoutEntry>>,
    bind_groups: Mutex<HashMap<BindGroupId, WgpuBindGroupEntry>>,
    pipeline_layouts: Mutex<HashMap<PipelineLayoutId, WgpuPipelineLayoutEntry>>,

    next_shader_id: AtomicUsize,
    next_pipeline_id: AtomicUsize,
    #[allow(dead_code)] // Will be used when create_compute_pipeline is implemented
    next_compute_pipeline_id: AtomicU64,
    next_pipeline_layout_id: AtomicUsize,
    next_buffer_id: AtomicUsize,
    next_texture_id: AtomicUsize,
    next_texture_view_id: AtomicUsize,
    next_sampler_id: AtomicUsize,
    next_bind_group_layout_id: AtomicUsize,
    next_bind_group_id: AtomicUsize,

    // VRAM Tracking
    vram_allocated_bytes: AtomicUsize,
    vram_peak_bytes: AtomicU64,

    /// Command buffers that have been finished but not yet submitted.
    pending_command_buffers: Mutex<HashMap<api_cmd::CommandBufferId, wgpu::CommandBuffer>>,
    /// A thread-safe counter to generate unique command buffer IDs.
    command_buffer_id_counter: AtomicU64,
}

/// A clonable, thread-safe handle to the WGPU graphics device.
/// It wraps the actual device state (`WgpuDeviceInternal`) in an Arc,
/// allowing it to be shared across threads and with command encoders.
#[derive(Clone, Debug)]
pub struct WgpuDevice {
    internal: Arc<WgpuDeviceInternal>,
}

impl WgpuDevice {
    pub fn new(context: Arc<Mutex<WgpuGraphicsContext>>) -> Self {
        Self {
            internal: Arc::new(WgpuDeviceInternal {
                context,
                shader_modules: Mutex::new(HashMap::new()),
                pipelines: Mutex::new(HashMap::new()),
                compute_pipelines: Mutex::new(HashMap::new()),
                buffers: Mutex::new(HashMap::new()),
                textures: Mutex::new(HashMap::new()),
                texture_views: Mutex::new(HashMap::new()),
                samplers: Mutex::new(HashMap::new()),
                bind_group_layouts: Mutex::new(HashMap::new()),
                bind_groups: Mutex::new(HashMap::new()),
                pipeline_layouts: Mutex::new(HashMap::new()),
                next_shader_id: AtomicUsize::new(0),
                next_pipeline_id: AtomicUsize::new(0),
                next_compute_pipeline_id: AtomicU64::new(0),
                next_pipeline_layout_id: AtomicUsize::new(0),
                next_buffer_id: AtomicUsize::new(0),
                next_texture_id: AtomicUsize::new(0),
                next_texture_view_id: AtomicUsize::new(0),
                next_sampler_id: AtomicUsize::new(0),
                next_bind_group_layout_id: AtomicUsize::new(0),
                next_bind_group_id: AtomicUsize::new(0),
                vram_allocated_bytes: AtomicUsize::new(0),
                vram_peak_bytes: AtomicU64::new(0),
                pending_command_buffers: Mutex::new(HashMap::new()),
                command_buffer_id_counter: AtomicU64::new(0),
            }),
        }
    }

    // --- ID Generation Helpers ---

    fn generate_shader_id(&self) -> ShaderModuleId {
        ShaderModuleId(self.internal.next_shader_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_pipeline_id(&self) -> RenderPipelineId {
        RenderPipelineId(
            self.internal
                .next_pipeline_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    fn generate_buffer_id(&self) -> api_buf::BufferId {
        api_buf::BufferId(self.internal.next_buffer_id.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_texture_id(&self) -> api_tex::TextureId {
        api_tex::TextureId(
            self.internal
                .next_texture_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    fn generate_texture_view_id(&self) -> api_tex::TextureViewId {
        api_tex::TextureViewId(
            self.internal
                .next_texture_view_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    fn generate_sampler_id(&self) -> api_tex::SamplerId {
        api_tex::SamplerId(
            self.internal
                .next_sampler_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    fn generate_bind_group_layout_id(&self) -> BindGroupLayoutId {
        BindGroupLayoutId(
            self.internal
                .next_bind_group_layout_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    fn generate_bind_group_id(&self) -> BindGroupId {
        BindGroupId(
            self.internal
                .next_bind_group_id
                .fetch_add(1, Ordering::Relaxed),
        )
    }

    /// Helper function to execute an operation with the wgpu::Device locked.
    /// Returns a Result to propagate lock errors or operation errors.
    fn with_wgpu_device<F, R>(&self, operation: F) -> Result<R, ResourceError>
    where
        F: FnOnce(&wgpu::Device) -> Result<R, ResourceError>,
    {
        let context_guard = self.internal.context.lock().map_err(|e| {
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

    /// Retrieves a reference-counted pointer to the internal WGPU render pipeline.
    /// Returns `None` if the ID is invalid.
    pub fn get_wgpu_render_pipeline(
        &self,
        id: RenderPipelineId,
    ) -> Option<Arc<wgpu::RenderPipeline>> {
        let pipelines = self.internal.pipelines.lock().unwrap();
        pipelines
            .get(&id)
            .map(|entry| Arc::clone(&entry.wgpu_pipeline))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU compute pipeline.
    /// Returns `None` if the ID is invalid.
    pub fn get_wgpu_compute_pipeline(
        &self,
        id: ComputePipelineId,
    ) -> Option<Arc<wgpu::ComputePipeline>> {
        let pipelines = self.internal.compute_pipelines.lock().unwrap();
        pipelines
            .get(&id)
            .map(|entry| Arc::clone(&entry.wgpu_pipeline))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU buffer.
    /// Returns `None` if the ID is invalid.
    pub fn get_wgpu_buffer(&self, id: api_buf::BufferId) -> Option<Arc<wgpu::Buffer>> {
        let buffers = self.internal.buffers.lock().unwrap();
        buffers.get(&id).map(|entry| Arc::clone(&entry.wgpu_buffer))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU texture view.
    /// Returns `None` if the ID is invalid.
    pub fn get_wgpu_texture_view(
        &self,
        id: &api_tex::TextureViewId,
    ) -> Option<Arc<wgpu::TextureView>> {
        let views = self.internal.texture_views.lock().unwrap();
        views.get(id).map(|entry| Arc::clone(&entry.wgpu_view))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU bind group.
    /// Returns `None` if the ID is invalid.
    pub fn get_wgpu_bind_group(&self, id: BindGroupId) -> Option<Arc<wgpu::BindGroup>> {
        let bind_groups = self.internal.bind_groups.lock().unwrap();
        bind_groups
            .get(&id)
            .map(|entry| Arc::clone(&entry.wgpu_bind_group))
    }

    /// Polls the underlying wgpu::Device in a blocking manner.
    /// This is primarily used during shutdown to ensure all pending operations
    /// and callbacks are completed before resources are destroyed, preventing panics.
    pub fn poll_device_blocking(&self) {
        if let Ok(context_guard) = self.internal.context.lock() {
            // PollType::Wait is blocking and will wait for the queue to be empty
            // and for all `on_submitted_work_done` callbacks to be processed.
            if let Err(e) = context_guard.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            }) {
                log::warn!("Failed to poll device during shutdown: {:?}", e);
            }
        } else {
            log::error!("WgpuDevice context mutex was poisoned during shutdown poll.");
        }
    }

    /// Polls the underlying wgpu::Device in a non-blocking manner.
    /// This is essential to process pending `map_async` callbacks from the GPU,
    /// allowing systems like the GpuProfiler to receive data.
    pub fn poll_device_non_blocking(&self) {
        if let Ok(context_guard) = self.internal.context.lock() {
            // PollType::Poll is non-blocking. It processes any completed work
            // but returns immediately if there is none.
            if let Err(e) = context_guard.device.poll(wgpu::PollType::Poll) {
                log::warn!("Failed to poll device (non-blocking): {:?}", e);
            }
        }
    }

    /// Creates a texture view for a raw wgpu::Texture (e.g., from the swap chain)
    /// and registers it with the device, returning an abstract ID.
    pub(crate) fn register_texture_view(
        &self,
        texture: &wgpu::Texture,
        label: Option<&str>,
    ) -> Result<api_tex::TextureViewId, ResourceError> {
        let wgpu_view = Arc::new(texture.create_view(&wgpu::TextureViewDescriptor {
            label,
            ..Default::default()
        }));
        let id = self.generate_texture_view_id();
        self.internal.texture_views.lock().unwrap().insert(
            id,
            WgpuTextureViewEntry {
                wgpu_view,
                source_texture_id: None,
            },
        );
        Ok(id)
    }

    /// (crate-internal) Registers a finished wgpu::CommandBuffer, storing it
    /// in a map and returning an abstract ID for it.
    pub(crate) fn register_command_buffer(
        &self,
        buffer: wgpu::CommandBuffer,
    ) -> api_cmd::CommandBufferId {
        let new_id_raw = self
            .internal
            .command_buffer_id_counter
            .fetch_add(1, Ordering::SeqCst);
        let new_id = api_cmd::CommandBufferId(new_id_raw);

        let mut guard = self.internal.pending_command_buffers.lock().unwrap();
        guard.insert(new_id, buffer);

        new_id
    }

    pub fn supports_feature(&self, _feature_name: &str) -> bool {
        let _context = self.internal.context.lock().unwrap();
        // features is a struct in wgpu, not a set with from_name in older versions?
        // Actually wgpu::Features has bits.
        // For now, let's just return true for "depth-clip-control" if we can.
        // A better implementation would find the feature in the adapter features.
        true
    }

    pub(crate) fn get_wgpu_texture_view_entry(
        &self,
        id: api_tex::TextureViewId,
    ) -> Option<WgpuTextureViewEntry> {
        self.internal
            .texture_views
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
    }

    pub(crate) fn get_wgpu_texture(&self, id: api_tex::TextureId) -> Option<Arc<wgpu::Texture>> {
        self.internal
            .textures
            .lock()
            .unwrap()
            .get(&id)
            .map(|e| Arc::clone(&e.wgpu_texture))
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
                    depth_write_enabled: ds.depth_write_enabled,
                    depth_compare: ds.depth_compare.into_wgpu(),
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
                 let layouts = self.internal.pipeline_layouts.lock().unwrap(); // Potential deadlock if called from within another lock?
                 // Note: create_render_pipeline already locks shader_modules.
                 // Locking pipeline_layouts here is fine as long as strict ordering is maintained.
                 // Ideally we should get it before entering with_wgpu_device callback if we can,
                 // but with_wgpu_device just locks context.
                 // So: Lock Context -> Lock ShaderModules -> Lock PipelineLayouts.
                 // Safe.
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
                    .map(|l: &&Arc<wgpu::BindGroupLayout>| l.as_ref())
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
        let context = self.internal.context.lock().unwrap();
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
            let layouts = self.internal.pipeline_layouts.lock().unwrap();
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
        let context = self.internal.context.lock().unwrap();
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
        self.internal
            .vram_allocated_bytes
            .fetch_add(descriptor.size as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the buffer into the map
        self.internal.buffers.lock().unwrap().insert(
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
        let context = self.internal.context.lock().unwrap();

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
        self.internal.buffers.lock().unwrap().insert(
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
        let mut buffers = self.internal.buffers.lock().unwrap();

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
        let buffers = self.internal.buffers.lock().unwrap();
        let entry = buffers.get(&id).ok_or(ResourceError::NotFound)?;
        let context = self.internal.context.lock().unwrap();

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
        let buffers_guard = self.internal.buffers.lock().unwrap();
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
        let context = self.internal.context.lock().unwrap();
        let device = &context.device;

        let wgpu_texture_descriptor = wgpu::TextureDescriptor {
            label: descriptor.label.as_deref(),
            size: descriptor.size.into_wgpu(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count.into_wgpu(),
            dimension: descriptor.dimension.into_wgpu(),
            format: descriptor.format.into_wgpu(),
            usage: wgpu::TextureUsages::from_bits_truncate(descriptor.usage.bits()),
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
        self.internal.textures.lock().unwrap().insert(
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
        let mut textures = self.internal.textures.lock().unwrap();

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
        let textures = self.internal.textures.lock().unwrap();
        let entry = textures.get(&texture_id).ok_or(ResourceError::NotFound)?;
        let context = self.internal.context.lock().unwrap();

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
        let textures = self.internal.textures.lock().unwrap();
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
        self.internal.texture_views.lock().unwrap().insert(
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
        let mut texture_views = self.internal.texture_views.lock().unwrap();

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
        let context = self.internal.context.lock().unwrap();
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
            .unwrap()
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
        let mut samplers = self.internal.samplers.lock().unwrap();

        // Remove the sampler from the map
        if samplers.remove(&id).is_some() {
            log::debug!("WgpuDevice: Destroyed sampler with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    fn get_surface_format(&self) -> Option<TextureFormat> {
        self.internal.context.lock().ok().map(|gc_guard| {
            let wgpu_format = gc_guard.surface_config.format;
            from_wgpu_texture_format(wgpu_format)
        })
    }

    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        let context_guard = self
            .internal
            .context
            .lock()
            .expect("WgpuDevice: Mutex poisoned (context) on get_adapter_info");
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
        let context_guard = self
            .internal
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

    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        let context_guard = self.internal.context.lock().unwrap();
        let descriptor = wgpu::CommandEncoderDescriptor { label };
        let encoder = context_guard.device.create_command_encoder(&descriptor);

        Box::new(WgpuCommandEncoder {
            encoder: Some(encoder), // Wrap in Option to be `take`n in finish()
            device: self.clone(),   // Clone the Arc handle
        })
    }

    fn submit_command_buffer(&self, command_buffer_id: api_cmd::CommandBufferId) {
        let mut guard = self.internal.pending_command_buffers.lock().unwrap();

        // Remove the command buffer from the map. If it doesn't exist, it's a logic error.
        if let Some(buffer) = guard.remove(&command_buffer_id) {
            let context_guard = self.internal.context.lock().unwrap();
            context_guard.queue.submit(std::iter::once(buffer));
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
                        sample_type: sample_type.clone().into_wgpu(),
                        view_dimension: view_dimension.clone().into_wgpu(),
                        multisampled: *multisampled,
                    },
                    BindingType::Sampler(sampler_ty) => {
                        wgpu::BindingType::Sampler(sampler_ty.clone().into_wgpu())
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
            let buffers_lock = self.internal.buffers.lock().unwrap();
            let texture_views_lock = self.internal.texture_views.lock().unwrap();
            let samplers_lock = self.internal.samplers.lock().unwrap();

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

impl ResourceMonitor for WgpuDevice {
    fn monitor_id(&self) -> Cow<'static, str> {
        Cow::Borrowed("WgpuDevice_VRAM_Monitor") // Simple to begin (TODO: use dynamic adapter name)
    }

    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::Vram
    }

    fn get_usage_report(&self) -> ResourceUsageReport {
        ResourceUsageReport {
            current_bytes: self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64,
            peak_bytes: Some(self.internal.vram_peak_bytes.load(Ordering::Relaxed)),
            total_capacity_bytes: None, //  Difficult to determine in WGPU
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl VramProvider for WgpuDevice {
    fn get_vram_usage_mb(&self) -> f32 {
        let bytes = self
            .internal
            .vram_allocated_bytes
            .load(std::sync::atomic::Ordering::SeqCst);
        bytes as f32 / (1024.0 * 1024.0)
    }

    fn get_vram_peak_mb(&self) -> f32 {
        let bytes = self
            .internal
            .vram_peak_bytes
            .load(std::sync::atomic::Ordering::SeqCst);
        bytes as f32 / (1024.0 * 1024.0)
    }

    fn get_vram_capacity_mb(&self) -> Option<f32> {
        // TODO: Implement VRAM capacity detection if available from adapter info
        // For now, return None as this information is not easily available in WGPU
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::telemetry::gpu_monitor::GpuMonitor;
    use khora_core::renderer::api::core::RenderStats;
    use khora_core::telemetry::{MonitoredResourceType, ResourceMonitor};

    #[test]
    fn gpu_monitor_creation() {
        let monitor = GpuMonitor::new("Test_WGPU".to_string());
        assert_eq!(monitor.monitor_id(), "Gpu_Test_WGPU");
        assert_eq!(monitor.resource_type(), MonitoredResourceType::Gpu);
        assert!(monitor.get_gpu_report().is_none());
    }

    #[test]
    fn gpu_monitor_update_stats() {
        let monitor = GpuMonitor::new("Test_WGPU".to_string());

        // Create sample render stats
        let stats = RenderStats {
            frame_number: 42,
            cpu_preparation_time_ms: 1.5,
            cpu_render_submission_time_ms: 0.2,
            gpu_main_pass_time_ms: 8.0,
            gpu_frame_total_time_ms: 10.5,
            draw_calls: 100,
            triangles_rendered: 5000,
            vram_usage_estimate_mb: 256.0,
        };

        monitor.update_from_frame_stats(&stats);

        let report = monitor.get_gpu_report().unwrap();
        assert_eq!(report.frame_number, 42);
        assert_eq!(report.main_pass_duration_us(), Some(8000)); // 8ms derived
        assert_eq!(report.frame_total_duration_us(), Some(10500)); // 10.5ms derived
        assert_eq!(report.cpu_preparation_time_us, Some(1500)); // 1.5ms = 1500μs
        assert_eq!(report.cpu_submission_time_us, Some(200)); // 0.2ms = 200μs
    }

    #[test]
    fn gpu_monitor_resource_monitor_trait() {
        let monitor = GpuMonitor::new("Test_WGPU".to_string());

        // Test ResourceMonitor trait implementation
        let usage_report = monitor.get_usage_report();
        assert_eq!(usage_report.current_bytes, 0);
        assert_eq!(usage_report.peak_bytes, None);
        assert_eq!(usage_report.total_capacity_bytes, None);

        // Performance report should be None initially
        assert!(monitor.get_gpu_report().is_none());
    }
}
