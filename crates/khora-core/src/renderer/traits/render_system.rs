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
use crate::renderer::error::RenderError;
use crate::renderer::{api::*, GraphicsDevice};
use crate::telemetry::ResourceMonitor;

/// Trait representing a render system.
/// This trait defines the methods that a render system must implement.
pub trait RenderSystem: std::fmt::Debug + Send + Sync {
    /// Initialize the rendering system.
    /// Returns a list of resource monitors it has created.
    fn init(
        &mut self,
        window: &dyn KhoraWindow,
    ) -> Result<Vec<Arc<dyn ResourceMonitor>>, RenderError>;

    /// Resize the window of the render system.
    fn resize(&mut self, new_width: u32, new_height: u32);

    /// Prepare the frame for rendering.
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

    /// Returns a Arc to the underlying `GraphicsDevice` used by this `RenderSystem`.
    /// This allows other parts of the engine (e.g., asset managers) to create
    /// graphics resources like buffers, textures, shaders, and pipelines.
    fn graphics_device(&self) -> Arc<dyn GraphicsDevice>;

    /// Clean up and release the resources of the rendering system.
    fn shutdown(&mut self);

    /// Downcast to Any for type-specific access
    fn as_any(&self) -> &dyn std::any::Any;
}
