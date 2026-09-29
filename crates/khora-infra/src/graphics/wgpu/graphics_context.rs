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

use anyhow::anyhow;
use anyhow::Result;
use khora_core::platform::window::KhoraWindowHandle;
use std::sync::Arc;
use wgpu::SurfaceTargetUnsafe;
use wgpu::{Adapter, Features, Instance};
use winit::dpi::PhysicalSize;

use super::resilience::GpuHealth;

/// Holds the core WGPU state objects required for rendering.
/// This structure manages the connection to the graphics API for a specific surface.
/// It is initialized with a pre-selected adapter, making it a passive component.
#[derive(Debug)]
pub struct WgpuGraphicsContext {
    pub surface: wgpu::Surface<'static>,
    #[allow(dead_code)]
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,

    // Configuration for the surface's swapchain behavior
    pub surface_config: wgpu::SurfaceConfiguration,

    // Store info for easy access
    pub adapter_name: String,
    pub adapter_backend: wgpu::Backend,
    pub adapter_device_type: wgpu::DeviceType,
    pub active_device_features: wgpu::Features,
    #[allow(dead_code)]
    pub device_limits: wgpu::Limits,

    /// Lock-free device-health flags raised by the wgpu error callbacks
    /// (uncaptured-error and device-lost). The render system reads these once
    /// per frame to decide whether the device is still usable. Shared by Arc so
    /// the callback closures can outlive any single borrow of the context.
    pub(crate) health: Arc<GpuHealth>,
}

impl WgpuGraphicsContext {
    /// Asynchronously initializes the graphics context for a given window surface.
    ///
    /// ## Arguments
    /// * `instance` - A reference to the shared `wgpu::Instance`.
    /// * `window` - A reference to any object that can provide a raw window handle.
    /// * `adapter` - The pre-selected `wgpu::Adapter` to use.
    /// * `window_size` - The initial physical size of the window surface.
    ///
    /// ## Returns
    /// * `Result<Self>` - A result containing the initialized `WgpuGraphicsContext` or an error.
    pub async fn new(
        instance: &Instance,
        window_handle: KhoraWindowHandle,
        adapter: Adapter,
        window_size: PhysicalSize<u32>,
    ) -> Result<Self> {
        log::info!("Initializing WGPU Graphics Context with pre-selected adapter...");

        // --- 1. Create Surface ---
        // SAFETY: `from_window` reads the raw window/display handles out of
        // `window_handle`. `KhoraWindowHandle` is an
        // `Arc<dyn WindowHandle + Send + Sync + 'static>`, so the underlying
        // window outlives the borrow taken here and the handles it exposes are
        // valid for the call. We pass a borrow, so ownership is unaffected.
        let surface_target = unsafe {
            SurfaceTargetUnsafe::from_window(&window_handle)
                .map_err(|e| anyhow!("Failed to create surface target: {}", e))?
        };

        // SAFETY: `surface_target` carries raw handles whose validity wgpu
        // cannot verify, which is why this is the `_unsafe` variant. The
        // resulting `Surface<'static>` must not outlive the window: callers
        // keep the same `KhoraWindowHandle` Arc alive for the whole lifetime of
        // the render system (the instance was built from a clone of it, and the
        // window is dropped only at shutdown after the surface), upholding the
        // `'static` contract.
        let surface = unsafe { instance.create_surface_unsafe(surface_target)? };
        log::debug!("WGPU surface created for the window.");

        let adapter_info = adapter.get_info();
        log::info!(
            "Using provided graphics adapter: \"{}\" (Backend: {:?})",
            adapter_info.name,
            adapter_info.backend
        );

        // --- 2. Create Logical Device and Command Queue from Adapter ---
        let required_features_for_engine: Features = wgpu::Features::TIMESTAMP_QUERY;
        let features_to_enable: Features = adapter.features() & required_features_for_engine;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Khora Engine Logical Device"),
                required_features: features_to_enable,
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
            })
            .await
            .map_err(|e| anyhow!("Failed to create logical device: {}", e))?;
        log::info!("Logical device and command queue created.");

        // Shared health flags: the error/device-lost callbacks run on wgpu's
        // own threads and must not hold a borrow of the context, so they
        // capture an `Arc<GpuHealth>` clone. The render system reads the same
        // flags each frame to decide whether the device is still usable.
        let health = Arc::new(GpuHealth::default());

        let uncaptured_health = Arc::clone(&health);
        device.on_uncaptured_error(std::sync::Arc::new(move |e| {
            // Out-of-memory is fatal and non-recoverable in place; validation
            // and internal errors are logged but only an OOM forces teardown.
            match &e {
                wgpu::Error::OutOfMemory { .. } => {
                    log::error!("WGPU uncaptured out-of-memory error: {e:?}");
                    uncaptured_health.mark_out_of_memory();
                }
                wgpu::Error::Internal { .. } => {
                    log::error!("WGPU uncaptured internal device error: {e:?}");
                    uncaptured_health.mark_device_lost();
                }
                wgpu::Error::Validation { .. } => {
                    log::error!("WGPU uncaptured validation error: {e:?}");
                }
            }
        }));

        // Device-lost callback: a driver crash/reset/destroy surfaces here. We
        // raise the sticky `device_lost` flag so the next frame stops
        // submitting and reports a fatal, non-panicking error to the host.
        let lost_health = Arc::clone(&health);
        device.set_device_lost_callback(move |reason, message| {
            log::error!("WGPU device lost ({reason:?}): {message}");
            lost_health.mark_device_lost();
        });

        let active_device_features = device.features();
        let device_limits = device.limits();
        log::info!("Active device features: {active_device_features:?}");
        log::info!("Device limits: {device_limits:?}");

        // --- 3. Configure Surface ---
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width.max(1),
            height: window_size.height.max(1),
            present_mode: surface_caps
                .present_modes
                .iter()
                .copied()
                .find(|m| *m == wgpu::PresentMode::Mailbox)
                .unwrap_or(wgpu::PresentMode::Fifo), // Fifo is guaranteed to be supported
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        Ok(WgpuGraphicsContext {
            surface,
            adapter,
            device,
            queue,
            surface_config,
            adapter_name: adapter_info.name,
            adapter_backend: adapter_info.backend,
            adapter_device_type: adapter_info.device_type,
            active_device_features,
            device_limits,
            health,
        })
    }

    /// Reconfigures the underlying surface (swapchain) when the window is resized.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width > 0 && new_height > 0 {
            log::info!(
                "WGPUGraphicsContext: Resizing surface configuration to {new_width}x{new_height}"
            );
            self.surface_config.width = new_width;
            self.surface_config.height = new_height;
            self.surface.configure(&self.device, &self.surface_config);
        } else {
            log::warn!(
                "WGPUGraphicsContext: Ignoring resize request to zero dimensions: {new_width}x{new_height}"
            );
        }
    }

    /// Returns the current surface texture for rendering.
    /// This is useful for obtaining the texture to render into.
    ///
    /// ## Returns
    /// * `wgpu::CurrentSurfaceTexture` - The result of acquiring the next swapchain frame.
    ///   See [`wgpu::CurrentSurfaceTexture`] for how each variant should be handled.
    pub fn get_current_texture(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
    }

    #[allow(dead_code)]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    #[allow(dead_code)]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    #[allow(dead_code)]
    pub fn surface_configuration(&self) -> &wgpu::SurfaceConfiguration {
        &self.surface_config
    }

    /// Returns the clear color used for rendering.
    /// This is the color used to clear the screen before rendering.
    ///
    /// ## Returns
    /// * `wgpu::Color` - The clear color used for rendering.
    pub fn get_clear_color(&self) -> wgpu::Color {
        wgpu::Color {
            r: 0.01,
            g: 0.02,
            b: 0.03,
            a: 1.0,
        }
    }

    /// Returns the size of the surface configuration.
    /// This is the size of the swapchain surface used for rendering.
    ///
    /// ## Returns
    /// * `(u32, u32)` - A tuple containing the width and height of the surface configuration.
    pub fn get_size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }
}
