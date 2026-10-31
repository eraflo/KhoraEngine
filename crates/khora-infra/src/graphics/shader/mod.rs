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

//! The engine's WGSL sources, under `shaders/`: `lib/` modules and
//! `pipelines/` entry points, composed by the wgpu `PipelineSystem` backend
//! ([`WgpuPipelineSystem`](crate::graphics::wgpu::WgpuPipelineSystem)) and
//! looked up by name.
//!
//! One shader is handed over as a raw string instead, because its consumer,
//! the egui editor overlay, takes WGSL source rather than a pipeline handle.
//! It is a whole shader, not composed, so it sits in `standalone/` rather
//! than `pipelines/`. It is the only `_WGSL` constant; a new pipeline goes
//! through the backend's table.

/// Shader for the egui editor overlay. Consumed by
/// `WgpuRenderSystem::create_editor_overlay_and_shell` as a raw `&str`.
pub const EGUI_WGSL: &str = include_str!("shaders/standalone/egui.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_shader_valid() {
        assert!(EGUI_WGSL.contains("@vertex"));
        assert!(EGUI_WGSL.contains("@fragment"));
    }
}
