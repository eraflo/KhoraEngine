
use crate::window::KhoraWindow;
use wgpu::SurfaceTarget;
use winit::dpi::PhysicalSize;
use std::sync::Arc;
use anyhow::Result;

/// Holds the core WGPU state objects required for rendering.
/// This structure manages the connection to the graphics API (WGPU).
#[derive(Debug)]
pub struct GraphicsContext {
    // Core WGPU objects representing the API state
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,

    // Configuration for the surface's swapchain behavior
    pub surface_config: wgpu::SurfaceConfiguration,

    // SAA Requirement: Track if GPU timing is supported for performance monitoring.
    // This allows the engine (or future DCC) to know if precise GPU metrics are available.
    pub supports_gpu_timestamps: bool,
}

impl GraphicsContext {
    
    /// Initializes the graphics context for rendering.
    /// This function sets up the WGPU instance, surface, adapter, device, and queue.
    /// It also configures the surface swapchain based on the window size and capabilities.
    /// ## Arguments
    /// * `window` - A reference to the `KhoraWindow` object that represents the window
    ///  where rendering will occur.
    /// ## Returns
    /// * `Result<Self>` - A result containing the initialized `GraphicsContext` or an error.
    pub fn new(window: &KhoraWindow) -> Result<Self> {
        log::info!("Initializing Graphics Context...");
        pollster::block_on(Self::initialize_async(window))
    }

    /// Asynchronous part of the initialization logic.
    /// ## Arguments
    /// * `window` - A reference to the `KhoraWindow` object that represents the window
    /// where rendering will occur.
    /// ## Returns
    /// * `Result<Self>` - A result containing the initialized `GraphicsContext` or an error.
    async fn initialize_async(window: &KhoraWindow) -> Result<Self> {
        let window_arc: Arc<winit::window::Window> = window.winit_window_arc().clone();
        let window_size = window_arc.inner_size();
        log::debug!("Window size for initial graphics setup: {}x{}", window_size.width, window_size.height);

        // --- 1. Create WGPU Instance ---
        // The instance is the entry point to the WGPU API.
        let instance_descriptor = wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::GL, // Use the primary backend (Vulkan, Metal, DX12, etc.)
                    ..Default::default()
                };
        let instance = wgpu::Instance::new(&instance_descriptor);
        log::debug!("WGPU instance created.");

        // --- 2. Create Surface ---
        // The surface represents the target window or canvas WGPU will draw to.
        let surface = instance.create_surface(window_arc.clone())?;
        log::debug!("WGPU surface created for the window.");

        // --- 3. Select Adapter (Physical GPU) ---
        // Request an adapter (GPU) that is compatible with the surface and prefers high performance.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface), // Must be able to render to our surface
                force_fallback_adapter: false, // Don't fallback to software rendering if possible
            })
            .await?;

        let adapter_info = adapter.get_info();
        log::info!(
            "Selected GPU: \"{}\", Backend: {:?}",
            adapter_info.name,
            adapter_info.backend
        );

        // --- 4. Request Device and Queue (Logical GPU Connection) ---

        // -- SAA Focus: Check for and request necessary features --
        // Define features critical for SAA or desired functionality.
        let required_features = wgpu::Features::TIMESTAMP_QUERY; // For GPU performance timings
        let adapter_features = adapter.features(); // Features the physical adapter supports
        log::debug!("Adapter supported features: {:?}", adapter_features);

        // Check specifically for timestamp support.
        let supports_gpu_timestamps = adapter_features.contains(required_features);
        if supports_gpu_timestamps {
            log::info!("Adapter supports TIMESTAMP_QUERY feature for GPU profiling.");
        } else {
            // NOTE (future): The engine needs to be aware that TIMESTAMP_QUERY is not supported on this adapter.
            log::warn!("Adapter does NOT support TIMESTAMP_QUERY. GPU performance monitoring will be unavailable.");
        }
        // Calculate the intersection: Only enable features that are BOTH required AND supported.
        let features_to_enable = adapter_features & required_features;


        // Request the logical device (our interface to the GPU) and command queue.
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Khora Engine Logical Device"),
                    required_features: features_to_enable,
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                }
            )
            .await?;
        log::info!("Logical device and command queue created.");

        // Sanity check if the feature was actually enabled on the device
        if supports_gpu_timestamps && !features_to_enable.contains(wgpu::Features::TIMESTAMP_QUERY) {
            log::error!("Timestamp query feature supported by adapter but FAILED to enable on logical device!");
        }

        // --- 5. Configure Surface Swapchain ---
        // Get the surface's capabilities (supported formats, present modes, etc.)
        let surface_caps = surface.get_capabilities(&adapter);

        // Select a texture format for the surface, preferring sRGB for better color representation.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb()) 
            .unwrap_or(surface_caps.formats[0]);

        // Define the configuration for the surface swapchain.
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, // Textures will be used as render targets
            format: surface_format, // The sRGB or fallback format chosen above
            width: window_size.width.max(1),   // Ensure width is at least 1 (required by WGPU)
            height: window_size.height.max(1), // Ensure height is at least 1
            present_mode: surface_caps
                .present_modes // Use modes supported by the surface/adapter combo
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::PresentMode::Mailbox) // Prefer Mailbox (low latency vsync)
                .unwrap_or(wgpu::PresentMode::Fifo), // Default to Fifo (standard vsync)
            alpha_mode: surface_caps.alpha_modes[0], // Usually Opaque, or Premultiplied if needed
            view_formats: vec![], // Additional formats textures can be viewed as (for advanced techniques)
            desired_maximum_frame_latency: 2, // Default value for frame buffering
        };
        // Apply this configuration to the surface.
        surface.configure(&device, &surface_config);

        log::info!(
            "Surface configured: Format={:?}, Size={}x{}, PresentMode={:?}",
            surface_format,
            surface_config.width,
            surface_config.height,
            surface_config.present_mode
        );

        // Initialization successful, return the created context.
        Ok(GraphicsContext {
            instance,
            surface,
            adapter,
            device,
            queue,
            surface_config,
            supports_gpu_timestamps: features_to_enable.contains(wgpu::Features::TIMESTAMP_QUERY),
        })
    }

    /// Reconfigures the underlying surface (swapchain) when the window is resized.
    ///
    /// This is crucial to ensure the rendered output matches the new window dimensions
    /// and to prevent surface errors (`Lost`, `Outdated`).
    ///
    /// ## Arguments
    /// * `new_size` - The new physical size of the window (width and height in pixels)
    ///                provided by the `winit` resize event.
    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        // Validate the new size, as configuring with zero dimensions is invalid.
        if new_size.width > 0 && new_size.height > 0 {
            log::info!(
                "Resizing graphics surface configuration to {}x{}",
                new_size.width,
                new_size.height
            );
            
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;

            // Apply the updated configuration to the WGPU surface object.
            self.surface.configure(&self.device, &self.surface_config);
        } else {
            log::warn!(
                "Ignoring resize request to zero dimensions: {}x{}",
                new_size.width,
                new_size.height
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
        log::trace!("GraphicsContext::render called");

        // --- 1. Acquire Frame ---
        // Get the next texture from the surface's swapchain to render into.
        let output_frame: wgpu::SurfaceTexture = self.surface.get_current_texture()?;
        log::trace!("Acquired surface texture frame from swapchain");

        // --- 2. Create Texture View ---
        // Create a view of the output texture. This view is what gets attached
        // to the render pass as the target. Default view covers the whole texture.
        let view = output_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        log::trace!("Created texture view for rendering");

        // --- 3. Create Command Encoder ---
        // Command encoders record GPU commands into a command buffer.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Khora Render Command Encoder")
            });
        log::trace!("Created command encoder");

        // --- 4. Begin Render Pass ---
        // A render pass defines the attachments (color, depth, stencil targets)
        // and executes drawing commands within its scope.
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear Screen Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view, // Render directly to the swapchain texture's view
                    resolve_target: None, // Used for multisampling; None for now
                    ops: wgpu::Operations {
                        // Action at the start of the pass for this attachment:
                        load: wgpu::LoadOp::Clear(wgpu::Color { // Clear the texture
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
                occlusion_query_set: None, // No occlusion queries yet
                // TODO: SAA Requirement: Add timestamp writes here in the dedicated task later
                // Needs `Features::TIMESTAMP_QUERY` enabled on the device.
                timestamp_writes: None,
            });
            log::trace!("Begun render pass (clearing screen)");

            // TODO: Implement actual drawing commands (e.g., set_pipeline, draw)

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
        wgpu::Color { r: 0.01, g: 0.02, b: 0.03, a: 1.0 }
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