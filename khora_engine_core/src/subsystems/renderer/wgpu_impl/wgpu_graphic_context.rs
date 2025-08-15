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

use super::backend_selector::WgpuBackendSelector;
use crate::subsystems::renderer::traits::graphics_backend_selector::{
    BackendSelectionConfig, GraphicsBackendSelector, GraphicsBackendType,
};
use crate::window::KhoraWindow;
use anyhow::Result;
use std::sync::Arc;
use wgpu::{
    CommandEncoder, Features, Instance, InstanceDescriptor, RenderPass, Surface,
    SurfaceCapabilities, SurfaceConfiguration, SurfaceTexture, TextureFormat, TextureView,
};
use winit::dpi::PhysicalSize;

/// Holds the core WGPU state objects required for rendering.
/// This structure manages the connection to the graphics API (WGPU).
#[derive(Debug)]
pub struct WgpuGraphicsContext {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,

    // Configuration for the surface's swapchain behavior
    pub surface_config: wgpu::SurfaceConfiguration,

    pub adapter_name: String,
    pub adapter_backend: wgpu::Backend,
    pub adapter_device_type: wgpu::DeviceType,
    pub active_device_features: wgpu::Features,
    pub device_limits: wgpu::Limits,
}

impl WgpuGraphicsContext {
    /// Initializes the graphics context for rendering.
    /// This function sets up the WGPU instance, surface, adapter, device, and queue.
    /// It also configures the surface swapchain based on the window size and capabilities.
    /// ## Arguments
    /// * `window` - A reference to the `KhoraWindow` object that represents the window where rendering will occur.
    /// ## Returns
    /// * `Result<Self>` - A result containing the initialized `GraphicsContext` or an error.
    pub fn new(window: &KhoraWindow) -> Result<Self> {
        log::info!("Initializing WGPU Graphics Context...");
        pollster::block_on(Self::initialize_async(window))
    }

    /// Asynchronous part of the initialization logic.
    /// ## Arguments
    /// * `window` - A reference to the `KhoraWindow` object that represents the window where rendering will occur.
    /// ## Returns
    /// * `Result<Self>` - A result containing the initialized `GraphicsContext` or an error.
    async fn initialize_async(window: &KhoraWindow) -> Result<Self> {
        let window_arc: Arc<winit::window::Window> = window.winit_window_arc().clone();
        let window_size: PhysicalSize<u32> = window_arc.inner_size();
        log::debug!(
            "Window size for initial graphics setup: {}x{}",
            window_size.width,
            window_size.height
        );

        // --- 1. Create Instance and Surface ---
        let instance = Instance::new(&InstanceDescriptor::default());
        let surface: Surface<'static> = instance.create_surface(window_arc.clone())?;
        log::debug!("WGPU surface created for the window.");

        // --- 2. Robust Backend Selection with Fallback Support ---
        // Use our sophisticated backend selector with preferences
        let backend_selector = WgpuBackendSelector::new(instance.clone());

        let selection_config = BackendSelectionConfig {
            preferred_backends: vec![
                GraphicsBackendType::Vulkan,
                #[cfg(target_os = "windows")]
                GraphicsBackendType::DirectX12,
                #[cfg(target_os = "macos")]
                GraphicsBackendType::Metal,
                GraphicsBackendType::OpenGL, // Fallback
            ],
            timeout: std::time::Duration::from_secs(10),
            prefer_discrete_gpu: true,
        };

        let selection_result = backend_selector
            .select_backend(&selection_config)
            .await
            .map_err(|e| anyhow::anyhow!("Backend selection failed: {}", e))?;

        let adapter = selection_result.adapter;
        let adapter_info = selection_result.adapter_info;

        log::info!(
            "Selected graphics adapter: \"{}\" (Backend: {:?}, Device: {:?}) after {}ms",
            adapter_info.name,
            adapter_info.backend_type,
            if adapter_info.is_discrete {
                "DiscreteGpu"
            } else {
                "IntegratedGpu"
            },
            selection_result.selection_time_ms
        );

        let adapter_name: String = adapter_info.name.clone();
        let adapter_backend_type = adapter_info.backend_type;
        let adapter_is_discrete = adapter_info.is_discrete;

        // Get WGPU-specific info from the actual adapter for internal storage
        let wgpu_adapter_info = adapter.get_info();
        let adapter_backend = wgpu_adapter_info.backend;
        let adapter_device_type = wgpu_adapter_info.device_type;

        log::info!(
            "Final selected GPU: \"{adapter_name}\" (Backend: {adapter_backend_type:?}, Device: {})",
            if adapter_is_discrete {
                "DiscreteGpu"
            } else {
                "IntegratedGpu"
            }
        );

        // --- 3. Create Logical Device and Command Queue ---

        let required_features_for_engine: Features = wgpu::Features::TIMESTAMP_QUERY;
        let adapter_supported_features: Features = adapter.features();
        let features_to_enable: Features =
            adapter_supported_features & required_features_for_engine;

        // Request default limits, then store the actual limits from the device
        let (device, queue): (wgpu::Device, wgpu::Queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Khora Engine Logical Device"),
                required_features: features_to_enable,
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;
        log::info!("Logical device and command queue created.");

        // Debug: log uncaptured validation errors early (to investigate encoder invalidation)
        device.on_uncaptured_error(Box::new(|e| {
            log::error!("WGPU Uncaptured Error: {e:?}");
        }));

        let active_device_features = device.features();
        let device_limits = device.limits();
        log::info!("Active device features: {active_device_features:?}");
        log::info!("Device limits: {device_limits:?}");

        // --- 5. Configure Surface ---
        // Get the surface capabilities (formats, present modes, etc.) for the selected adapter.
        let surface_caps: SurfaceCapabilities = surface.get_capabilities(&adapter);
        let surface_format: TextureFormat = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let surface_config: SurfaceConfiguration = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width.max(1),
            height: window_size.height.max(1),
            present_mode: surface_caps
                .present_modes
                .iter()
                .copied()
                .find(|m| *m == wgpu::PresentMode::Mailbox)
                .unwrap_or(wgpu::PresentMode::Fifo),
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        Ok(WgpuGraphicsContext {
            instance,
            surface,
            adapter,
            device,
            queue,
            surface_config,
            adapter_name,
            adapter_backend,
            adapter_device_type,
            active_device_features,
            device_limits,
        })
    }

    /// Reconfigures the underlying surface (swapchain) when the window is resized.
    ///
    /// This is crucial to ensure the rendered output matches the new window dimensions
    /// and to prevent surface errors (`Lost`, `Outdated`).
    ///
    /// ## Arguments
    /// * `new_width` - The new width of the window.
    /// * `new_height` - The new height of the window.
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

    /// Performs the rendering operations for a single frame.
    /// # Returns
    /// * `Result<(), wgpu::SurfaceError>` -
    ///   - `Ok(())`: Indicates that rendering commands were successfully submitted for the frame.
    ///   - `Err(wgpu::SurfaceError)`: Indicates an error occurred while interacting with the surface,
    ///     such as `Lost`, `Outdated` (requiring reconfiguration), `OutOfMemory` (critical), or `Timeout`.
    pub fn render(&self) -> Result<(), wgpu::SurfaceError> {
        log::trace!("WGPUGraphicsContext::render called");

        // --- 1. Acquire Frame ---
        // Get the next texture from the surface's swapchain to render into.
        let output_frame: SurfaceTexture = self.surface.get_current_texture()?;
        log::trace!("Acquired surface texture frame from swapchain");

        // --- 2. Create Texture View ---
        // Create a view of the output texture. This view is what gets attached
        // to the render pass as the target. Default view covers the whole texture.
        let view: TextureView = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        log::trace!("Created texture view for rendering");

        // --- 3. Create Command Encoder ---
        // Command encoders record GPU commands into a command buffer.
        let mut encoder: CommandEncoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Khora Render Command Encoder"),
                });
        log::trace!("Created command encoder");

        // --- 4. Begin Render Pass ---
        // A render pass defines the attachments (color, depth, stencil targets)
        // and executes drawing commands within its scope.
        {
            let _render_pass: RenderPass<'_> =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Clear Screen Render Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,          // Render directly to the swapchain texture's view
                        depth_slice: None,    // No depth slice for now
                        resolve_target: None, // Used for multisampling; None for now
                        ops: wgpu::Operations {
                            // Action at the start of the pass for this attachment:
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                // Clear the texture
                                r: 0.1,
                                g: 0.2,
                                b: 0.3,
                                a: 1.0,
                            }),
                            // Action at the end of the pass for this attachment:
                            store: wgpu::StoreOp::Store, // Keep the results rendered in the pass
                        },
                    })],
                    depth_stencil_attachment: None, // No depth/stencil buffer yet
                    occlusion_query_set: None,      // No occlusion queries yet
                    // Timestamp writes now handled by dedicated compute passes in WgpuRenderSystem when enabled.
                    timestamp_writes: None,
                });
            log::trace!("Begun render pass (clearing screen)");

            // Drawing commands will be inserted here when renderable pipelines are available.
        } // `_render_pass` is dropped, ending the render pass and releasing `encoder`.
        log::trace!("Render pass finished and recorded.");

        // --- 5. Submit Commands ---
        // Finalize the command buffer recorded by the encoder and submit it
        // to the GPU command queue for execution.
        self.queue.submit(std::iter::once(encoder.finish()));
        log::trace!("Submitted command buffer to GPU queue.");

        // --- 6. Present Frame ---
        // In wgpu 0.20+, presentation to the screen occurs automatically
        // when the `output_frame` (SurfaceTexture) acquired in step 1 is dropped.
        // Explicitly dropping clarifies intent but is not strictly necessary if it goes out of scope.
        drop(output_frame);
        log::trace!("Dropped surface texture frame, implicitly queueing for presentation.");

        Ok(())
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
