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

use std::sync::Arc;

use super::CommandEncoder;
use crate::platform::window::KhoraWindow;
use crate::renderer::api::{
    core::{GraphicsAdapterInfo, RenderContext, RenderSettings, RenderStats},
    resource::ViewInfo,
    scene::RenderObject,
};
use crate::renderer::error::RenderError;
use crate::renderer::GraphicsDevice;
use crate::telemetry::ResourceMonitor;

/// Type alias for the render encoder closure.
pub type RenderEncoderFn<'a> = Box<dyn FnOnce(&mut dyn CommandEncoder, &RenderContext) + Send + 'a>;

/// A high-level trait representing the entire rendering subsystem.
///
/// This trait defines the primary interface for the engine to interact with the renderer.
/// A concrete implementation of `RenderSystem` (likely in `khora-infra`) encapsulates
/// all the state and logic needed to render a frame, including device management,
/// swapchain handling, and the execution of render pipelines.
///
/// This can be considered the main "contract" for a `RenderAgent` to manage.
pub trait RenderSystem: std::fmt::Debug + Send + Sync {
    /// Initializes the rendering system with a given window.
    ///
    /// This method sets up the graphics device, swapchain, and any other necessary
    /// backend resources. It should be called once at application startup.
    ///
    /// # Returns
    ///
    /// On success, it returns a `Vec` of `ResourceMonitor` trait objects that the
    /// telemetry system can use to track GPU-specific resources like VRAM.
    fn init(
        &mut self,
        window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError>;

    /// Notifies the rendering system that the output window has been resized.
    ///
    /// The implementation should handle recreating the swapchain and any other
    /// size-dependent resources (like depth textures).
    fn resize(&mut self, new_width: u32, new_height: u32);

    /// Prepares for a new frame.
    ///
    /// This is typically called once per frame before `render`. It can be used
    /// to update internal resources, like uniform buffers, based on the camera's
    /// `ViewInfo`.
    fn prepare_frame(&mut self, view_info: &ViewInfo);

    /// Renders a single frame using agent-driven encoding.
    ///
    /// This method acquires the frame, creates an encoder, calls the provided
    /// closure to encode commands (typically from RenderAgent), and submits.
    ///
    /// # Arguments
    ///
    /// * `clear_color`: The color to clear the framebuffer with
    /// * `encoder_fn`: A boxed closure that encodes GPU commands
    ///
    /// # Returns
    ///
    /// On success, it returns `RenderStats` with performance metrics for the frame.
    fn render_with_encoder(
        &mut self,
        clear_color: crate::math::LinearRgba,
        encoder_fn: RenderEncoderFn<'_>,
    ) -> Result<RenderStats, RenderError>;

    /// Renders a single frame.
    ///
    /// This is the main workload function, responsible for executing all necessary
    /// render passes to draw the provided `renderables` to the screen.
    ///
    /// # Arguments
    ///
    /// * `renderables`: A slice of `RenderObject`s representing the scene to be drawn.
    /// * `view_info`: Contains camera and projection information for the frame.
    /// * `settings`: Contains global rendering settings for the frame.
    ///
    /// # Returns
    ///
    /// On success, it returns `RenderStats` with performance metrics for the frame.
    fn render(
        &mut self,
        renderables: &[RenderObject],
        view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError>;

    /// Returns a reference to the statistics of the last successfully rendered frame.
    fn get_last_frame_stats(&self) -> &RenderStats;

    /// Checks if a specific, optional rendering feature is supported by the backend.
    fn supports_feature(&self, feature_name: &str) -> bool;

    /// Returns information about the active graphics adapter (GPU).
    fn get_adapter_info(&self) -> Option<GraphicsAdapterInfo>;

    /// Returns a shared, thread-safe reference to the underlying `GraphicsDevice`.
    ///
    /// This allows other parts of the engine (e.g., the asset system) to create
    /// graphics resources like buffers and textures on the correct GPU device.
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice>;

    /// Cleans up and releases all graphics resources.
    /// This should be called once at application shutdown.
    fn shutdown(&mut self);

    /// Allows downcasting to a concrete `RenderSystem` type.
    fn as_any(&self) -> &dyn std::any::Any;
}
