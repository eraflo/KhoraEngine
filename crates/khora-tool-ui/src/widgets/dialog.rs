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

//! Dialog — a question with a few rows of facts and several answers.
//!
//! Where [`confirm_modal`](super::confirm_modal) asks yes or no, a dialog
//! states what is at stake — a title, a subtitle, a lead line, label · value
//! rows on a sunk plate, a note — and offers its choices, right-aligned in
//! the order given.

use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use khora_core::ui::editor::ui_builder::FontFamilyHint;

use super::buttons::{button, Button, ButtonKind};
use super::paint::{fill, fill_stroke, icon_centered, mono, text, with_alpha};
use super::Rect;

/// One answer a [`Dialog`] offers.
#[derive(Debug, Clone, Copy)]
pub struct DialogChoice<'a> {
    /// What the button says.
    pub label: &'a str,
    /// How it is drawn: the one answer the dialog leads to is `Primary`.
    pub kind: ButtonKind,
}

/// What a [`dialog`] shows.
#[derive(Debug, Clone, Copy)]
pub struct Dialog<'a> {
    /// The glyph on the plate above the title, if any.
    pub icon: Option<Icon>,
    /// The question.
    pub title: &'a str,
    /// A line under the title, in monospace — a path, a name.
    pub subtitle: Option<&'a str>,
    /// The sentence that says what is at stake.
    pub lead: &'a str,
    /// Label · value rows, on a sunk plate; none drawn when empty.
    pub rows: &'a [(&'a str, &'a str)],
    /// A line under the rows.
    pub note: &'a str,
    /// The answers, right-aligned, in order.
    pub choices: &'a [DialogChoice<'a>],
}

/// Paints `spec` in place over `rect` — a dimmed backdrop and the dialog's
/// box centred on it, as [`confirm_modal`](super::confirm_modal) does — and
/// returns the index of the choice clicked this frame, `None` when none was.
/// A click on the backdrop chooses nothing.
///
/// Interaction ids, from `id_salt`: the backdrop is `"{id_salt}-backdrop"`,
/// the choice at index `i` is `"{id_salt}-choice-{i}"`.
pub fn dialog(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    spec: &Dialog<'_>,
) -> Option<usize> {
    let backdrop = ui.interact_rect(&format!("{id_salt}-backdrop"), rect);
    fill(ui, rect, with_alpha(theme.background, 0.72), 0.0);

    let inner = BOX_W - PAD * 2.0;
    let body = theme.font_size_body;
    let caption = theme.font_size_caption + 1.0;
    let lead = wrap_text(ui, spec.lead, body, inner);
    let note = wrap_text(ui, spec.note, caption, inner);
    let line = |size: f32| (size * 1.5).round();
    // A row's value wraps in its column, as the lead and the note do.
    let value_w = inner - ROW_PAD * 2.0 - LABEL_W;
    let values: Vec<Vec<String>> = (spec.rows.iter())
        .map(|(_, value)| wrap_text(ui, value, body, value_w))
        .collect();
    let row_h = |lines: &Vec<String>| ROW_H + lines.len().saturating_sub(1) as f32 * line(body);

    // The height the parts take, laid out before the box is drawn.
    let header_h = HEADER_H;
    let lead_h = lead.len() as f32 * line(body);
    let rows_h = if spec.rows.is_empty() {
        0.0
    } else {
        values.iter().map(row_h).sum::<f32>() + ROW_PAD * 2.0
    };
    let note_h = note.len() as f32 * line(caption);
    let gaps = [header_h, lead_h, rows_h, note_h]
        .iter()
        .filter(|h| **h > 0.0)
        .count() as f32
        * GAP;
    let box_h = PAD * 2.0 + header_h + lead_h + rows_h + note_h + gaps + BTN_H;
    let bx = rect[0] + (rect[2] - BOX_W) * 0.5;
    let by = rect[1] + (rect[3] - box_h) * 0.5;
    fill_stroke(
        ui,
        [bx, by, BOX_W, box_h],
        theme.surface_elevated,
        theme.border_strong,
        theme.radius_lg,
    );

    // The header: the glyph's plate, the question, the subtitle under it.
    let x = bx + PAD;
    let mut y = by + PAD;
    let mut title_x = x;
    if let Some(glyph) = spec.icon {
        let plate = [x, y, PLATE, PLATE];
        fill(ui, plate, theme.surface_active, theme.radius_md);
        icon_centered(ui, plate, glyph, 18.0, theme.primary);
        title_x = x + PLATE + 12.0;
    }
    // The header holds one line of each: a long one is shortened, a path in
    // its middle, where its folder and its name both stay readable.
    let header_w = bx + BOX_W - PAD - title_x;
    let title = shorten(ui, spec.title, theme.font_size_title, header_w, false);
    text(ui, [title_x, y], &title, theme.font_size_title, theme.text);
    if let Some(subtitle) = spec.subtitle {
        let subtitle = shorten(ui, subtitle, theme.font_size_caption, header_w, true);
        mono(
            ui,
            [title_x, y + theme.font_size_title + 6.0],
            &subtitle,
            theme.font_size_caption,
            theme.text_muted,
        );
    }
    y += header_h + GAP;

    for line_text in &lead {
        text(ui, [x, y], line_text, body, theme.text_dim);
        y += line(body);
    }
    if !lead.is_empty() {
        y += GAP;
    }

    // The facts, label · value, on a sunk plate.
    if !spec.rows.is_empty() {
        fill(ui, [x, y, inner, rows_h], theme.surface, theme.radius_md);
        let mut row_y = y + ROW_PAD;
        for ((label, _), lines) in spec.rows.iter().zip(&values) {
            mono(
                ui,
                [x + ROW_PAD, row_y + 3.0],
                label,
                theme.font_size_caption,
                theme.text_muted,
            );
            let mut line_y = row_y;
            for line_text in lines {
                text(
                    ui,
                    [x + ROW_PAD + LABEL_W, line_y],
                    line_text,
                    body,
                    theme.text,
                );
                line_y += line(body);
            }
            row_y += row_h(lines);
        }
        y += rows_h + GAP;
    }

    for line_text in &note {
        text(ui, [x, y], line_text, caption, theme.text_muted);
        y += line(caption);
    }

    // The answers, right-aligned in the order given.
    let widths: Vec<f32> = (spec.choices.iter())
        .map(|choice| {
            let label = ui.measure_text(choice.label, body, FontFamilyHint::Proportional)[0];
            (label + 28.0).max(BTN_MIN_W)
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + BTN_GAP * widths.len().saturating_sub(1) as f32;
    let mut btn_x = bx + BOX_W - PAD - total;
    let btn_y = by + box_h - PAD - BTN_H;
    let mut chosen = None;
    for (index, (choice, width)) in spec.choices.iter().zip(&widths).enumerate() {
        let hit = button(
            ui,
            theme,
            [btn_x, btn_y, *width, BTN_H],
            &format!("{id_salt}-choice-{index}"),
            Button::new(choice.label, choice.kind),
        );
        if hit.clicked {
            chosen = Some(index);
        }
        btn_x += width + BTN_GAP;
    }
    // A click on the backdrop chooses nothing: it is not an answer.
    let _ = backdrop;
    chosen
}

/// The width of a dialog.
const BOX_W: f32 = 520.0;
/// Its inner margin.
const PAD: f32 = 24.0;
/// The space between its parts.
const GAP: f32 = 14.0;
/// The header's height: the glyph's plate, or the title and its subtitle.
const HEADER_H: f32 = 40.0;
/// The glyph's plate.
const PLATE: f32 = 36.0;
/// One fact's line, and the plate's inner margin.
const ROW_H: f32 = 24.0;
const ROW_PAD: f32 = 12.0;
/// The labels' column.
const LABEL_W: f32 = 96.0;
/// The answers' buttons.
const BTN_H: f32 = 34.0;
const BTN_MIN_W: f32 = 88.0;
const BTN_GAP: f32 = 8.0;

/// `text` cut to fit `width` at `size`, an ellipsis where it was cut: at its
/// end, or in its `middle` (monospace, for a path).
fn shorten(ui: &dyn UiBuilder, text: &str, size: f32, width: f32, middle: bool) -> String {
    let family = if middle {
        FontFamilyHint::Monospace
    } else {
        FontFamilyHint::Proportional
    };
    let fits = |candidate: &str| ui.measure_text(candidate, size, family)[0] <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = if middle {
            let head = keep.div_ceil(2);
            let tail = keep - head;
            (chars[..head].iter())
                .chain(['\u{2026}'].iter())
                .chain(chars[chars.len() - tail..].iter())
                .collect()
        } else {
            chars[..keep].iter().chain(['\u{2026}'].iter()).collect()
        };
        if fits(&candidate) {
            return candidate;
        }
    }
    String::new()
}

/// `text` broken into lines no wider than `width` at `size`, at spaces —
/// in the proportional face. A word wider than `width` stands on its own
/// line.
pub fn wrap_text(ui: &dyn UiBuilder, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ').filter(|word| !word.is_empty()) {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        let wide = ui.measure_text(&candidate, size, FontFamilyHint::Proportional)[0] > width;
        if wide && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{PaintCall, RecordingUiBuilder};
    use khora_core::ui::editor::ui_builder::FontFamilyHint;

    const SCREEN: Rect = [0.0, 0.0, 1000.0, 700.0];
    const TITLE: &str = "Do you trust \u{201c}cave-demo\u{201d}?";

    fn theme() -> UiTheme {
        crate::khora_dark()
    }

    const CHOICES: [DialogChoice<'static>; 3] = [
        DialogChoice {
            label: "Close project",
            kind: ButtonKind::Ghost,
        },
        DialogChoice {
            label: "Open restricted",
            kind: ButtonKind::Ghost,
        },
        DialogChoice {
            label: "Trust and open",
            kind: ButtonKind::Primary,
        },
    ];

    const ROWS: [(&str, &str); 2] = [
        ("Play", "runs its scripts"),
        ("Build Game", "compiles its Cargo.toml"),
    ];

    fn spec<'a>(rows: &'a [(&'a str, &'a str)]) -> Dialog<'a> {
        Dialog {
            icon: Some(Icon::Warn),
            title: TITLE,
            subtitle: Some("D:/games/cave-demo"),
            lead: "A trusted project runs its own code on this machine.",
            rows,
            note: "Opened restricted, it runs nothing.",
            choices: &CHOICES,
        }
    }

    /// One frame of the dialog over `ui`; the choice it returns.
    fn frame(ui: &mut RecordingUiBuilder, rows: &[(&str, &str)]) -> Option<usize> {
        dialog(ui, &theme(), SCREEN, "trust", &spec(rows))
    }

    /// The x of the text run that is exactly `label`.
    fn x_of(ui: &RecordingUiBuilder, label: &str) -> f32 {
        ui.calls()
            .iter()
            .find_map(|call| match call {
                PaintCall::Text { text, pos, .. } if text == label => Some(pos[0]),
                _ => None,
            })
            .unwrap_or_else(|| panic!("`{label}` is painted: {:?}", ui.texts()))
    }

    /// **The dialog paints everything it is given**: the title, the
    /// subtitle in monospace, the lead, each row's label and value, the
    /// note, and each choice's label.
    #[test]
    fn a_dialog_paints_its_title_subtitle_lead_rows_note_and_choices() {
        let mut ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        frame(&mut ui, &ROWS);

        for painted in [
            TITLE,
            "D:/games/cave-demo",
            "A trusted project runs its own code on this machine.",
            "Play",
            "runs its scripts",
            "Build Game",
            "compiles its Cargo.toml",
            "Opened restricted, it runs nothing.",
            "Close project",
            "Open restricted",
            "Trust and open",
        ] {
            assert!(
                ui.painted_text(painted),
                "`{painted}` is painted: {:?}",
                ui.texts()
            );
        }
        assert_eq!(
            ui.text_family("D:/games/cave-demo"),
            Some(FontFamilyHint::Monospace),
            "the subtitle is monospace"
        );
    }

    /// **The choices sit in the order given**, left to right.
    #[test]
    fn a_dialogs_choices_sit_in_the_order_given() {
        let mut ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        frame(&mut ui, &ROWS);

        let close = x_of(&ui, "Close project");
        let restricted = x_of(&ui, "Open restricted");
        let trust = x_of(&ui, "Trust and open");
        assert!(
            close < restricted && restricted < trust,
            "left to right in order: {close} < {restricted} < {trust}"
        );
    }

    /// **A click on a choice returns its index**, each choice its own;
    /// nothing clicked returns `None`.
    #[test]
    fn a_click_on_a_choice_returns_its_index() {
        for index in 0..CHOICES.len() {
            let mut ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3])
                .with_click(&format!("trust-choice-{index}"));
            assert_eq!(frame(&mut ui, &ROWS), Some(index), "choice {index}");
        }
        let mut ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        assert_eq!(frame(&mut ui, &ROWS), None, "nothing clicked");
        assert!(
            ui.interact_ids().contains(&"trust-backdrop"),
            "the backdrop takes clicks: {:?}",
            ui.interact_ids()
        );
    }

    /// **A click on the backdrop chooses nothing.**
    #[test]
    fn a_click_on_the_backdrop_chooses_nothing() {
        let mut ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]).with_click("trust-backdrop");
        assert_eq!(frame(&mut ui, &ROWS), None);
    }

    /// **No rows, no plate**: the sunk plate the rows sit on is not drawn
    /// when there are none — the dialog with rows paints more filled
    /// rectangles than the same dialog without.
    #[test]
    fn a_dialog_without_rows_draws_no_plate() {
        let mut with_rows = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        frame(&mut with_rows, &ROWS);
        let mut without = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        frame(&mut without, &[]);

        assert!(
            without.rects_filled().len() < with_rows.rects_filled().len(),
            "the plate is drawn only for rows: {} without, {} with",
            without.rects_filled().len(),
            with_rows.rects_filled().len()
        );
        assert!(!without.painted_text("runs its scripts"));
    }

    /// **Text wraps at spaces, within its width**: at 10 px a glyph is
    /// 5.5 px wide, so 60 px hold ten; a word wider than the width stands
    /// alone, never split.
    #[test]
    fn text_wraps_at_spaces_within_its_width() {
        let ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        assert_eq!(
            wrap_text(&ui, "aaaa bbbb  cccc dddddddddddddd e", 10.0, 60.0),
            vec!["aaaa bbbb", "cccc", "dddddddddddddd", "e"]
        );
        assert!(wrap_text(&ui, "   ", 10.0, 60.0).is_empty());
    }

    /// **A header line too long is shortened**: a title at its end, a path
    /// in its middle, each within the width; one that fits is kept whole.
    #[test]
    fn a_long_header_line_is_shortened_within_its_width() {
        let ui = RecordingUiBuilder::new(SCREEN[2], SCREEN[3]);
        assert_eq!(shorten(&ui, "short", 10.0, 60.0, false), "short");
        assert_eq!(
            shorten(&ui, "abcdefghijklmnop", 10.0, 60.0, false),
            "abcdefghi\u{2026}"
        );
        assert_eq!(
            shorten(&ui, "C:/projects/ember", 10.0, 60.0, true),
            "C:/pr\u{2026}mber"
        );
    }
}
