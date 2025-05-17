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

// Public API of the rendering subsystem
pub mod api;
pub mod error;
pub mod traits;

// Specific WGPU implementation
pub mod wgpu_impl;

// --- Re-exports for convenient use by the rest of the engine ---

// Core traits
pub use traits::graphics_device::GraphicsDevice;
pub use traits::render_system::RenderSystem;

// Concrete WGPU implementation of RenderSystem
pub use wgpu_impl::WgpuRenderSystem;

// Common API data types (errors, info structs, render data)
pub use api::common_types::{
    RenderObject, RenderSettings, RenderStats, RendererAdapterInfo, RendererBackendType,
    RendererDeviceType, ShaderStage, ViewInfo,
};
pub use error::{RenderError, ResourceError, ShaderError};
