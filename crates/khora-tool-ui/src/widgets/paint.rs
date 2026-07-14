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

//! Low-level paint helpers every other widget builds on.
//!
//! These wrap the raw [`UiBuilder`] primitives with the rect / color
//! conventions used across this crate. They carry no theme opinions of their
//! own — callers pass the colors in.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, TextAlign};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::{Color, Rect};

/// Returns `c` with its alpha replaced by `a` (clamped to `0..=1`).
#[inline]
pub fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], a.clamp(0.0, 1.0)]
}

/// Returns `c` with its alpha multiplied by `factor` — the "fade this out"
/// helper (disabled controls, off filter pills, ghost strokes).
#[inline]
pub fn tint(c: Color, factor: f32) -> Color {
    [c[0], c[1], c[2], (c[3] * factor).clamp(0.0, 1.0)]
}

/// Linearly interpolates between two colors. `t` is clamped to `0..=1`.
#[inline]
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Fills a rounded rect.
#[inline]
pub fn fill(ui: &mut dyn UiBuilder, rect: Rect, color: Color, rounding: f32) {
    ui.paint_rect_filled([rect[0], rect[1]], [rect[2], rect[3]], color, rounding);
}

/// Strokes a rounded rect.
#[inline]
pub fn stroke(ui: &mut dyn UiBuilder, rect: Rect, color: Color, rounding: f32, thickness: f32) {
    ui.paint_rect_stroke(
        [rect[0], rect[1]],
        [rect[2], rect[3]],
        color,
        rounding,
        thickness,
    );
}

/// Fills then strokes a rounded rect — the standard "card / chip / input"
/// treatment.
#[inline]
pub fn fill_stroke(
    ui: &mut dyn UiBuilder,
    rect: Rect,
    fill_color: Color,
    stroke_color: Color,
    rounding: f32,
) {
    fill(ui, rect, fill_color, rounding);
    stroke(ui, rect, stroke_color, rounding, 1.0);
}

/// Approximates a vertical gradient by stacking horizontal strips.
///
/// The paint API has no gradient primitive; the strips overlap by a fraction of
/// a pixel so no seams show at fractional DPI scales.
pub fn vertical_gradient(
    ui: &mut dyn UiBuilder,
    rect: Rect,
    top: Color,
    bottom: Color,
    steps: u32,
) {
    let steps = steps.max(2);
    let strip_h = rect[3] / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let y = rect[1] + strip_h * i as f32;
        ui.paint_rect_filled(
            [rect[0], y],
            [rect[2], strip_h.ceil() + 0.6],
            lerp_color(top, bottom, t),
            0.0,
        );
    }
}

/// A 1px horizontal separator across `[x0, x1]` at `y`, in the theme's
/// separator color.
#[inline]
pub fn hairline_h(ui: &mut dyn UiBuilder, theme: &UiTheme, x0: f32, x1: f32, y: f32) {
    ui.paint_line([x0, y], [x1, y], theme.separator, 1.0);
}

/// A 1px vertical separator down `[y0, y1]` at `x`.
#[inline]
pub fn hairline_v(ui: &mut dyn UiBuilder, theme: &UiTheme, y0: f32, y1: f32, x: f32) {
    ui.paint_line([x, y0], [x, y1], theme.separator, 1.0);
}

/// Paints an [`Icon`] glyph from the icon font at `pos` (top-left anchored).
#[inline]
pub fn icon(ui: &mut dyn UiBuilder, pos: [f32; 2], glyph: Icon, size: f32, color: Color) {
    ui.paint_text_styled(
        pos,
        glyph.glyph(),
        size,
        color,
        FontFamilyHint::Icons,
        TextAlign::Left,
    );
}

/// Paints an [`Icon`] centred inside `rect`.
#[inline]
pub fn icon_centered(ui: &mut dyn UiBuilder, rect: Rect, glyph: Icon, size: f32, color: Color) {
    let c = super::center(rect);
    ui.paint_text_styled(
        [c[0], c[1] - size * 0.5],
        glyph.glyph(),
        size,
        color,
        FontFamilyHint::Icons,
        TextAlign::Center,
    );
}

/// Paints left-aligned proportional text at `pos`.
#[inline]
pub fn text(ui: &mut dyn UiBuilder, pos: [f32; 2], s: &str, size: f32, color: Color) {
    ui.paint_text_styled(
        pos,
        s,
        size,
        color,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}

/// Paints left-aligned monospace text at `pos` — for numbers, paths, IDs, and
/// anything the user reads as data rather than prose.
#[inline]
pub fn mono(ui: &mut dyn UiBuilder, pos: [f32; 2], s: &str, size: f32, color: Color) {
    ui.paint_text_styled(
        pos,
        s,
        size,
        color,
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
}

/// Paints left-aligned display (serif) text — screen headings and hero
/// numerals only. Falls back to the proportional face if Fraunces is absent.
#[inline]
pub fn display(ui: &mut dyn UiBuilder, pos: [f32; 2], s: &str, size: f32, color: Color) {
    ui.paint_text_styled(
        pos,
        s,
        size,
        color,
        FontFamilyHint::Display,
        TextAlign::Left,
    );
}

/// Paints text vertically centred in `rect`, starting at `rect.x + pad_x`.
#[inline]
pub fn text_in(
    ui: &mut dyn UiBuilder,
    rect: Rect,
    s: &str,
    size: f32,
    color: Color,
    pad_x: f32,
    family: FontFamilyHint,
) {
    ui.paint_text_styled(
        [rect[0] + pad_x, super::text_y(rect, size)],
        s,
        size,
        color,
        family,
        TextAlign::Left,
    );
}

/// Paints text centred both ways inside `rect`.
#[inline]
pub fn text_centered(
    ui: &mut dyn UiBuilder,
    rect: Rect,
    s: &str,
    size: f32,
    color: Color,
    family: FontFamilyHint,
) {
    let c = super::center(rect);
    ui.paint_text_styled(
        [c[0], super::text_y(rect, size)],
        s,
        size,
        color,
        family,
        TextAlign::Center,
    );
}

/// Paints a vertical accent bar hugging the left edge of `rect` — the
/// selection marker used by tree nodes, palette rows, and spine buttons.
#[inline]
pub fn selection_bar(ui: &mut dyn UiBuilder, rect: Rect, color: Color) {
    let inset = 4.0;
    fill(
        ui,
        [
            rect[0],
            rect[1] + inset,
            2.0,
            (rect[3] - 2.0 * inset).max(0.0),
        ],
        color,
        1.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn with_alpha_replaces_and_clamps() {
        assert_eq!(with_alpha([1.0, 0.5, 0.0, 1.0], 0.25)[3], 0.25);
        assert_eq!(with_alpha([1.0, 0.5, 0.0, 1.0], 5.0)[3], 1.0);
        assert_eq!(with_alpha([1.0, 0.5, 0.0, 1.0], -1.0)[3], 0.0);
    }

    #[test]
    fn tint_multiplies_alpha_and_keeps_rgb() {
        let c = tint([0.2, 0.4, 0.6, 0.8], 0.5);
        assert_eq!([c[0], c[1], c[2]], [0.2, 0.4, 0.6]);
        assert!((c[3] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn lerp_color_hits_both_ends_and_midpoint() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
        assert!((lerp_color(a, b, 0.5)[0] - 0.5).abs() < 1e-6);
        // out-of-range t is clamped, never extrapolated
        assert_eq!(lerp_color(a, b, 2.0), b);
    }

    #[test]
    fn selection_bar_is_two_px_on_the_left_edge() {
        let mut ui = RecordingUiBuilder::new(100.0, 40.0);
        selection_bar(&mut ui, [10.0, 0.0, 80.0, 20.0], [1.0, 0.0, 0.0, 1.0]);
        let rects = ui.rects_filled();
        assert_eq!(rects.len(), 1);
        if let crate::testing::PaintCall::RectFilled { min, size, .. } = rects[0] {
            assert_eq!(min[0], 10.0, "bar hugs the rect's left edge");
            assert_eq!(size[0], 2.0, "bar is 2px wide");
        } else {
            panic!("expected a filled rect");
        }
    }

    #[test]
    fn text_in_uses_the_requested_family() {
        let mut ui = RecordingUiBuilder::new(100.0, 24.0);
        mono(&mut ui, [0.0, 0.0], "14.2ms", 11.0, [1.0; 4]);
        assert_eq!(
            ui.text_family("14.2ms"),
            Some(FontFamilyHint::Monospace),
            "numbers must render in the monospace face"
        );
    }
}
