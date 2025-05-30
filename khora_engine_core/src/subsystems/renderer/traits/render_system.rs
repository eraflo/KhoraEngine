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

use super::graphics_device::GraphicsDevice;
use crate::subsystems::renderer::api::common_types::{
    RenderObject, RenderSettings, RenderStats, RendererAdapterInfo, ViewInfo,
};
use crate::subsystems::renderer::error::RenderError;
use crate::window::KhoraWindow;

/// Trait representing a render system.
/// This trait defines the methods that a render system must implement.
pub trait RenderSystem: std::fmt::Debug + Send + Sync {
    /// Initialize the rendering system.
    /// This method is called once at the beginning of the application.
    fn init(&mut self, window: &KhoraWindow) -> Result<(), RenderError>;

    /// Resize the window of the render system.
    fn resize(&mut self, new_width: u32, new_height: u32);

    /// Prepare the frame for rendering.
    /// This method is called before the actual rendering process.
    fn prepare_frame(&mut self, view_info: &ViewInfo);

    /// Render the frame to the window.
    fn render(
        &mut self,
        renderables: &[RenderObject],
        view_info: &ViewInfo,
        settings: &RenderSettings,
    ) -> Result<RenderStats, RenderError>;

    /// Get the stats of the last rendered frame.
    fn get_last_frame_stats(&self) -> &RenderStats;

    /// Indicate if a specific feature is supported.
    fn supports_feature(&self, feature_name: &str) -> bool;

    /// Get the adapter information of the rendering system.
    fn get_adapter_info(&self) -> Option<RendererAdapterInfo>;

    /// Returns a reference to the underlying `GraphicsDevice` used by this `RenderSystem`.
    /// This allows other parts of the engine (e.g., asset managers) to create
    /// graphics resources like buffers, textures, shaders, and pipelines.
    fn graphics_device(&self) -> &dyn GraphicsDevice;

    /// Clean up and release the resources of the rendering system.
    fn shutdown(&mut self);
}
