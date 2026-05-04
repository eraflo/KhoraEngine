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

use crate::platform::window::KhoraWindow;
use crate::renderer::api::{
    core::{GraphicsAdapterInfo, RenderSettings, RenderStats},
    resource::{TextureViewId, ViewInfo},
    scene::RenderObject,
};
use crate::renderer::error::RenderError;
use crate::renderer::GraphicsDevice;
use crate::telemetry::ResourceMonitor;

/// Targets acquired by [`RenderSystem::begin_frame`] for the current frame.
///
/// The engine inserts these into the per-frame `FrameContext` (as
/// `ColorTarget` / `DepthTarget`) so agents can read them when recording
/// passes into the frame graph.
#[derive(Debug, Clone, Copy)]
pub struct FrameTargets {
    /// Color attachment: swapchain texture or offscreen viewport.
    pub color: TextureViewId,
    /// Depth attachment, when depth buffering is enabled.
    pub depth: Option<TextureViewId>,
}

/// A high-level trait representing the entire rendering subsystem.
///
/// This trait defines the primary interface for the engine to interact with the renderer.
/// A concrete implementation of `RenderSystem` (likely in `khora-infra`) encapsulates
/// all the state and logic needed to render a frame, including device management,
/// swapchain handling, and the execution of render pipelines.
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
    fn resize(&mut self, new_width: u32, new_height: u32);

    /// Prepares for a new frame.
    ///
    /// Updates per-frame uniforms (camera view-projection, etc.) before any
    /// pass is recorded.
    fn prepare_frame(&mut self, view_info: &ViewInfo);

    /// Renders a single frame from a flat list of render objects.
    ///
    /// Legacy entry point retained for non-agent code paths (tests, demos).
    /// Production rendering goes through the frame graph.
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
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice>;

    /// Begins a new visual frame by acquiring the swapchain (or viewport) texture.
    ///
    /// Called exactly once per frame by the engine, **before** any agent runs.
    /// The returned [`FrameTargets`] are inserted into the per-frame
    /// `FrameContext` so agents can address the same color/depth attachments.
    /// The matching [`end_frame`](Self::end_frame) presents the result.
    fn begin_frame(&mut self) -> Result<FrameTargets, RenderError>;

    /// Ends the current visual frame by presenting the swapchain texture.
    ///
    /// Called exactly once per frame by the engine, **after** the frame graph
    /// has been compiled and submitted.
    fn end_frame(&mut self) -> Result<RenderStats, RenderError>;

    /// Renders an editor overlay on top of the current frame.
    ///
    /// Called between the frame graph submission and [`end_frame`](Self::end_frame).
    /// The default implementation is a no-op (no overlay).
    fn render_overlay(
        &mut self,
        _overlay: &mut dyn crate::ui::EditorOverlay,
        _screen: crate::ui::OverlayScreenDescriptor,
    ) -> Result<(), RenderError> {
        Ok(())
    }

    /// Cleans up and releases all graphics resources.
    fn shutdown(&mut self);

    /// Allows downcasting to a concrete `RenderSystem` type.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Allows mutable downcasting to a concrete `RenderSystem` type.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Whether [`begin_frame`](Self::begin_frame) targets an offscreen viewport
    /// instead of the swapchain.
    ///
    /// When `true`, the engine skips its own [`end_frame`](Self::end_frame) call
    /// — the caller managing the viewport is responsible for presenting.
    fn render_to_viewport(&self) -> bool {
        false
    }

    /// Toggles whether [`begin_frame`](Self::begin_frame) targets the offscreen
    /// viewport texture instead of the swapchain.
    fn set_render_to_viewport(&mut self, _enabled: bool) {}
}
