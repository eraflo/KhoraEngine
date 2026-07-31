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

//! Containers — the frames that group things.

use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill_stroke, icon, text};
use super::{right, text_y, Rect};

/// Height of a card's header strip.
const HEAD_H: f32 = 40.0;

/// A titled panel card: an icon, a title, a hairline, and a body.
///
/// Returns the **body rect** — the area inside the card, below the header —
/// so the caller fills it however it likes. Cards do not nest: a card inside a
/// card is the point at which grouping stops grouping anything.
pub fn card(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, glyph: Icon, title: &str) -> Rect {
    fill_stroke(ui, rect, theme.surface, theme.border, theme.radius_md);

    let pad = 16.0;
    let head: Rect = [rect[0], rect[1], rect[2], HEAD_H];
    let size = theme.font_size_body + 0.5;

    icon(
        ui,
        [rect[0] + pad, text_y(head, size + 2.0)],
        glyph,
        size + 2.0,
        theme.primary_dim,
    );
    text(
        ui,
        [rect[0] + pad + size + 11.0, text_y(head, size)],
        title,
        size,
        theme.text,
    );

    // Hairline between header and body.
    ui.paint_line(
        [rect[0], rect[1] + HEAD_H],
        [right(rect), rect[1] + HEAD_H],
        theme.separator,
        1.0,
    );

    [
        rect[0] + pad,
        rect[1] + HEAD_H + pad,
        (rect[2] - pad * 2.0).max(0.0),
        (rect[3] - HEAD_H - pad * 2.0).max(0.0),
    ]
}

/// The rect a card's header-right slot occupies — for a status chip or a small
/// action button aligned with the title.
pub fn card_action_slot(rect: Rect, width: f32) -> Rect {
    let pad = 16.0;
    [
        right(rect) - pad - width,
        rect[1] + (HEAD_H - 26.0) * 0.5,
        width,
        26.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn card_returns_a_body_rect_inside_itself() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(400.0, 200.0);
        let outer: Rect = [0.0, 0.0, 380.0, 180.0];
        let body = card(&mut ui, &t, outer, Icon::Github, "GitHub");

        assert!(ui.painted_text("GitHub"));
        assert!(body[1] > outer[1] + HEAD_H, "body starts below the header");
        assert!(
            body[0] >= outer[0] && right(body) <= right(outer),
            "body must not escape the card horizontally"
        );
        assert!(body[3] > 0.0, "body has usable height");
    }

    #[test]
    fn a_card_too_short_for_its_header_yields_no_negative_body() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(400.0, 40.0);
        let body = card(&mut ui, &t, [0.0, 0.0, 380.0, 20.0], Icon::Github, "GitHub");
        assert!(body[2] >= 0.0 && body[3] >= 0.0, "clamped, never negative");
    }

    #[test]
    fn action_slot_sits_at_the_header_right() {
        let outer: Rect = [0.0, 0.0, 380.0, 180.0];
        let slot = card_action_slot(outer, 80.0);
        assert!(right(slot) < right(outer), "inside the card");
        assert!(slot[1] < HEAD_H, "vertically within the header strip");
    }
}
