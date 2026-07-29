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

//! Wireframe debug-overlay configuration.
//!
//! Drawing every scene mesh as edge lines is a debug view, like the editor
//! [`GridConfig`](super::GridConfig). Only the contract type lives in
//! `khora-data`; the GPU work is the engine-side `WireframeLane`'s job.
//!
//! A host application opts in by enabling this config (shared as an
//! `Arc<Mutex<WireframeConfig>>` runtime resource). Disabled by default — the
//! editor drives it from a viewport toggle; the sandbox leaves it off.

use khora_core::math::LinearRgba;

/// Wireframe debug-overlay state.
#[derive(Debug, Clone)]
pub struct WireframeConfig {
    /// Whether `WireframeLane` draws the scene meshes as edge lines.
    pub enabled: bool,
    /// Line color (linear RGBA; alpha modulates the anti-aliased edge).
    pub line_color: LinearRgba,
    /// Edge line width, in the shader's barycentric units.
    pub line_width: f32,
}

impl Default for WireframeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // A bright cyan reads clearly over both lit surfaces and the sky.
            line_color: LinearRgba::new(0.2, 0.9, 1.0, 1.0),
            line_width: 1.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default() {
        assert!(!WireframeConfig::default().enabled);
    }
}
