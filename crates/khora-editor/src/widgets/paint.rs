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

//! Low-level painting helpers used by other widgets.

use khora_sdk::editor_ui::{FontFamilyHint, Icon, TextAlign, UiBuilder};

/// Returns `color` with its alpha channel replaced.
pub fn with_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)]
}

/// Linear interpolation between two RGBA colors. Used by
/// [`paint_vertical_gradient`].
fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - t) + b[0] * t,
        a[1] * (1.0 - t) + b[1] * t,
        a[2] * (1.0 - t) + b[2] * t,
        a[3] * (1.0 - t) + b[3] * t,
    ]
}

/// Paints a vertical gradient by stacking `steps` strips that lerp between
/// `top` and `bottom`. Cheap approximation of an actual gradient — egui has
/// no native gradient support.
pub fn paint_vertical_gradient(
    ui: &mut dyn UiBuilder,
    rect: [f32; 4],
    top: [f32; 4],
    bottom: [f32; 4],
    steps: u32,
) {
    let [x, y, w, h] = rect;
    let steps = steps.max(1);
    let strip_h = h / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1).max(1) as f32;
        let color = lerp_color(top, bottom, t);
        ui.paint_rect_filled(
            [x, y + strip_h * i as f32],
            [w, strip_h.ceil() + 0.6],
            color,
            0.0,
        );
    }
}

/// 1-pixel horizontal hairline.
pub fn paint_hairline_h(ui: &mut dyn UiBuilder, x: f32, y: f32, w: f32, color: [f32; 4]) {
    ui.paint_line([x, y], [x + w, y], color, 1.0);
}

/// Paints a single Lucide icon glyph at the given position. The icon is
/// rendered with the bundled `"icons"` font family — falls back to a single
/// dot if that family isn't installed.
pub fn paint_icon(ui: &mut dyn UiBuilder, pos: [f32; 2], icon: Icon, size: f32, color: [f32; 4]) {
    ui.paint_text_styled(
        pos,
        icon.glyph(),
        size,
        color,
        FontFamilyHint::Icons,
        TextAlign::Left,
    );
}

/// Paints proportional text at the given position with explicit size + color.
pub fn paint_text_size(
    ui: &mut dyn UiBuilder,
    pos: [f32; 2],
    text: &str,
    size: f32,
    color: [f32; 4],
) {
    ui.paint_text_styled(
        pos,
        text,
        size,
        color,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}
