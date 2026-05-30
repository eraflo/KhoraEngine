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

//! Built-in shader sources for the Khora Engine rendering system.
//!
//! All rendering lanes now consume shaders through
//! [`crate::render_lane::ShaderRegistry`] (naga_oil composer +
//! validation at boot). This module exposes the two remaining legacy
//! `include_str!` constants still required by infra-side consumers
//! (the egui editor overlay and the text renderer) — they take raw
//! WGSL strings, not registry handles.
//!
//! New lanes / pipelines should be registered in
//! [`crate::render_lane::shader_registry::PIPELINE_MODULES`] and
//! looked up by name; no new `_WGSL` constants should appear here.

/// Shader for text rendering. Consumed by
/// `khora_infra::StandardTextRenderer::new` as a raw `String`.
pub const TEXT_WGSL: &str = include_str!("pipelines/text.wgsl");

/// Shader for the egui editor overlay. Consumed by
/// `khora_infra::WgpuRenderSystem::create_editor_overlay_and_shell`
/// as a raw `&str`.
pub const EGUI_WGSL: &str = include_str!("pipelines/egui.wgsl");

// NOTE — the grid and gizmo shaders are NOT exposed as `_WGSL`
// constants: grid / gizmo rendering is owned by the engine-side
// `GridLane` / `GizmoLane`, which compose `khora::pipelines::grid` /
// `khora::pipelines::gizmo` through the `ShaderRegistry` like every
// other render lane.

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
