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
use std::sync::{Arc, Mutex, OnceLock};
use wgpu;

use entries::{
    WgpuBindGroupEntry, WgpuBindGroupLayoutEntry, WgpuBufferEntry, WgpuComputePipelineEntry,
    WgpuPipelineLayoutEntry, WgpuRenderPipelineEntry, WgpuSamplerEntry, WgpuShaderModuleEntry,
    WgpuTextureEntry, WgpuTextureViewEntry,
};
use khora_core::renderer::api::command as api_cmd;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::api::device::GraphicsAdapterInfo;
use khora_core::renderer::api::pipeline::ComputePipelineId;
use khora_core::renderer::api::pipeline::{PipelineLayoutId, RenderPipelineId};
use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::api::resource::texture::{self as api_tex};
use khora_core::renderer::api::shader::ShaderModuleId;

mod entries;
mod graphics_device;
mod map_async;

#[cfg(test)]
mod tests;

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

use crate::graphics::wgpu::resilience::GpuHealth;

/// Every id table of the device, behind the device's one lock.
///
/// Reached only through [`WgpuDevice::with_registry`], whose closure gets the
/// registry and nothing else: it looks entries up and clones their `Arc`s
/// out, inserts, removes, and returns. It never calls wgpu and never takes
/// another lock, so the registry's lock is a leaf — held by nobody who waits
/// for anything — and no two calls of the device can ever wait on each other.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) shader_modules: HashMap<ShaderModuleId, WgpuShaderModuleEntry>,
    pub(crate) pipelines: HashMap<RenderPipelineId, WgpuRenderPipelineEntry>,
    pub(crate) compute_pipelines: HashMap<ComputePipelineId, WgpuComputePipelineEntry>,
    pub(crate) pipeline_layouts: HashMap<PipelineLayoutId, WgpuPipelineLayoutEntry>,
    pub(crate) buffers: HashMap<api_buf::BufferId, WgpuBufferEntry>,
    pub(crate) textures: HashMap<api_tex::TextureId, WgpuTextureEntry>,
    pub(crate) texture_views: HashMap<api_tex::TextureViewId, WgpuTextureViewEntry>,
    pub(crate) samplers: HashMap<api_tex::SamplerId, WgpuSamplerEntry>,
    pub(crate) bind_group_layouts: HashMap<BindGroupLayoutId, WgpuBindGroupLayoutEntry>,
    pub(crate) bind_groups: HashMap<BindGroupId, WgpuBindGroupEntry>,
    /// Command buffers that have been finished but not yet submitted.
    pub(crate) pending_command_buffers: HashMap<api_cmd::CommandBufferId, wgpu::CommandBuffer>,
}

/// The surface as the device knows it: plain values the render system gives
/// it on every configure.
#[derive(Debug, Clone, Copy, Default)]
struct SurfaceInfo {
    format: Option<khora_core::renderer::api::resource::TextureFormat>,
    size: (u32, u32),
}

/// The internal, non-clonable state of the WgpuDevice.
///
/// wgpu's device and queue are thread-safe, so they are held as they are;
/// the id tables sit in one [`Registry`] behind one lock. Lanes call the
/// device from many threads at once (they prepare and record in concurrent
/// waves), so no call may wait while holding anything.
#[derive(Debug)]
pub struct WgpuDeviceInternal {
    device: wgpu::Device,
    queue: wgpu::Queue,
    registry: Mutex<Registry>,
    /// The surface's format and size; its own leaf, touched only to read or
    /// replace the two values.
    surface: Mutex<SurfaceInfo>,
    /// The adapter the device runs on, described once by the render system.
    adapter: OnceLock<GraphicsAdapterInfo>,

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

    /// A thread-safe counter to generate unique command buffer IDs.
    command_buffer_id_counter: AtomicU64,

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
    /// Builds a device from a wgpu device and its queue, with no surface:
    /// it reports no surface format until [`set_surface_format`] gives it one.
    ///
    /// [`set_surface_format`]: Self::set_surface_format
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, health: Arc<GpuHealth>) -> Self {
        Self {
            internal: Arc::new(WgpuDeviceInternal {
                device,
                queue,
                registry: Mutex::new(Registry::default()),
                surface: Mutex::new(SurfaceInfo::default()),
                adapter: OnceLock::new(),
                health,
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
                command_buffer_id_counter: AtomicU64::new(0),
            }),
        }
    }

    /// Tells the device the surface's format and size, on every configure.
    pub fn set_surface_format(
        &self,
        format: Option<khora_core::renderer::api::resource::TextureFormat>,
        size: (u32, u32),
    ) {
        match self.internal.surface.lock() {
            Ok(mut surface) => *surface = SurfaceInfo { format, size },
            Err(_) => log::error!("WgpuDevice::set_surface_format: surface mutex poisoned"),
        }
    }

    /// Describes the adapter the device runs on; the first description stays.
    pub fn set_adapter_info(&self, info: GraphicsAdapterInfo) {
        let _ = self.internal.adapter.set(info);
    }

    /// The surface the render system last configured.
    fn surface(&self) -> SurfaceInfo {
        match self.internal.surface.lock() {
            Ok(surface) => *surface,
            Err(_) => {
                log::error!("WgpuDevice: surface mutex poisoned");
                SurfaceInfo::default()
            }
        }
    }

    /// The device's one lock. The closure gets the registry and nothing
    /// else: it looks up and clones out, inserts or removes, and returns —
    /// never a wgpu call, never another lock. What it removes it returns, so
    /// the entry is dropped, and wgpu releases it, once the lock is free.
    pub(crate) fn with_registry<R>(
        &self,
        operation: impl FnOnce(&mut Registry) -> R,
    ) -> Result<R, ResourceError> {
        let mut registry = self.internal.registry.lock().map_err(|_| {
            ResourceError::BackendError("the device registry's mutex is poisoned".to_owned())
        })?;
        Ok(operation(&mut registry))
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

    /// The wgpu device: thread-safe, called without any lock.
    pub(crate) fn wgpu_device(&self) -> &wgpu::Device {
        &self.internal.device
    }

    /// The wgpu queue: thread-safe, called without any lock.
    pub(crate) fn wgpu_queue(&self) -> &wgpu::Queue {
        &self.internal.queue
    }

    /// Runs `operation` on the wgpu device; it takes no lock.
    fn with_wgpu_device<F, R>(&self, operation: F) -> Result<R, ResourceError>
    where
        F: FnOnce(&wgpu::Device) -> Result<R, ResourceError>,
    {
        operation(&self.internal.device)
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
        self.with_registry(|r| r.pipelines.get(&id).map(|e| Arc::clone(&e.wgpu_pipeline)))
            .ok()
            .flatten()
    }

    /// Retrieves a reference-counted pointer to the internal WGPU compute pipeline.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_compute_pipeline(
        &self,
        id: ComputePipelineId,
    ) -> Option<Arc<wgpu::ComputePipeline>> {
        self.with_registry(|r| {
            r.compute_pipelines
                .get(&id)
                .map(|e| Arc::clone(&e.wgpu_pipeline))
        })
        .ok()
        .flatten()
    }

    /// Retrieves a reference-counted pointer to the internal WGPU buffer.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_buffer(&self, id: api_buf::BufferId) -> Option<Arc<wgpu::Buffer>> {
        self.with_registry(|r| r.buffers.get(&id).map(|e| Arc::clone(&e.wgpu_buffer)))
            .ok()
            .flatten()
    }

    /// Retrieves a reference-counted pointer to the internal WGPU texture view.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_texture_view(
        &self,
        id: &api_tex::TextureViewId,
    ) -> Option<Arc<wgpu::TextureView>> {
        self.with_registry(|r| r.texture_views.get(id).map(|e| Arc::clone(&e.wgpu_view)))
            .ok()
            .flatten()
    }

    /// Retrieves a reference-counted pointer to the internal WGPU bind group.
    /// Returns `None` if the ID is invalid or the registry mutex is poisoned.
    pub fn get_wgpu_bind_group(&self, id: BindGroupId) -> Option<Arc<wgpu::BindGroup>> {
        self.with_registry(|r| {
            r.bind_groups
                .get(&id)
                .map(|e| Arc::clone(&e.wgpu_bind_group))
        })
        .ok()
        .flatten()
    }

    /// Polls the underlying wgpu::Device in a blocking manner.
    /// This is primarily used during shutdown to ensure all pending operations
    /// and callbacks are completed before resources are destroyed, preventing panics.
    pub fn poll_device_blocking(&self) {
        // PollType::Wait is blocking and will wait for the queue to be empty
        // and for all `on_submitted_work_done` callbacks to be processed.
        if let Err(e) = self.internal.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            log::warn!("Failed to poll device during shutdown: {:?}", e);
        }
    }

    /// Polls the underlying wgpu::Device in a non-blocking manner.
    /// This is essential to process pending `map_async` callbacks from the GPU,
    /// allowing systems like the GpuProfiler to receive data.
    pub fn poll_device_non_blocking(&self) {
        // PollType::Poll is non-blocking. It processes any completed work
        // but returns immediately if there is none.
        if let Err(e) = self.internal.device.poll(wgpu::PollType::Poll) {
            log::warn!("Failed to poll device (non-blocking): {:?}", e);
        }
    }

    /// Waits for every `queue.submit()` so far to be processed by the GPU.
    /// Must be called before `get_current_texture()` to avoid Vulkan semaphore
    /// validation errors (VUID-vkAcquireNextImageKHR-semaphore-01286).
    pub fn wait_for_last_submission(&self) {
        // Every submission so far, not one remembered index: submissions come
        // from many threads, and on one queue waiting for all of them is
        // waiting for the latest.
        if let Err(e) = self.internal.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            log::warn!("WgpuDevice::wait_for_last_submission: device poll failed: {e:?}");
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
        self.with_registry(|r| {
            r.texture_views.insert(
                id,
                WgpuTextureViewEntry {
                    wgpu_view,
                    source_texture_id: None,
                },
            )
        })?;
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

        match self.with_registry(|r| r.pending_command_buffers.insert(new_id, buffer)) {
            Ok(_) => Some(new_id),
            Err(e) => {
                log::error!("WgpuDevice::register_command_buffer: {e}");
                None
            }
        }
    }

    pub(crate) fn get_wgpu_texture_view_entry(
        &self,
        id: api_tex::TextureViewId,
    ) -> Option<WgpuTextureViewEntry> {
        self.with_registry(|r| r.texture_views.get(&id).cloned())
            .ok()
            .flatten()
    }

    pub(crate) fn get_wgpu_texture(&self, id: api_tex::TextureId) -> Option<Arc<wgpu::Texture>> {
        self.with_registry(|r| r.textures.get(&id).map(|e| Arc::clone(&e.wgpu_texture)))
            .ok()
            .flatten()
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
