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

/// A text-input frame. Paints the box and returns the **inner rect** the
/// caller should host the real text edit in (via
/// [`UiBuilder::region_at`](khora_core::ui::UiBuilder::region_at)).
///
/// `invalid` swaps the border to the error colour — the field itself carries
/// the failure, so the message below it can explain rather than announce.
pub fn input_frame(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    invalid: bool,
) -> Rect {
    let out = ui.interact_rect(id_salt, rect);
    let border = if invalid {
        theme.error
    } else if out.hovered {
        theme.primary
    } else {
        theme.border_strong
    };
    fill_stroke(ui, rect, theme.background, border, theme.radius_md);

    let pad = 9.0;
    [
        rect[0] + pad,
        rect[1] + (rect[3] - 20.0) * 0.5,
        (rect[2] - pad * 2.0).max(0.0),
        20.0,
    ]
}

/// A field label — the small caption above an input.
pub fn field_label(ui: &mut dyn UiBuilder, theme: &UiTheme, pos: [f32; 2], label: &str) {
    text(
        ui,
        pos,
        label,
        theme.font_size_caption + 1.0,
        theme.text_dim,
    );
}

/// A validation message under a field: a warning glyph and the reason.
///
/// It says what is wrong *and* enough to fix it — "Directory doesn't exist"
/// beats "Invalid".
pub fn error_line(ui: &mut dyn UiBuilder, theme: &UiTheme, pos: [f32; 2], message: &str) {
    let size = theme.font_size_caption;
    icon(ui, pos, Icon::Warn, size + 1.0, theme.error);
    text(
        ui,
        [pos[0] + size + 6.0, pos[1]],
        message,
        size,
        theme.error,
    );
}

/// A checkbox with its label. Returns the [`Interaction`]; the caller owns the
/// boolean and flips it on `clicked`.
pub fn checkbox(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    label: &str,
    checked: bool,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    let side = 16.0;
    let bx = rect[0];
    let by = rect[1] + (rect[3] - side) * 0.5;
    let box_rect: Rect = [bx, by, side, side];

    if checked {
        fill_stroke(ui, box_rect, theme.primary, theme.primary, theme.radius_sm);
        super::paint::icon_centered(ui, box_rect, Icon::Check, 12.0, theme.text_inverse);
    } else {
        let border = if out.hovered {
            theme.primary
        } else {
            theme.border_strong
        };
        fill_stroke(ui, box_rect, theme.background, border, theme.radius_sm);
    }

    let size = theme.font_size_body;
    text(
        ui,
        [bx + side + 9.0, text_y(rect, size)],
        label,
        size,
        theme.text_dim,
    );

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
    fn invalid_input_frame_borders_in_the_error_color() {
        let t = khora_dark();

        let mut ok = RecordingUiBuilder::new(300.0, 40.0);
        input_frame(&mut ok, &t, [0.0, 0.0, 280.0, 32.0], "name", false);

        let mut bad = RecordingUiBuilder::new(300.0, 40.0);
        input_frame(&mut bad, &t, [0.0, 0.0, 280.0, 32.0], "name", true);

        let stroke_color = |ui: &RecordingUiBuilder| {
            ui.calls().iter().find_map(|c| match c {
                crate::testing::PaintCall::RectStroke { color, .. } => Some(*color),
                _ => None,
            })
        };
        assert_eq!(stroke_color(&bad), Some(t.error));
        assert_ne!(stroke_color(&ok), Some(t.error));
    }

    #[test]
    fn input_frame_inner_rect_stays_inside_the_frame() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 40.0);
        let outer = [10.0, 0.0, 280.0, 32.0];
        let inner = input_frame(&mut ui, &t, outer, "name", false);
        assert!(inner[0] > outer[0]);
        assert!(super::super::right(inner) < super::super::right(outer));
    }

    #[test]
    fn checked_box_fills_with_brand_and_shows_a_check() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 30.0);
        checkbox(
            &mut ui,
            &t,
            [0.0, 0.0, 260.0, 24.0],
            "git",
            "Initialize a git repository",
            true,
        );

        assert!(
            ui.used_fill(t.primary),
            "a checked box is filled with the brand"
        );
        assert!(
            ui.painted_text(Icon::Check.glyph()),
            "and carries the check glyph"
        );
    }

    #[test]
    fn unchecked_box_is_empty_and_unfilled() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 30.0);
        checkbox(
            &mut ui,
            &t,
            [0.0, 0.0, 260.0, 24.0],
            "git",
            "Initialize a git repository",
            false,
        );

        assert!(!ui.used_fill(t.primary));
        assert!(!ui.painted_text(Icon::Check.glyph()));
        assert!(ui.painted_text("Initialize a git repository"));
    }

    #[test]
    fn error_line_says_what_is_wrong_in_the_error_color() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 20.0);
        error_line(&mut ui, &t, [0.0, 0.0], "Directory doesn't exist");
        assert_eq!(ui.text_color("Directory doesn't exist"), Some(t.error));
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
