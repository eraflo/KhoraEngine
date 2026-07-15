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
//!
//! Colour maths and the gradient live once in `khora_tool_ui::widgets`; the
//! helpers here re-expose them under the editor's historical names, plus the
//! icon / text conveniences that are just thin wrappers over the `UiBuilder`
//! trait (no logic to share).

use khora_sdk::editor_ui::{FontFamilyHint, Icon, TextAlign, UiBuilder};

/// Returns `color` with its alpha channel replaced. See
/// [`khora_tool_ui::widgets::with_alpha`].
pub fn with_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    khora_tool_ui::widgets::with_alpha(color, alpha)
}

/// Paints a vertical gradient. Adapter over
/// [`khora_tool_ui::widgets::vertical_gradient`].
pub fn paint_vertical_gradient(
    ui: &mut dyn UiBuilder,
    rect: [f32; 4],
    top: [f32; 4],
    bottom: [f32; 4],
    steps: u32,
) {
    khora_tool_ui::widgets::vertical_gradient(ui, rect, top, bottom, steps);
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
