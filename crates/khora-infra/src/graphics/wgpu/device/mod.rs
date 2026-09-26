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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use wgpu;

use entries::{
    WgpuBindGroupEntry, WgpuBindGroupLayoutEntry, WgpuBufferEntry, WgpuComputePipelineEntry,
    WgpuPipelineLayoutEntry, WgpuRenderPipelineEntry, WgpuSamplerEntry, WgpuShaderModuleEntry,
    WgpuTextureEntry, WgpuTextureViewEntry,
};
use khora_core::renderer::api::command::{
    self as api_cmd, BindGroupId, BindGroupLayoutId, ComputePipelineId,
};
use khora_core::renderer::api::core::ShaderModuleId;
use khora_core::renderer::api::pipeline::{PipelineLayoutId, RenderPipelineId};
use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::api::resource::texture::{self as api_tex};

mod entries;
mod graphics_device;
mod map_async;

/// Build the wgpu descriptor for an empty buffer.
///
/// A free function rather than three lines inside `create_buffer`, and that is
/// the whole point: the bug this fixes (issue #279) lived at the call site, not
/// in the conversion it should have used. `IntoWgpu for BufferUsage` was always
/// correct; `create_buffer` reinterpreted the raw bits instead of calling it,
/// and no test could see the difference because there was nothing to call.
///
/// Now there is. A regression here fails `a_vertex_buffer_stays_a_vertex_buffer`
/// on any machine, with no GPU.
fn buffer_descriptor<'a>(
    descriptor: &'a api_buf::BufferDescriptor<'a>,
) -> wgpu::BufferDescriptor<'a> {
    wgpu::BufferDescriptor {
        label: descriptor.label.as_deref(),
        size: descriptor.size,
        usage: descriptor.usage.into_wgpu(),
        mapped_at_creation: descriptor.mapped_at_creation,
    }
}

use khora_core::renderer::ResourceError;

use crate::graphics::wgpu::conversions::IntoWgpu;

use crate::graphics::wgpu::context::WgpuGraphicsContext;

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
    /// The submission index returned by the most recent `queue.submit()` call.
    /// Used to synchronize frame acquisition with GPU completion.
    last_submission_index: Mutex<Option<wgpu::SubmissionIndex>>,

    /// Lock-free device-health flags shared with the graphics context. Raised
    /// by the wgpu error/device-lost callbacks; read by the render system each
    /// frame to detect a lost or out-of-memory device.
    health: Arc<super::resilience::GpuHealth>,
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
        // Share the context's health flags so the render system can query the
        // device directly without taking the context lock on the hot path. If
        // the context is momentarily poisoned at construction, fall back to a
        // fresh set of flags (no health signal yet, but never a panic).
        let health = match context.lock() {
            Ok(ctx) => Arc::clone(&ctx.health),
            Err(_) => {
                log::error!("WgpuDevice::new: context mutex poisoned; using detached health flags");
                Arc::new(super::resilience::GpuHealth::default())
            }
        };
        Self {
            internal: Arc::new(WgpuDeviceInternal {
                context,
                health,
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
                last_submission_index: Mutex::new(None),
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
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_render_pipeline(
        &self,
        id: RenderPipelineId,
    ) -> Option<Arc<wgpu::RenderPipeline>> {
        let pipelines = match self.internal.pipelines.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!("WgpuDevice::get_wgpu_render_pipeline: pipelines mutex poisoned");
                return None;
            }
        };
        pipelines
            .get(&id)
            .map(|entry| Arc::clone(&entry.wgpu_pipeline))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU compute pipeline.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_compute_pipeline(
        &self,
        id: ComputePipelineId,
    ) -> Option<Arc<wgpu::ComputePipeline>> {
        let pipelines = match self.internal.compute_pipelines.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::get_wgpu_compute_pipeline: compute_pipelines mutex poisoned"
                );
                return None;
            }
        };
        pipelines
            .get(&id)
            .map(|entry| Arc::clone(&entry.wgpu_pipeline))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU buffer.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_buffer(&self, id: api_buf::BufferId) -> Option<Arc<wgpu::Buffer>> {
        let buffers = match self.internal.buffers.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!("WgpuDevice::get_wgpu_buffer: buffers mutex poisoned");
                return None;
            }
        };
        buffers.get(&id).map(|entry| Arc::clone(&entry.wgpu_buffer))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU texture view.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_texture_view(
        &self,
        id: &api_tex::TextureViewId,
    ) -> Option<Arc<wgpu::TextureView>> {
        let views = match self.internal.texture_views.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!("WgpuDevice::get_wgpu_texture_view: texture_views mutex poisoned");
                return None;
            }
        };
        views.get(id).map(|entry| Arc::clone(&entry.wgpu_view))
    }

    /// Retrieves a reference-counted pointer to the internal WGPU bind group.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_bind_group(&self, id: BindGroupId) -> Option<Arc<wgpu::BindGroup>> {
        let bind_groups = match self.internal.bind_groups.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!("WgpuDevice::get_wgpu_bind_group: bind_groups mutex poisoned");
                return None;
            }
        };
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

    /// Waits for the most recent `queue.submit()` to be processed by the GPU.
    /// Must be called before `get_current_texture()` to avoid Vulkan semaphore
    /// validation errors (VUID-vkAcquireNextImageKHR-semaphore-01286).
    pub fn wait_for_last_submission(&self) {
        let idx = match self.internal.last_submission_index.lock() {
            Ok(mut g) => g.take(),
            Err(_) => {
                log::error!(
                    "WgpuDevice::wait_for_last_submission: last_submission_index mutex poisoned"
                );
                return;
            }
        };
        if let Some(submission_index) = idx {
            if let Ok(context_guard) = self.internal.context.lock() {
                if let Err(e) = context_guard.device.poll(wgpu::PollType::Wait {
                    submission_index: Some(submission_index),
                    timeout: None,
                }) {
                    log::warn!("WgpuDevice::wait_for_last_submission: device poll failed: {e:?}");
                }
            }
        }
    }

    /// Returns `true` if the underlying device has been reported lost
    /// (driver crash/reset/destroy) or hit an internal device error.
    ///
    /// Raised asynchronously by the wgpu device-lost / uncaptured-error
    /// callbacks; the render system polls this each frame to stop submitting
    /// work and surface a fatal, non-panicking error.
    pub fn is_device_lost(&self) -> bool {
        self.internal.health.is_device_lost()
    }

    /// Returns `true` if the device reported an out-of-memory condition.
    pub fn is_device_out_of_memory(&self) -> bool {
        self.internal.health.is_out_of_memory()
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
        let mut views = self.internal.texture_views.lock().map_err(|_| {
            ResourceError::BackendError(
                "register_texture_view: texture_views mutex poisoned".to_owned(),
            )
        })?;
        views.insert(
            id,
            WgpuTextureViewEntry {
                wgpu_view,
                source_texture_id: None,
            },
        );
        Ok(id)
    }

    /// (crate-internal) Registers a finished wgpu::CommandBuffer, storing it
    /// in a map and returning an abstract ID for it. Returns `None` if the
    /// pending-buffer registry mutex is poisoned (the caller treats this
    /// as a backend failure).
    pub(crate) fn register_command_buffer(
        &self,
        buffer: wgpu::CommandBuffer,
    ) -> Option<api_cmd::CommandBufferId> {
        let new_id_raw = self
            .internal
            .command_buffer_id_counter
            .fetch_add(1, Ordering::SeqCst);
        let new_id = api_cmd::CommandBufferId(new_id_raw);

        let mut guard = match self.internal.pending_command_buffers.lock() {
            Ok(g) => g,
            Err(_) => {
                log::error!(
                    "WgpuDevice::register_command_buffer: pending_command_buffers mutex poisoned"
                );
                return None;
            }
        };
        guard.insert(new_id, buffer);

        Some(new_id)
    }

    pub fn supports_feature(&self, _feature_name: &str) -> bool {
        if self.internal.context.lock().is_err() {
            log::error!("WgpuDevice::supports_feature: context mutex poisoned");
            return false;
        }
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

#[cfg(test)]
mod buffer_descriptor_tests {
    use super::*;

    fn a_descriptor(usage: api_buf::BufferUsage) -> api_buf::BufferDescriptor<'static> {
        api_buf::BufferDescriptor {
            label: Some(std::borrow::Cow::Borrowed("test")),
            size: 256,
            usage,
            mapped_at_creation: false,
        }
    }

    /// **Issue #279.** Reported from the outside against a Metal build: a
    /// vertex buffer reached the driver marked as an index buffer.
    ///
    /// The cause was `wgpu::BufferUsages::from_bits_truncate(usage.bits())`
    /// here, where Khora numbers `VERTEX` at bit 4 and wgpu numbers it at
    /// bit 5. The conversion that does it properly already existed and was
    /// used by `create_buffer_init` fifty lines below; this path simply did
    /// not call it.
    #[test]
    fn a_vertex_buffer_stays_a_vertex_buffer() {
        let source = a_descriptor(api_buf::BufferUsage::VERTEX);
        let built = buffer_descriptor(&source);

        assert_eq!(built.usage, wgpu::BufferUsages::VERTEX);
        assert!(
            !built.usage.contains(wgpu::BufferUsages::INDEX),
            "the exact swap that was reported"
        );
    }

    #[test]
    fn an_index_buffer_stays_an_index_buffer() {
        let source = a_descriptor(api_buf::BufferUsage::INDEX);
        let built = buffer_descriptor(&source);

        assert_eq!(built.usage, wgpu::BufferUsages::INDEX);
        assert!(!built.usage.contains(wgpu::BufferUsages::VERTEX));
    }

    /// **The reporter's case, with their workaround removed.**
    ///
    /// `lana-khora-spike` asked for `VERTEX | INDEX | COPY_DST` on a vertex
    /// buffer so that the raw bit copy would yield wgpu `INDEX | VERTEX`, and
    /// the `VERTEX` flag would survive whichever way the bits landed. Their
    /// comment says to drop the `INDEX` bit once `create_buffer` calls
    /// `into_wgpu`. This is that call, without the bit.
    ///
    /// The engine asks for the same combination in its own text renderer
    /// (`khora-infra/src/renderer/text.rs`), which is why the defect was not
    /// specific to their project — it simply never surfaced there, because
    /// nothing queues text in the sandbox and the buffer is bound only when
    /// something does.
    #[test]
    fn a_vertex_buffer_that_is_also_a_copy_target() {
        let source = a_descriptor(api_buf::BufferUsage::VERTEX | api_buf::BufferUsage::COPY_DST);
        let built = buffer_descriptor(&source);

        assert_eq!(
            built.usage,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            "no INDEX bit needed to keep VERTEX"
        );
    }

    /// The rest of the descriptor is carried across unchanged — a fix that
    /// quietly dropped the label or the mapping flag would trade one silent
    /// failure for another.
    #[test]
    fn the_rest_of_the_descriptor_is_carried_across() {
        let mut source = a_descriptor(api_buf::BufferUsage::UNIFORM);
        source.mapped_at_creation = true;

        let built = buffer_descriptor(&source);

        assert_eq!(built.label, Some("test"));
        assert_eq!(built.size, 256);
        assert!(built.mapped_at_creation);
    }
}
