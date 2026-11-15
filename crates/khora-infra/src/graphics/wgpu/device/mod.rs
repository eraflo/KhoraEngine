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

mod bind_groups;
mod buffers;
mod entries;
mod graphics_device;
mod map_async;
mod pipelines;
mod textures;
pub mod vram_probe;
mod wgpu_bind_group_entry;
mod wgpu_bind_group_layout_entry;
mod wgpu_buffer_entry;
mod wgpu_compute_pipeline_entry;
mod wgpu_pipeline_layout_entry;
mod wgpu_render_pipeline_entry;
mod wgpu_sampler_entry;
mod wgpu_texture_entry;
mod wgpu_texture_view_entry;

/// Builds the wgpu descriptor for an empty buffer.
///
/// A function of its own so a test checks, with no GPU, that the usage crosses
/// through `IntoWgpu` and never as raw bits (`a_vertex_buffer_stays_a_vertex_buffer`).
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
/// Reached only through [`WgpuDevice::with_registry`]. Its lock is a leaf: held
/// only to look up, insert or remove, never across a wgpu call or another lock.
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

/// The shared state behind a [`WgpuDevice`].
///
/// Lanes call the device from many threads at once, so no call waits while it
/// holds a lock: wgpu's device and queue are thread-safe, the id tables sit in
/// one leaf [`Registry`].
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
    /// The wgpu adapter itself, which the VRAM report asks the platform's
    /// budget through; given once by the render system.
    adapter_handle: OnceLock<wgpu::Adapter>,

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
    /// The most video memory a report has said in use: the platform's count
    /// included, which sees more than Khora allocates.
    reported_peak_bytes: AtomicU64,

    /// A thread-safe counter to generate unique command buffer IDs.
    command_buffer_id_counter: AtomicU64,

    /// Device-health flags shared with the graphics context, raised by the
    /// wgpu error callbacks.
    health: Arc<super::resilience::GpuHealth>,
}

/// A clonable, thread-safe handle to the wgpu graphics device; clones share
/// one [`WgpuDeviceInternal`].
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
                adapter_handle: OnceLock::new(),
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
                reported_peak_bytes: AtomicU64::new(0),
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

    /// Gives the device the wgpu adapter it runs on, which its VRAM report
    /// reads the platform's budget through; the first adapter stays.
    pub fn set_adapter_handle(&self, adapter: wgpu::Adapter) {
        let _ = self.internal.adapter_handle.set(adapter);
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

    /// Runs `operation` on the registry, under the device's one lock.
    ///
    /// The closure looks up and clones out, inserts or removes, and returns:
    /// never a wgpu call, never another lock. What it removes it returns, so
    /// wgpu releases the entry once the lock is free.
    ///
    /// # Errors
    ///
    /// Fails when the registry's mutex is poisoned.
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

    /// Returns a texture's size in bytes, ignoring mips, block compression and padding.
    fn calculate_texture_size_in_bytes(descriptor: &api_tex::TextureDescriptor) -> u64 {
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

    /// Blocks until the GPU has done all submitted work and run its callbacks.
    ///
    /// Called at shutdown, so no resource is destroyed under a pending callback.
    pub fn poll_device_blocking(&self) {
        if let Err(e) = self.internal.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            log::warn!("Failed to poll device during shutdown: {:?}", e);
        }
    }

    /// Runs the callbacks of finished GPU work, such as `map_async`, without blocking.
    pub fn poll_device_non_blocking(&self) {
        if let Err(e) = self.internal.device.poll(wgpu::PollType::Poll) {
            log::warn!("Failed to poll device (non-blocking): {:?}", e);
        }
    }

    /// Waits until the GPU has processed every submission so far.
    ///
    /// Call it before `get_current_texture()`: acquiring sooner trips Vulkan's
    /// VUID-vkAcquireNextImageKHR-semaphore-01286.
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

    /// Returns `true` once the device was reported lost or hit an internal error.
    ///
    /// See the book: concepts/rendering.md, "Device-loss resilience".
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

    /// Keeps a finished command buffer until it is submitted, and returns its id.
    ///
    /// Returns `None` when the registry's mutex is poisoned.
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
        // One device per process, so a fixed name is unique.
        Cow::Borrowed("WgpuDevice_VRAM_Monitor")
    }

    fn resource_type(&self) -> MonitoredResourceType {
        MonitoredResourceType::Vram
    }

    /// Reports what the device holds and, once it knows its adapter, the
    /// platform's budget as the capacity.
    ///
    /// The platform's count of what is used includes what Khora does not
    /// allocate (the swapchain, the driver).
    /// See the book: concepts/telemetry.md, "Video memory".
    fn get_usage_report(&self) -> ResourceUsageReport {
        let own = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        let reading = self
            .internal
            .adapter_handle
            .get()
            .and_then(|adapter| vram_probe::probe(Some(adapter), &self.internal.device));
        let current = reading.and_then(|reading| reading.usage).unwrap_or(own);
        // The peak of what was reported, never below what is reported now:
        // the platform's count runs above Khora's own peak.
        let reported = self
            .internal
            .reported_peak_bytes
            .fetch_max(current, Ordering::Relaxed)
            .max(current);
        let peak = reported.max(self.internal.vram_peak_bytes.load(Ordering::Relaxed));
        ResourceUsageReport {
            current_bytes: current,
            peak_bytes: Some(peak),
            total_capacity_bytes: reading.map(|reading| reading.budget),
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
        // The platform's budget reaches telemetry through `get_usage_report`.
        None
    }
}

#[cfg(test)]
mod tests;
