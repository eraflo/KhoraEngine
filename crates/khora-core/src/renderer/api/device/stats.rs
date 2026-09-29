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

//! Performance statistics for the rendering system.

/// A collection of performance statistics for a single rendered frame.
#[derive(Debug, Clone)]
pub struct RenderStats {
    /// A sequential counter for rendered frames.
    pub frame_number: u64,
    /// The CPU time spent in pre-render preparation (resource updates, culling, etc.).
    pub cpu_preparation_time_ms: f32,
    /// The CPU time spent submitting encoded command buffers to the GPU.
    pub cpu_render_submission_time_ms: f32,
    /// The GPU execution time of the main render pass, as measured by timestamp queries.
    pub gpu_main_pass_time_ms: f32,
    /// The total GPU execution time for the entire frame, as measured by timestamp queries.
    pub gpu_frame_total_time_ms: f32,
    /// The number of draw calls encoded for the frame.
    pub draw_calls: u32,
    /// The total number of triangles submitted for the frame.
    pub triangles_rendered: u32,
    /// An estimate of the VRAM usage in megabytes.
    pub vram_usage_estimate_mb: f32,
}

impl Default for RenderStats {
    fn default() -> Self {
        Self {
            frame_number: 0,
            cpu_preparation_time_ms: 0.0,
            cpu_render_submission_time_ms: 0.0,
            gpu_main_pass_time_ms: 0.0,
            gpu_frame_total_time_ms: 0.0,
            draw_calls: 0,
            triangles_rendered: 0,
            vram_usage_estimate_mb: 0.0,
        }
    }
}
