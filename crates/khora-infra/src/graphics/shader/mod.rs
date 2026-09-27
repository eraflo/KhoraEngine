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
//! Two shaders are handed over as raw strings instead, because their
//! consumers take WGSL source rather than a pipeline handle: the text renderer
//! and the egui editor overlay. They are whole shaders, not composed, so they
//! sit in `standalone/` rather than `pipelines/`. They are the only `_WGSL` constants; a new
//! pipeline goes through the backend's table.

/// Shader for text rendering. Consumed by
/// [`StandardTextRenderer::new`](crate::graphics::text::StandardTextRenderer::new)
/// as a raw `String`.
pub const TEXT_WGSL: &str = include_str!("shaders/standalone/text.wgsl");

/// Shader for the egui editor overlay. Consumed by
/// `WgpuRenderSystem::create_editor_overlay_and_shell` as a raw `&str`.
pub const EGUI_WGSL: &str = include_str!("shaders/standalone/egui.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_shader_valid() {
        assert!(TEXT_WGSL.contains("@vertex"));
        assert!(TEXT_WGSL.contains("@fragment"));
    }

    #[test]
    fn egui_shader_valid() {
        assert!(EGUI_WGSL.contains("@vertex"));
        assert!(EGUI_WGSL.contains("@fragment"));
    }
}
