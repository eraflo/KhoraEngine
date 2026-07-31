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

//! Feedback — what the tools say when something happened, or when there is
//! nothing to show.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::chips::Tone;
use super::paint::{fill_stroke, icon, stroke, text, text_centered, tint};
use super::{text_y, Rect};

/// A transient banner / toast: icon, bold title, explanatory body, dismiss.
///
/// The tone tints the plate and the icon; the text stays in the normal ink so
/// it remains readable — coloured body text on a coloured plate is the classic
/// way to make an alert *look* urgent and *read* badly.
///
/// Returns the interaction on the dismiss affordance.
pub fn banner(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    title: &str,
    body: &str,
    tone: Tone,
) -> Interaction {
    let accent = tone.fg(theme);
    fill_stroke(
        ui,
        rect,
        tint(accent, 0.10),
        tint(accent, 0.40),
        theme.radius_md,
    );

    let glyph = match tone {
        Tone::Error | Tone::Warning => Icon::Warn,
        Tone::Success => Icon::CheckCircle,
        _ => Icon::Info,
    };

    let pad = 12.0;
    let isize = theme.font_size_title + 2.0;
    icon(ui, [rect[0] + pad, rect[1] + pad], glyph, isize, accent);

    let tx = rect[0] + pad + isize + 10.0;
    let tsize = theme.font_size_body;
    text(ui, [tx, rect[1] + pad], title, tsize, theme.text);
    if !body.is_empty() {
        text(
            ui,
            [tx, rect[1] + pad + tsize + 3.0],
            body,
            theme.font_size_caption + 1.0,
            theme.text_muted,
        );
    }

    // Dismiss.
    let close: Rect = [super::right(rect) - pad - 16.0, rect[1] + pad, 16.0, 16.0];
    let out = ui.interact_rect(id_salt, close);
    let fg = if out.hovered {
        theme.text
    } else {
        theme.text_disabled
    };
    super::paint::icon_centered(ui, close, Icon::Close, 14.0, fg);

    out
}

/// An empty state. It teaches the interface instead of announcing a void:
/// a glyph, what this place is for, and (optionally) the way out of it.
///
/// The dashed border says "content belongs here" — a solid card would read as
/// a thing rather than the absence of things.
pub fn empty_state(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    glyph: Icon,
    title: &str,
    hint: &str,
) {
    stroke(ui, rect, theme.border_strong, theme.radius_lg, 1.0);

    let c = super::center(rect);
    let isize = 22.0;
    let block_h = isize + 8.0 + theme.font_size_body + 4.0 + theme.font_size_caption;
    let top = c[1] - block_h * 0.5;

    ui.paint_text_styled(
        [c[0], top],
        glyph.glyph(),
        isize,
        theme.primary_dim,
        FontFamilyHint::Icons,
        khora_core::ui::editor::ui_builder::TextAlign::Center,
    );

    let ty = top + isize + 8.0;
    text_centered(
        ui,
        [rect[0], ty, rect[2], theme.font_size_body],
        title,
        theme.font_size_body,
        theme.text,
        FontFamilyHint::Proportional,
    );

    if !hint.is_empty() {
        let hy = ty + theme.font_size_body + 4.0;
        text_centered(
            ui,
            [rect[0], hy, rect[2], theme.font_size_caption],
            hint,
            theme.font_size_caption + 1.0,
            theme.text_muted,
            FontFamilyHint::Proportional,
        );
    }
}

/// A tooltip plate — elevated surface, strong border, caption text.
pub fn tooltip(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, label: &str) {
    fill_stroke(
        ui,
        rect,
        theme.surface_interactive,
        theme.border_strong,
        theme.radius_sm,
    );
    let size = theme.font_size_caption + 1.0;
    text(
        ui,
        [rect[0] + 10.0, text_y(rect, size)],
        label,
        size,
        theme.text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn error_banner_tints_the_plate_but_keeps_the_title_readable() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(400.0, 70.0);
        banner(
            &mut ui,
            &t,
            [0.0, 0.0, 380.0, 60.0],
            "x",
            "Download failed",
            "Couldn't reach github.com",
            Tone::Error,
        );

        assert_eq!(
            ui.text_color("Download failed"),
            Some(t.text),
            "title must stay in the normal ink, not the error color"
        );
        assert!(
            ui.fill_colors()
                .iter()
                .any(|c| c[0] == t.error[0] && c[3] < 0.5),
            "the plate is a translucent tint of the tone"
        );
    }

    #[test]
    fn banner_dismiss_is_clickable() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(400.0, 70.0).with_click("x");
        let out = banner(
            &mut ui,
            &t,
            [0.0, 0.0, 380.0, 60.0],
            "x",
            "Installed",
            "",
            Tone::Info,
        );
        assert!(out.clicked);
    }

    #[test]
    fn empty_state_teaches_rather_than_announcing_a_void() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 160.0);
        empty_state(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 140.0],
            Icon::Layers,
            "No projects yet",
            "Create one to start building.",
        );
        assert!(ui.painted_text("No projects yet"));
        assert!(ui.painted_text("Create one to start building."));
        assert!(
            ui.rects_filled().is_empty(),
            "an empty state is outlined, never filled — it is an absence, not a card"
        );
    }

    #[test]
    fn empty_state_hint_is_optional() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 160.0);
        empty_state(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 140.0],
            Icon::Layers,
            "Empty scene",
            "",
        );
        assert!(ui.painted_text("Empty scene"));
    }
}
