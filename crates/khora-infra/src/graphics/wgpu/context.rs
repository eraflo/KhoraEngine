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
use wgpu::SurfaceTargetUnsafe;
use wgpu::{Adapter, Features, Instance};
use winit::dpi::PhysicalSize;

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
        let surface_target = unsafe {
            SurfaceTargetUnsafe::from_window(&window_handle)
                .map_err(|e| anyhow!("Failed to create surface target: {}", e))?
        };

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
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
            })
            .await
            .map_err(|e| anyhow!("Failed to create logical device: {}", e))?;
        log::info!("Logical device and command queue created.");

        device.on_uncaptured_error(Box::new(|e| {
            log::error!("WGPU Uncaptured Error: {e:?}");
        }));

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
    /// * `Result<wgpu::SurfaceTexture, wgpu::SurfaceError>` -
    ///   - `Ok(wgpu::SurfaceTexture)`: The current surface texture for rendering.
    ///  - `Err(wgpu::SurfaceError)`: An error occurred while acquiring the texture,
    pub fn get_current_texture(&self) -> Result<wgpu::SurfaceTexture, wgpu::SurfaceError> {
        self.surface.get_current_texture()
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

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
