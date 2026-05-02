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

//! Small painting helpers shared by chrome panels.
//!
//! These functions only call [`UiBuilder`] primitives — they're backend-
//! agnostic. They expect coordinates in the *screen-space* domain returned by
//! [`UiBuilder::panel_rect`].

use khora_sdk::editor_ui::UiBuilder;

/// Paints a diamond-shaped outline (the Khora brand mark) centered at
/// `(cx, cy)` with the given half-size. Uses 4 line segments — the resulting
/// shape is rotated 45° from an axis-aligned square.
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

/// Paints a filled diamond (centered) by overlaying many short horizontal
/// lines. Used when an outline isn't enough (small brand glyphs).
pub fn paint_diamond_filled(
    ui: &mut dyn UiBuilder,
    cx: f32,
    cy: f32,
    size: f32,
    color: [f32; 4],
) {
    // Sample the diamond as horizontal scanlines at sub-pixel resolution.
    let steps = (size * 2.0).ceil() as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let y = cy - size + t * 2.0 * size;
        let half = size * (1.0 - (t - 0.5).abs() * 2.0);
        if half < 0.5 {
            continue;
        }
        ui.paint_line([cx - half, y], [cx + half, y], color, 1.0);
    }
}

/// Paints a horizontal "gradient" approximated by `steps` stacked horizontal
/// strips, lerping between `top_color` and `bottom_color`. Good enough for
/// the subtle title-bar / panel-header gradients in the brand chrome.
pub fn paint_vertical_gradient(
    ui: &mut dyn UiBuilder,
    rect: [f32; 4],
    top_color: [f32; 4],
    bottom_color: [f32; 4],
    steps: u32,
) {
    let [x, y, w, h] = rect;
    let steps = steps.max(1);
    let strip_h = h / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1).max(1) as f32;
        let color = lerp_color(top_color, bottom_color, t);
        ui.paint_rect_filled(
            [x, y + strip_h * i as f32],
            [w, strip_h.ceil() + 0.5],
            color,
            0.0,
        );
    }
}

/// Linear interpolation between two RGBA colors in the source linear-RGB
/// space. Components are clamped after the lerp.
pub fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - t) + b[0] * t,
        a[1] * (1.0 - t) + b[1] * t,
        a[2] * (1.0 - t) + b[2] * t,
        a[3] * (1.0 - t) + b[3] * t,
    ]
}

/// Modifies a color's alpha channel without touching RGB. Useful for pulling
/// translucent variants (e.g. `surface @ 85%`) out of a single base palette
/// entry.
pub fn with_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)]
}

/// Paints a 1px hairline separator across a horizontal strip.
pub fn paint_hairline_h(ui: &mut dyn UiBuilder, x: f32, y: f32, w: f32, color: [f32; 4]) {
    ui.paint_line([x, y], [x + w, y], color, 1.0);
}

/// Paints a 1px hairline separator across a vertical strip.
pub fn paint_hairline_v(ui: &mut dyn UiBuilder, x: f32, y: f32, h: f32, color: [f32; 4]) {
    ui.paint_line([x, y], [x, y + h], color, 1.0);
}
