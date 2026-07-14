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

//! Fields — inputs and the frames around them.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill_stroke, icon, mono, text};
use super::{text_y, Color, Rect};

/// A spatial axis. Its colour is a *label*, not decoration: X/Y/Z are always
/// red/green/cyan, in the inspector and on the gizmo alike, so the two read as
/// the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// X — red.
    X,
    /// Y — green.
    Y,
    /// Z — cyan.
    Z,
}

impl Axis {
    /// The single letter shown in the field.
    pub fn letter(self) -> &'static str {
        match self {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
        }
    }

    /// The theme slot for this axis.
    pub fn color(self, theme: &UiTheme) -> Color {
        match self {
            Axis::X => theme.axis_x,
            Axis::Y => theme.axis_y,
            Axis::Z => theme.axis_z,
        }
    }
}

/// One component of a vector: an axis letter in its axis colour, and the value
/// right-aligned in monospace.
///
/// The colour is carried by the *letter*, not the whole field — a fully tinted
/// field would turn a dense inspector into a rainbow. Returns the
/// [`Interaction`] so the caller can drive its own drag.
pub fn axis_field(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    axis: Axis,
    value: f32,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    let border = if out.hovered {
        theme.border_strong
    } else {
        theme.border
    };
    fill_stroke(ui, rect, theme.background, border, theme.radius_sm);

    let size = theme.font_size_caption;
    let y = text_y(rect, size);

    mono(
        ui,
        [rect[0] + 7.0, y],
        axis.letter(),
        size - 1.0,
        axis.color(theme),
    );

    let s = format!("{value:.1}");
    let w = ui.measure_text(&s, size, FontFamilyHint::Monospace)[0];
    mono(ui, [super::right(rect) - 7.0 - w, y], &s, size, theme.text);

    out
}

/// A search / filter input. Purely presentational — the caller owns the text
/// and hosts the real text edit inside the frame via
/// [`UiBuilder::region_at`](khora_core::ui::UiBuilder::region_at) when it needs
/// one; this paints the frame, the icon, and the placeholder.
pub fn search_field(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    content: &str,
    placeholder: &str,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    let border = if out.hovered {
        theme.border_strong
    } else {
        theme.border
    };
    fill_stroke(ui, rect, theme.background, border, theme.radius_sm);

    let size = theme.font_size_caption + 1.0;
    let y = text_y(rect, size);
    icon(
        ui,
        [rect[0] + 8.0, y],
        Icon::Search,
        size,
        theme.text_disabled,
    );

    let x = rect[0] + 8.0 + size + 6.0;
    if content.is_empty() {
        text(ui, [x, y], placeholder, size, theme.text_disabled);
    } else {
        text(ui, [x, y], content, size, theme.text);
    }

    out
}

/// A labelled property row: a fixed-width label gutter on the left, and the
/// value rect returned for the caller to fill.
///
/// Returns the rect the value control should paint into. The gutter is a
/// constant so every row in a panel aligns — the single biggest thing that
/// makes a dense inspector feel built rather than assembled.
pub fn property_row(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    label: &str,
    gutter: f32,
) -> Rect {
    let size = theme.font_size_caption + 1.0;
    text(
        ui,
        [rect[0], text_y(rect, size)],
        label,
        size,
        theme.text_muted,
    );
    let x = rect[0] + gutter;
    [x, rect[1], (super::right(rect) - x).max(0.0), rect[3]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn each_axis_uses_its_own_theme_slot() {
        let t = khora_dark();
        assert_eq!(Axis::X.color(&t), t.axis_x);
        assert_eq!(Axis::Y.color(&t), t.axis_y);
        assert_eq!(Axis::Z.color(&t), t.axis_z);
    }

    #[test]
    fn axis_field_tints_the_letter_not_the_value() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(80.0, 24.0);
        axis_field(&mut ui, &t, [0.0, 0.0, 70.0, 20.0], "pos.x", Axis::X, 1.5);

        assert_eq!(
            ui.text_color("X"),
            Some(t.axis_x),
            "the letter carries the axis color"
        );
        assert_eq!(
            ui.text_color("1.5"),
            Some(t.text),
            "the value stays neutral ink"
        );
    }

    #[test]
    fn axis_field_formats_to_one_decimal() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(80.0, 24.0);
        axis_field(&mut ui, &t, [0.0, 0.0, 70.0, 20.0], "pos.y", Axis::Y, 1.0);
        assert!(ui.painted_text("1.0"));
    }

    #[test]
    fn axis_field_values_are_monospace_so_columns_align() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(80.0, 24.0);
        axis_field(&mut ui, &t, [0.0, 0.0, 70.0, 20.0], "pos.z", Axis::Z, 0.0);
        assert_eq!(ui.text_family("0.0"), Some(FontFamilyHint::Monospace));
    }

    #[test]
    fn search_field_shows_placeholder_only_when_empty() {
        let t = khora_dark();

        let mut empty = RecordingUiBuilder::new(160.0, 30.0);
        search_field(&mut empty, &t, [0.0, 0.0, 150.0, 26.0], "s", "", "Filter…");
        assert_eq!(empty.text_color("Filter…"), Some(t.text_disabled));

        let mut typed = RecordingUiBuilder::new(160.0, 30.0);
        search_field(
            &mut typed,
            &t,
            [0.0, 0.0, 150.0, 26.0],
            "s",
            "sphere",
            "Filter…",
        );
        assert!(
            !typed.painted_text("Filter…"),
            "placeholder hidden once there is content"
        );
        assert_eq!(typed.text_color("sphere"), Some(t.text));
    }

    #[test]
    fn property_row_returns_the_value_rect_after_the_gutter() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 30.0);
        let value = property_row(&mut ui, &t, [10.0, 0.0, 200.0, 23.0], "Position", 72.0);
        assert_eq!(value[0], 82.0, "value rect starts after the label gutter");
        assert_eq!(value[2], 128.0, "and claims the remaining width");
        assert!(ui.painted_text("Position"));
    }
}
