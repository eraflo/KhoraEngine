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

//! Editor theme data — backend-agnostic color palette.

/// Color palette for the editor UI.
///
/// All colors are `[r, g, b, a]` in linear (0.0–1.0) space.
/// The concrete UI backend converts them to its native format.
#[derive(Debug, Clone)]
pub struct EditorTheme {
    /// Main window / empty area background.
    pub background: [f32; 4],
    /// Panel and widget surface color.
    pub surface: [f32; 4],
    /// Hovered / active surface highlight.
    pub surface_highlight: [f32; 4],
    /// Primary accent (interactive elements, selections).
    pub primary: [f32; 4],
    /// Default text color.
    pub text: [f32; 4],
    /// Dimmed / secondary text.
    pub text_dim: [f32; 4],
    /// Accent color (buttons, links).
    pub accent: [f32; 4],
    /// Success indicator.
    pub success: [f32; 4],
    /// Warning indicator.
    pub warning: [f32; 4],
    /// Error indicator.
    pub error: [f32; 4],
    /// Panel borders.
    pub border: [f32; 4],
    /// Separator lines.
    pub separator: [f32; 4],
}

/// Default theme: modern dark palette with blue and silver accents.
impl Default for EditorTheme {
    fn default() -> Self {
        Self {
            background: [0.067, 0.071, 0.082, 1.0],       // #111214 — deepest black
            surface: [0.106, 0.114, 0.133, 1.0],           // #1b1d22 — panel body
            surface_highlight: [0.149, 0.161, 0.188, 1.0], // #262930 — hovered/active row
            primary: [0.227, 0.529, 0.941, 1.0],           // #3a87f0 — vivid blue accent
            text: [0.906, 0.918, 0.941, 1.0],              // #e7eaf0 — near-white text
            text_dim: [0.533, 0.561, 0.616, 1.0],          // #888f9d — silver/muted text
            accent: [0.337, 0.647, 1.0, 1.0],              // #56a5ff — lighter blue links
            success: [0.302, 0.788, 0.467, 1.0],           // #4dc977 — green
            warning: [0.969, 0.761, 0.263, 1.0],           // #f7c243 — amber
            error: [0.937, 0.325, 0.314, 1.0],             // #ef5350 — red
            border: [0.176, 0.192, 0.224, 1.0],            // #2d3139 — subtle border
            separator: [0.176, 0.192, 0.224, 1.0],         // #2d3139 — subtle separator
        }
    }
}
