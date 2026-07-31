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

//! Modal — the one place a tool is allowed to stop the user and ask.
//!
//! Reserved for actions that cannot be undone. Anything reversible should just
//! happen, with the outcome reported afterwards; a dialog for a recoverable
//! action trains people to dismiss dialogs without reading them, which is
//! exactly what makes the irreversible one dangerous.

use khora_core::ui::editor::ui_builder::FontFamilyHint;
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::buttons::{button, Button, ButtonKind};
use super::paint::{fill, fill_stroke, icon_centered, text_centered, tint, with_alpha};
use super::Rect;

/// What the user did with a [`confirm_modal`] this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalChoice {
    /// Still open — the user has not answered yet.
    Pending,
    /// Dismissed: the backdrop, the cancel button, or the close affordance.
    Cancelled,
    /// The user took the action.
    Confirmed,
}

/// Content and tone of a confirmation dialog.
pub struct Confirm<'a> {
    /// Short question, sentence case, no trailing question mark ceremony —
    /// "Delete 3 assets?" rather than "Are you sure you want to…".
    pub title: &'a str,
    /// One line naming the consequence, including anything irreversible.
    pub body: &'a str,
    /// Verb in the user's voice: `Delete`, `Discard`, `Overwrite`.
    pub confirm_label: &'a str,
    /// Renders the confirm button as destructive.
    pub danger: bool,
}

impl<'a> Confirm<'a> {
    /// A destructive confirmation.
    pub fn danger(title: &'a str, body: &'a str, confirm_label: &'a str) -> Self {
        Self {
            title,
            body,
            confirm_label,
            danger: true,
        }
    }
}

/// Width of the dialog box. Narrow on purpose: a confirmation that needs more
/// room is asking the wrong question.
const BOX_W: f32 = 380.0;
const BOX_H: f32 = 168.0;
const PAD: f32 = 20.0;
const BTN_W: f32 = 96.0;
const BTN_H: f32 = 30.0;

/// Paints a centred confirmation dialog over `screen` and returns the choice.
///
/// The caller owns the "is it open" flag; this only draws and reports. Clicking
/// the dimmed backdrop cancels, which is the same escape hatch `Esc` gives —
/// a modal with no way out but the destructive button is a trap.
pub fn confirm_modal(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    screen: Rect,
    id_salt: &str,
    spec: Confirm<'_>,
) -> ModalChoice {
    // Backdrop: dim the work rather than blur it, so the user keeps their
    // bearings and can see what the action is about to affect.
    let backdrop = ui.interact_rect(&format!("{id_salt}-backdrop"), screen);
    fill(ui, screen, with_alpha(theme.background, 0.72), 0.0);

    let bx = screen[0] + (screen[2] - BOX_W) * 0.5;
    let by = screen[1] + (screen[3] - BOX_H) * 0.5;
    let boxr = [bx, by, BOX_W, BOX_H];

    fill_stroke(
        ui,
        boxr,
        theme.surface_elevated,
        theme.border_strong,
        theme.radius_lg,
    );

    let accent = if spec.danger {
        theme.error
    } else {
        theme.primary
    };
    let glyph = if spec.danger { Icon::Warn } else { Icon::Info };

    // Icon plate, centred above the title — the shape reads before the words.
    let plate = 40.0;
    let plate_rect = [bx + (BOX_W - plate) * 0.5, by + PAD, plate, plate];
    fill(ui, plate_rect, tint(accent, 0.14), theme.radius_md);
    icon_centered(ui, plate_rect, glyph, 20.0, accent);

    let title_row = [bx, by + PAD + plate + 10.0, BOX_W, 22.0];
    text_centered(
        ui,
        title_row,
        spec.title,
        theme.font_size_title,
        theme.text,
        FontFamilyHint::Proportional,
    );

    let body_row = [bx, title_row[1] + title_row[3] + 2.0, BOX_W, 18.0];
    text_centered(
        ui,
        body_row,
        spec.body,
        theme.font_size_caption + 1.0,
        theme.text_muted,
        FontFamilyHint::Proportional,
    );

    // Cancel sits left of confirm: the safe choice is the one the hand reaches
    // first, and the destructive one never lands under a reflexive click.
    let btn_y = by + BOX_H - PAD - BTN_H;
    let gap = 10.0;
    let total = BTN_W * 2.0 + gap;
    let bx0 = bx + (BOX_W - total) * 0.5;

    let cancel = button(
        ui,
        theme,
        [bx0, btn_y, BTN_W, BTN_H],
        &format!("{id_salt}-cancel"),
        Button::new("Cancel", ButtonKind::Ghost),
    );
    let confirm_kind = if spec.danger {
        ButtonKind::Danger
    } else {
        ButtonKind::Primary
    };
    let confirm = button(
        ui,
        theme,
        [bx0 + BTN_W + gap, btn_y, BTN_W, BTN_H],
        &format!("{id_salt}-confirm"),
        Button::new(spec.confirm_label, confirm_kind),
    );

    if confirm.clicked {
        ModalChoice::Confirmed
    } else if cancel.clicked || backdrop.clicked {
        ModalChoice::Cancelled
    } else {
        ModalChoice::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingUiBuilder;

    fn theme() -> UiTheme {
        crate::khora_dark()
    }

    /// The dialog states the question and the consequence — a confirmation
    /// that only says "Are you sure?" makes the user guess what they are
    /// confirming.
    #[test]
    fn confirm_modal_paints_title_and_body() {
        let mut ui = RecordingUiBuilder::new(800.0, 600.0);
        confirm_modal(
            &mut ui,
            &theme(),
            [0.0, 0.0, 800.0, 600.0],
            "del",
            Confirm::danger(
                "Delete 3 assets?",
                "This moves them to the recycle bin.",
                "Delete",
            ),
        );
        assert!(ui.painted_text("Delete 3 assets?"));
        assert!(ui.painted_text("This moves them to the recycle bin."));
        assert!(ui.painted_text("Cancel"));
        assert!(ui.painted_text("Delete"));
    }

    /// Nothing happens until the user answers: a modal that reports an action
    /// on the frame it opens would fire on the click that opened it.
    #[test]
    fn confirm_modal_defaults_to_pending() {
        let mut ui = RecordingUiBuilder::new(800.0, 600.0);
        let choice = confirm_modal(
            &mut ui,
            &theme(),
            [0.0, 0.0, 800.0, 600.0],
            "del",
            Confirm::danger("Delete?", "Irreversible.", "Delete"),
        );
        assert_eq!(choice, ModalChoice::Pending);
    }
}
