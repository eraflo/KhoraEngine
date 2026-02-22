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

//! Global settings for the rendering system.

use crate::renderer::api::util::enums::RenderStrategy;

/// A collection of global settings that can affect the rendering process.
#[derive(Debug, Clone)]
pub struct RenderSettings {
    /// The desired high-level rendering strategy.
    pub strategy: RenderStrategy,
    /// A generic quality level (e.g., 1=Low, 2=Medium, 3=High).
    pub quality_level: u32,
    /// If `true`, objects should be rendered in wireframe mode.
    pub show_wireframe: bool,
    /// The quiet period in milliseconds after a resize event before the surface is reconfigured.
    pub resize_debounce_ms: u64,
    /// A fallback number of frames after which a pending resize is forced, even if events are still incoming.
    pub resize_max_pending_frames: u32,
    /// A runtime toggle to enable/disable GPU timestamp instrumentation for profiling.
    pub enable_gpu_timestamps: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            strategy: RenderStrategy::Forward,
            quality_level: 1,
            show_wireframe: false,
            resize_debounce_ms: 120,
            resize_max_pending_frames: 10,
            enable_gpu_timestamps: true,
        }
    }
}
