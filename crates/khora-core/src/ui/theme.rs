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

//! Backend-agnostic UI theme — palette + sizing + type tokens.
//!
//! All slots are *semantic*: `primary`, `accent_a`, `success`, etc. The theme
//! contains no brand-specific naming — apps configure the concrete values.
//! All color slots are `[r, g, b, a]` in linear space (0.0–1.0). The concrete
//! UI backend converts them to its native format.
//!
//! Same struct used by both the editor (`khora-editor`) and the hub
//! (`hub`); each app defines its own `khora_dark()`-style constructor
//! with its own values.

/// Color palette and sizing tokens for any Khora UI surface.
#[derive(Debug, Clone)]
pub struct UiTheme {
    // ── Surfaces ─────────────────────────────────────
    /// Outermost background (empty regions, behind everything).
    pub background: [f32; 4],
    /// Default panel surface.
    pub surface: [f32; 4],
    /// Slightly elevated surface (e.g. tab bar, card body).
    pub surface_elevated: [f32; 4],
    /// Interactive surface (buttons, combo boxes).
    pub surface_interactive: [f32; 4],
    /// Hovered or active surface highlight.
    pub surface_active: [f32; 4],

    // ── Lines / borders ──────────────────────────────
    /// Subtle separator (between items inside a panel).
    pub separator: [f32; 4],
    /// Default panel border.
    pub border: [f32; 4],
    /// Stronger border (used for emphasis around modals, popups).
    pub border_strong: [f32; 4],

    // ── Text ─────────────────────────────────────────
    /// Primary text color.
    pub text: [f32; 4],
    /// Secondary text (labels, sub-titles).
    pub text_dim: [f32; 4],
    /// Tertiary text (hints, captions).
    pub text_muted: [f32; 4],
    /// Disabled text.
    pub text_disabled: [f32; 4],

    // ── Brand / accents ──────────────────────────────
    /// Primary brand color (selection rings, focus highlights).
    pub primary: [f32; 4],
    /// Dimmed primary (backgrounds of selected rows, etc.).
    pub primary_dim: [f32; 4],
    /// Accent A — secondary brand color.
    pub accent_a: [f32; 4],
    /// Accent B — tertiary brand color.
    pub accent_b: [f32; 4],
    /// Accent C — quaternary brand color (warnings of-emphasis, special states).
    pub accent_c: [f32; 4],

    // ── Status colors ────────────────────────────────
    /// Success indicator (green-family).
    pub success: [f32; 4],
    /// Warning indicator (amber-family).
    pub warning: [f32; 4],
    /// Error indicator (red-family).
    pub error: [f32; 4],

    // ── 3D axes ──────────────────────────────────────
    /// Color for the X axis (typically red-orange).
    pub axis_x: [f32; 4],
    /// Color for the Y axis (typically green).
    pub axis_y: [f32; 4],
    /// Color for the Z axis (typically blue).
    pub axis_z: [f32; 4],

    // ── Sizing tokens ────────────────────────────────
    /// Small corner radius (chips, badges).
    pub radius_sm: f32,
    /// Medium corner radius (default for buttons / inputs).
    pub radius_md: f32,
    /// Large corner radius (panels, cards).
    pub radius_lg: f32,
    /// Extra-large corner radius (modal dialogs, palettes).
    pub radius_xl: f32,

    // ── Type sizes ───────────────────────────────────
    /// Caption / metadata font size in logical points.
    pub font_size_caption: f32,
    /// Body / default font size.
    pub font_size_body: f32,
    /// Section title font size.
    pub font_size_title: f32,
    /// Display heading font size.
    pub font_size_display: f32,

    // ── Spacing ──────────────────────────────────────
    /// Default vertical padding for a row of content.
    pub pad_row: f32,
    /// Default inner padding for a card.
    pub pad_card: f32,
}

/// Default theme: a neutral dark palette suitable for any app.
///
/// Apps that want a branded look (e.g. the Khora "Deep Navy / Silver" palette)
/// build their own [`UiTheme`] and pass it where needed.
impl Default for UiTheme {
    fn default() -> Self {
        Self {
            // Surfaces
            background: [0.039, 0.039, 0.055, 1.0],
            surface: [0.067, 0.071, 0.090, 1.0],
            surface_elevated: [0.094, 0.102, 0.129, 1.0],
            surface_interactive: [0.133, 0.145, 0.188, 1.0],
            surface_active: [0.164, 0.177, 0.220, 1.0],

            // Lines
            separator: [0.149, 0.165, 0.220, 1.0],
            border: [0.180, 0.196, 0.255, 1.0],
            border_strong: [0.260, 0.280, 0.345, 1.0],

            // Text
            text: [0.886, 0.910, 0.941, 1.0],
            text_dim: [0.612, 0.647, 0.706, 1.0],
            text_muted: [0.420, 0.455, 0.518, 1.0],
            text_disabled: [0.300, 0.325, 0.380, 1.0],

            // Brand / accents
            primary: [0.227, 0.529, 0.941, 1.0],
            primary_dim: [0.098, 0.255, 0.510, 1.0],
            accent_a: [0.486, 0.361, 0.871, 1.0],
            accent_b: [0.392, 0.741, 0.918, 1.0],
            accent_c: [0.953, 0.788, 0.353, 1.0],

            // Status
            success: [0.227, 0.722, 0.478, 1.0],
            warning: [0.941, 0.627, 0.227, 1.0],
            error: [0.941, 0.353, 0.227, 1.0],

            // Axes
            axis_x: [0.890, 0.310, 0.250, 1.0],
            axis_y: [0.450, 0.820, 0.380, 1.0],
            axis_z: [0.310, 0.560, 0.890, 1.0],

            // Radii
            radius_sm: 4.0,
            radius_md: 6.0,
            radius_lg: 10.0,
            radius_xl: 14.0,

            // Type
            font_size_caption: 10.5,
            font_size_body: 12.0,
            font_size_title: 14.0,
            font_size_display: 18.0,

            // Spacing
            pad_row: 8.0,
            pad_card: 14.0,
        }
    }
}
