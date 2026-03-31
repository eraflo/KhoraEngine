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
    /// Slightly elevated surface (e.g. tab bar).
    pub surface2: [f32; 4],
    /// Interactive surface (buttons, combo boxes).
    pub surface3: [f32; 4],
    /// Hovered / active surface highlight.
    pub surface_highlight: [f32; 4],
    /// Primary accent (interactive elements, selections).
    pub primary: [f32; 4],
    /// Dimmed primary (for borders/backgrounds related to primary).
    pub primary_dim: [f32; 4],
    /// Default text color.
    pub text: [f32; 4],
    /// Dimmed / secondary text.
    pub text_dim: [f32; 4],
    /// Even more muted text (hints, version labels).
    pub text_muted: [f32; 4],
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
    /// Lighter panel border (for subtle separation).
    pub border_light: [f32; 4],
    /// Separator lines.
    pub separator: [f32; 4],
    /// Toolbar background (can be darker than `background`).
    pub toolbar_bg: [f32; 4],
    /// Status bar background.
    pub status_bar_bg: [f32; 4],
}

/// Default theme: modern dark palette matching the Khora Hub brand.
impl Default for EditorTheme {
    fn default() -> Self {
        Self {
            background: [0.039, 0.039, 0.055, 1.0],        // #0A0A0E
            surface: [0.067, 0.071, 0.090, 1.0],           // #111217
            surface2: [0.094, 0.102, 0.129, 1.0],          // #181A21
            surface3: [0.133, 0.145, 0.188, 1.0],          // #222530
            surface_highlight: [0.149, 0.161, 0.188, 1.0], // #262930
            primary: [0.227, 0.529, 0.941, 1.0],           // #3A87F0
            primary_dim: [0.098, 0.255, 0.510, 1.0],       // #194182
            text: [0.886, 0.910, 0.941, 1.0],              // #E2E8F0
            text_dim: [0.533, 0.573, 0.643, 1.0],          // #8892A4
            text_muted: [0.314, 0.345, 0.416, 1.0],        // #50586A
            accent: [0.486, 0.361, 0.871, 1.0],            // #7C5CDE
            success: [0.227, 0.722, 0.478, 1.0],           // #3AB87A
            warning: [0.941, 0.627, 0.227, 1.0],           // #F0A03A
            error: [0.941, 0.353, 0.227, 1.0],             // #F05A3A
            border: [0.149, 0.165, 0.220, 1.0],            // #262A38
            border_light: [0.216, 0.235, 0.306, 1.0],      // #373C4E
            separator: [0.149, 0.165, 0.220, 1.0],         // #262A38
            toolbar_bg: [0.039, 0.043, 0.059, 1.0],        // #0A0B0F
            status_bar_bg: [0.039, 0.039, 0.055, 1.0],     // #0A0A0E
        }
    }
}
