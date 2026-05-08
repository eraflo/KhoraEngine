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

//! Brand-mark painters — the Khora "diamond" shape and its containing pills.

use khora_sdk::editor_ui::{UiTheme, FontFamilyHint, TextAlign, UiBuilder};

use super::paint::with_alpha;

/// Paints a 4-point diamond (rotated square) outline centered at `(cx, cy)`.
pub fn paint_diamond_outline(
    ui: &mut dyn UiBuilder,
    cx: f32,
    cy: f32,
    size: f32,
    color: [f32; 4],
    thickness: f32,
) {
    let top = [cx, cy - size];
    let right = [cx + size, cy];
    let bottom = [cx, cy + size];
    let left = [cx - size, cy];
    ui.paint_line(top, right, color, thickness);
    ui.paint_line(right, bottom, color, thickness);
    ui.paint_line(bottom, left, color, thickness);
    ui.paint_line(left, top, color, thickness);
}

/// Paints a filled 4-point diamond using a closed polygon path.
pub fn paint_diamond_filled(ui: &mut dyn UiBuilder, cx: f32, cy: f32, size: f32, color: [f32; 4]) {
    let pts = [
        [cx, cy - size],
        [cx + size, cy],
        [cx, cy + size],
        [cx - size, cy],
    ];
    ui.paint_path_filled(&pts, color);
}

/// Paints the editor's branded "pill" (rounded 999 background containing a
/// diamond, the engine name, a separator and the active project name).
///
/// Returns the right edge of the pill in screen-space, so the caller can
/// position the next widget after it.
pub fn paint_brand_pill(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    height: f32,
    engine_name: &str,
    project_name: &str,
    theme: &UiTheme,
) -> f32 {
    // Real font measurement (Phase 3 — replaces the previous 7px-per-char
    // guess that broke at large font sizes / non-ASCII names).
    let engine_w = ui.measure_text(
        engine_name,
        theme.font_size_body,
        FontFamilyHint::Proportional,
    )[0];
    let project_w = ui.measure_text(
        project_name,
        theme.font_size_body - 1.0,
        FontFamilyHint::Proportional,
    )[0];
    let total_w = 18.0 /* diamond */ + 12.0 + engine_w + 14.0 /* sep */ + project_w + 24.0;

    let pad_y = (height - 24.0).max(0.0) * 0.5;
    let pill_h = (height - pad_y * 2.0).max(20.0);
    let pill_x = origin[0];
    let pill_y = origin[1] + pad_y;

    // Background
    ui.paint_rect_filled(
        [pill_x, pill_y],
        [total_w, pill_h],
        with_alpha(theme.background, 0.6),
        pill_h * 0.5,
    );
    ui.paint_rect_stroke(
        [pill_x, pill_y],
        [total_w, pill_h],
        with_alpha(theme.separator, 0.6),
        pill_h * 0.5,
        1.0,
    );

    // Diamond
    let diamond_cx = pill_x + 14.0;
    let diamond_cy = pill_y + pill_h * 0.5;
    paint_diamond_filled(ui, diamond_cx, diamond_cy, 6.5, theme.primary);

    // Engine name
    let text_y = pill_y + (pill_h - theme.font_size_body) * 0.5;
    ui.paint_text_styled(
        [pill_x + 26.0, text_y],
        engine_name,
        theme.font_size_body,
        theme.text,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    // Vertical separator after engine name
    let sep_x = pill_x + 26.0 + engine_w + 6.0;
    ui.paint_line(
        [sep_x, pill_y + 5.0],
        [sep_x, pill_y + pill_h - 5.0],
        with_alpha(theme.border, 0.7),
        1.0,
    );

    // Project name
    ui.paint_text_styled(
        [sep_x + 8.0, text_y],
        project_name,
        theme.font_size_body - 1.0,
        theme.text_muted,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );

    pill_x + total_w
}
