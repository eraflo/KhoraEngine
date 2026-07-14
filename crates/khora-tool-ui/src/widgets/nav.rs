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

//! Navigation — how the user moves between things.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill, fill_stroke, icon, selection_bar, text, text_centered, tint};
use super::{text_y, Rect};

/// A segmented control — mutually exclusive views of the *same* thing
/// (Properties | Debug). Not for navigation between different things; that's
/// [`nav_item`].
///
/// Returns `Some(index)` when a segment was clicked.
pub fn segmented_tabs(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    labels: &[&str],
    active: usize,
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    fill_stroke(ui, rect, theme.background, theme.border, theme.radius_md);

    let pad = 2.0;
    let inner = super::shrink(rect, pad);
    let seg_w = inner[2] / labels.len() as f32;
    let size = theme.font_size_caption + 1.0;

    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let seg: Rect = [inner[0] + i as f32 * seg_w, inner[1], seg_w, inner[3]];
        let out = ui.interact_rect(&format!("{id_salt}:{i}"), seg);
        if out.clicked {
            clicked = Some(i);
        }

        let is_active = i == active;
        if is_active {
            fill(ui, seg, theme.surface_interactive, theme.radius_sm);
        }
        let fg = if is_active {
            theme.text
        } else if out.hovered {
            theme.text_dim
        } else {
            theme.text_muted
        };
        text_centered(ui, seg, label, size, fg, FontFamilyHint::Proportional);
    }
    clicked
}

/// A sidebar / spine navigation entry — icon, label, brand-tinted when active.
///
/// This is for switching *place* (Recent / Engines / Settings). The active
/// state is a brand tint, not the gold selection colour: gold is reserved for
/// "the thing you have selected", never "where you are".
pub fn nav_item(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    glyph: Icon,
    label: &str,
    active: bool,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    if active {
        fill(ui, rect, tint(theme.primary, 0.14), theme.radius_md);
    } else if out.hovered {
        fill(
            ui,
            rect,
            tint(theme.surface_interactive, 0.5),
            theme.radius_md,
        );
    }

    let icon_size = theme.font_size_title + 1.0;
    let fg_icon = if active {
        theme.primary
    } else {
        theme.text_muted
    };
    let fg_text = if active { theme.text } else { theme.text_dim };

    icon(
        ui,
        [rect[0] + 10.0, text_y(rect, icon_size)],
        glyph,
        icon_size,
        fg_icon,
    );
    let size = theme.font_size_body;
    text(
        ui,
        [rect[0] + 10.0 + icon_size + 9.0, text_y(rect, size)],
        label,
        size,
        fg_text,
    );

    out
}

/// A dock tab (Console | Assets | GORNA stream). The active tab is lifted onto
/// the panel background so it reads as connected to the content below it.
pub fn panel_tab(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    glyph: Option<Icon>,
    label: &str,
    active: bool,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    if active {
        fill(ui, rect, theme.background, theme.radius_sm);
    }
    let fg = if active {
        theme.text
    } else if out.hovered {
        theme.text_dim
    } else {
        theme.text_muted
    };

    let size = theme.font_size_caption + 1.0;
    let mut x = rect[0] + 11.0;
    if let Some(g) = glyph {
        icon(ui, [x, text_y(rect, size)], g, size, fg);
        x += size + 6.0;
    }
    text(ui, [x, text_y(rect, size)], label, size, fg);

    out
}

/// A breadcrumb trail. The last crumb is the current location and renders in
/// the primary ink; the rest are muted and separated by chevrons.
pub fn breadcrumb(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, crumbs: &[&str]) {
    let size = theme.font_size_caption + 1.0;
    let y = text_y(rect, size);
    let mut x = rect[0];

    for (i, crumb) in crumbs.iter().enumerate() {
        let last = i + 1 == crumbs.len();
        let fg = if last { theme.text } else { theme.text_muted };
        text(ui, [x, y], crumb, size, fg);
        x += ui.measure_text(crumb, size, FontFamilyHint::Proportional)[0] + 6.0;

        if !last {
            icon(ui, [x, y], Icon::ChevronRight, size, theme.text_disabled);
            x += size + 6.0;
        }
    }
}

/// Where a step sits in a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Already completed.
    Done,
    /// The step being performed now.
    Current,
    /// Not reached yet.
    Pending,
}

/// A vertical step rail — "what happens when I press Create". Steps are
/// numbered only because they *are* a sequence; the number carries real
/// ordering information.
pub fn step_rail(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    steps: &[(&str, &str, StepState)],
) {
    let row_h = 34.0;
    let dot_r = 10.0;

    for (i, (title, detail, state)) in steps.iter().enumerate() {
        let y = rect[1] + i as f32 * row_h;
        let cy = y + dot_r;
        let cx = rect[0] + dot_r;

        match state {
            StepState::Done => {
                ui.paint_circle_filled([cx, cy], dot_r, theme.success);
                super::paint::icon_centered(
                    ui,
                    [cx - dot_r, cy - dot_r, dot_r * 2.0, dot_r * 2.0],
                    Icon::Check,
                    11.0,
                    theme.text_inverse,
                );
            }
            StepState::Current => {
                ui.paint_circle_stroke([cx, cy], dot_r, theme.primary, 1.0);
                super::paint::text_centered(
                    ui,
                    [cx - dot_r, cy - dot_r, dot_r * 2.0, dot_r * 2.0],
                    &(i + 1).to_string(),
                    theme.font_size_caption,
                    theme.primary,
                    FontFamilyHint::Monospace,
                );
            }
            StepState::Pending => {
                ui.paint_circle_stroke([cx, cy], dot_r, theme.border_strong, 1.0);
                super::paint::text_centered(
                    ui,
                    [cx - dot_r, cy - dot_r, dot_r * 2.0, dot_r * 2.0],
                    &(i + 1).to_string(),
                    theme.font_size_caption,
                    theme.text_muted,
                    FontFamilyHint::Monospace,
                );
            }
        }

        let tx = cx + dot_r + 10.0;
        let fg_title = if *state == StepState::Pending {
            theme.text_muted
        } else {
            theme.text_dim
        };
        text(ui, [tx, y], title, theme.font_size_body, fg_title);
        text(
            ui,
            [tx, y + theme.font_size_body + 2.0],
            detail,
            theme.font_size_caption,
            theme.text_muted,
        );
    }
}

/// A selectable card — the engine-version picker. A radio in card clothing:
/// exactly one of a set, but with room for metadata the plain control has no
/// space for.
pub fn radio_card(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    label: &str,
    meta: &str,
    selected: bool,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);

    let (bg, border) = if selected {
        (tint(theme.primary, 0.08), theme.primary)
    } else if out.hovered {
        (theme.surface, theme.border_strong)
    } else {
        ([0.0; 4], theme.border)
    };
    fill_stroke(ui, rect, bg, border, theme.radius_md);

    // The radio itself.
    let cx = rect[0] + 12.0 + 7.5;
    let cy = rect[1] + rect[3] * 0.5;
    ui.paint_circle_stroke(
        [cx, cy],
        7.5,
        if selected {
            theme.primary
        } else {
            theme.border_strong
        },
        1.0,
    );
    if selected {
        ui.paint_circle_filled([cx, cy], 3.5, theme.primary);
    }

    let size = theme.font_size_body;
    text(
        ui,
        [cx + 7.5 + 11.0, text_y(rect, size)],
        label,
        size,
        theme.text,
    );

    if !meta.is_empty() {
        let msize = theme.font_size_caption;
        let w = ui.measure_text(meta, msize, FontFamilyHint::Monospace)[0];
        super::paint::mono(
            ui,
            [super::right(rect) - 12.0 - w, text_y(rect, msize)],
            meta,
            msize,
            theme.text_muted,
        );
    }

    out
}

/// A selectable row with a gold selection bar — hierarchy nodes, palette rows.
///
/// Gold is the selection colour and *only* the selection colour; that is what
/// makes "what am I acting on" instantly legible anywhere in the tools.
pub fn selectable_row(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    selected: bool,
) -> Interaction {
    let out = ui.interact_rect(id_salt, rect);
    if selected {
        fill(ui, rect, theme.surface_active, theme.radius_sm);
        selection_bar(ui, rect, theme.accent_c);
    } else if out.hovered {
        fill(ui, rect, tint(theme.surface_elevated, 0.6), theme.radius_sm);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn segmented_tabs_fill_only_the_active_segment() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0);
        segmented_tabs(
            &mut ui,
            &t,
            [0.0, 0.0, 180.0, 24.0],
            "seg",
            &["Properties", "Debug"],
            0,
        );

        let active_fills = ui
            .rects_filled()
            .into_iter()
            .filter(|c| matches!(c, crate::testing::PaintCall::RectFilled { color, .. } if *color == t.surface_interactive))
            .count();
        assert_eq!(active_fills, 1, "exactly one segment may be filled");

        assert_eq!(
            ui.text_color("Properties"),
            Some(t.text),
            "active tab is primary ink"
        );
        assert_eq!(
            ui.text_color("Debug"),
            Some(t.text_muted),
            "inactive tab is muted"
        );
    }

    #[test]
    fn segmented_tabs_report_the_clicked_index() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0).with_click("seg:1");
        let hit = segmented_tabs(
            &mut ui,
            &t,
            [0.0, 0.0, 180.0, 24.0],
            "seg",
            &["Properties", "Debug"],
            0,
        );
        assert_eq!(hit, Some(1));
    }

    #[test]
    fn segmented_tabs_handle_an_empty_label_set() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0);
        assert_eq!(
            segmented_tabs(&mut ui, &t, [0.0, 0.0, 180.0, 24.0], "seg", &[], 0),
            None
        );
    }

    #[test]
    fn active_nav_item_tints_with_brand_never_with_gold() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 40.0);
        nav_item(
            &mut ui,
            &t,
            [0.0, 0.0, 180.0, 32.0],
            "recent",
            Icon::Layers,
            "Recent",
            true,
        );

        assert_eq!(ui.text_color("Recent"), Some(t.text));
        assert!(
            !ui.used_fill(t.accent_c),
            "gold is the selection color; navigation must not use it"
        );
    }

    #[test]
    fn selected_row_gets_a_gold_selection_bar() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0);
        selectable_row(&mut ui, &t, [0.0, 0.0, 180.0, 26.0], "e3", true);
        assert!(
            ui.used_fill(t.accent_c),
            "the selected row must carry the gold bar"
        );
        assert!(ui.used_fill(t.surface_active));
    }

    #[test]
    fn unselected_row_paints_no_gold() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(200.0, 30.0);
        selectable_row(&mut ui, &t, [0.0, 0.0, 180.0, 26.0], "e3", false);
        assert!(!ui.used_fill(t.accent_c));
    }

    #[test]
    fn breadcrumb_emphasizes_only_the_last_crumb() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 30.0);
        breadcrumb(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 20.0],
            &["assets", "textures"],
        );
        assert_eq!(ui.text_color("assets"), Some(t.text_muted));
        assert_eq!(ui.text_color("textures"), Some(t.text));
    }

    #[test]
    fn radio_card_shows_its_dot_only_when_selected() {
        let t = khora_dark();

        let mut on = RecordingUiBuilder::new(300.0, 40.0);
        radio_card(
            &mut on,
            &t,
            [0.0, 0.0, 280.0, 36.0],
            "v070",
            "0.7.0",
            "installed",
            true,
        );
        assert_eq!(on.circles_filled().len(), 1, "selected → inner dot painted");

        let mut off = RecordingUiBuilder::new(300.0, 40.0);
        radio_card(
            &mut off,
            &t,
            [0.0, 0.0, 280.0, 36.0],
            "v070",
            "0.7.0",
            "installed",
            false,
        );
        assert!(off.circles_filled().is_empty(), "unselected → ring only");
    }

    #[test]
    fn step_rail_marks_done_current_and_pending_distinctly() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(280.0, 160.0);
        step_rail(
            &mut ui,
            &t,
            [0.0, 0.0, 260.0, 140.0],
            &[
                ("Scaffold", "project files", StepState::Done),
                ("git init", "+ remote", StepState::Current),
                ("Open", "in the editor", StepState::Pending),
            ],
        );
        // Done steps get a filled dot; current + pending are stroked rings.
        assert_eq!(ui.circles_filled().len(), 1);
        assert!(ui.painted_text("Scaffold"));
        assert!(ui.painted_text("git init"));
    }
}
