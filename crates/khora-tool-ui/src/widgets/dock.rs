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

//! Dock chrome — the tab strips, dividers and drop hints that make
//! [`DockTree`](khora_core::ui::editor::dock::DockTree) touchable.
//!
//! Painting only: the geometry comes from a `DockLayout`, and every gesture is
//! reported back to the caller, which owns the drag state and applies the
//! change to the tree. Splitting the two means the rules stay unit-testable
//! without a window, and this file stays about how the dock *looks*.
//!
//! There is deliberately **no close button** yet: removing a panel is only safe
//! once there is a way to bring it back, and the View menu that would do that
//! does not exist. An affordance that loses a panel for good is worse than no
//! affordance.

use khora_core::ui::editor::dock::{
    ratio_from_pointer, DockRect, DropZone, SplitAxis, SplitterLayout, TabGroupLayout,
};
use khora_core::ui::{UiBuilder, UiTheme};

use khora_core::ui::editor::ui_builder::FontFamilyHint;

use super::nav::panel_tab;
use super::paint::{fill, hairline_h, tint, with_alpha};
use super::Rect;

/// Width a tab needs for `label`, measured through the backend so the strip
/// packs correctly whatever font pack is installed.
fn tab_width(ui: &mut dyn UiBuilder, label: &str, size: f32) -> f32 {
    ui.measure_text(label, size, FontFamilyHint::Proportional)[0] + 26.0
}

/// Marks a drag payload as "a dock tab is moving".
///
/// The payload carries no identity: the panel being dragged is held by the
/// caller for the whole gesture, so nothing here can go stale the way an index
/// into a rebuilt list would.
pub const DOCK_DRAG_TAG: u64 = 0x4B44_434B_0000_0001; // "KDCK"

/// Height of the tab strip above every group's content.
pub const TAB_STRIP_H: f32 = 26.0;

/// What the user did to a tab strip this frame. Indices are into
/// [`TabGroupLayout::panels`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabStripEvent {
    /// A tab was clicked and should become the visible one.
    pub activated: Option<usize>,
    /// A tab started being dragged out of the strip.
    pub drag_started: Option<usize>,
}

/// Paints a group's tab strip and reports clicks and drag starts.
///
/// A single-tab strip is still drawn: it is the panel's title *and* its drag
/// handle, so hiding it would leave a lone panel unmovable.
pub fn dock_tab_strip(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    group: &TabGroupLayout,
    id_salt: &str,
) -> TabStripEvent {
    let [x, y, w, _] = group.rect;
    let strip: Rect = [x, y, w, TAB_STRIP_H];
    fill(ui, strip, theme.surface, 0.0);
    hairline_h(ui, theme, x, x + w, y + TAB_STRIP_H);

    let mut event = TabStripEvent::default();
    let mut tx = x + 4.0;
    let size = theme.font_size_caption + 1.0;

    for (i, panel) in group.panels.iter().enumerate() {
        let label = short_title(panel);
        let tab_w = tab_width(ui, &label, size);
        if tx + tab_w > x + w {
            break;
        }
        let rect: Rect = [tx, y + 3.0, tab_w, TAB_STRIP_H - 5.0];
        let active = i == group.active;

        let out = panel_tab(
            ui,
            theme,
            rect,
            &format!("{id_salt}-tab-{i}"),
            None,
            &label,
            active,
        );

        // The active tab gets a gold underline rather than a filled plate:
        // selection is gold everywhere in the editor, and a plate here would
        // compete with the panel content right below it.
        if active {
            fill(
                ui,
                [rect[0] + 4.0, y + TAB_STRIP_H - 2.0, rect[2] - 8.0, 2.0],
                theme.accent_c,
                0.0,
            );
        }

        if out.clicked {
            event.activated = Some(i);
        }
        if ui.is_last_item_dragged() {
            ui.dnd_attach_drag_payload(DOCK_DRAG_TAG);
            event.drag_started = Some(i);
        }

        tx += tab_w + 2.0;
    }

    event
}

/// Paints a divider and, while it is being dragged, returns the ratio the
/// pointer implies.
///
/// The ratio is derived by [`ratio_from_pointer`] — the inverse of the same
/// geometry the layout used — so the divider cannot drift away from the cursor.
pub fn dock_splitter(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    splitter: &SplitterLayout,
    id_salt: &str,
) -> Option<f32> {
    let out = ui.interact_rect(id_salt, splitter.rect);

    // At rest the divider is just the gap between panels; it only draws when
    // the pointer is on it, so the workbench stays quiet.
    if out.hovered || out.pressed {
        let colour = if out.pressed {
            theme.accent_c
        } else {
            theme.border_strong
        };
        let r = splitter.rect;
        let bar = match splitter.axis {
            SplitAxis::Horizontal => [r[0] + r[2] * 0.5 - 1.0, r[1] + 4.0, 2.0, r[3] - 8.0],
            SplitAxis::Vertical => [r[0] + 4.0, r[1] + r[3] * 0.5 - 1.0, r[2] - 8.0, 2.0],
        };
        fill(ui, bar, colour, 1.0);
    }

    if out.pressed {
        return ui
            .pointer_position()
            .map(|p| ratio_from_pointer(splitter, p));
    }
    None
}

/// Paints the half (or whole, for [`DropZone::Center`]) of `rect` a drop would
/// land in.
///
/// Showing the resulting area rather than an arrow answers the question the
/// user actually has — *where will it go* — before they commit.
pub fn dock_drop_overlay(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: DockRect, zone: DropZone) {
    let [x, y, w, h] = rect;
    let target: Rect = match zone {
        DropZone::Center => [x, y, w, h],
        DropZone::Left => [x, y, w * 0.5, h],
        DropZone::Right => [x + w * 0.5, y, w * 0.5, h],
        DropZone::Top => [x, y, w, h * 0.5],
        DropZone::Bottom => [x, y + h * 0.5, w, h * 0.5],
    };
    fill(
        ui,
        target,
        with_alpha(theme.accent_c, 0.16),
        theme.radius_sm,
    );
    super::paint::stroke(ui, target, tint(theme.accent_c, 0.7), theme.radius_sm, 2.0);
}

/// Paints the label of the panel being dragged, following the cursor.
pub fn dock_drag_ghost(ui: &mut dyn UiBuilder, theme: &UiTheme, label: &str, pointer: [f32; 2]) {
    let size = theme.font_size_caption + 1.0;
    let w = tab_width(ui, label, size);
    let rect: Rect = [pointer[0] + 10.0, pointer[1] + 10.0, w, 22.0];
    super::paint::fill_stroke(
        ui,
        rect,
        theme.surface_elevated,
        theme.accent_c,
        theme.radius_sm,
    );
    super::paint::text(
        ui,
        [rect[0] + 12.0, super::text_y(rect, size)],
        label,
        size,
        theme.text,
    );
}

/// Trims a panel id down to what belongs on a tab.
///
/// Panel ids are namespaced (`khora.editor.scene_tree`) because they key the
/// saved layout; the tab shows the last segment, title-cased, so the strip
/// reads as words instead of paths.
fn short_title(panel_id: &str) -> String {
    let tail = panel_id.rsplit('.').next().unwrap_or(panel_id);
    let mut out = String::with_capacity(tail.len());
    let mut capitalise = true;
    for ch in tail.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            capitalise = true;
        } else if capitalise {
            out.extend(ch.to_uppercase());
            capitalise = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingUiBuilder;
    use khora_core::ui::editor::dock::{DockTree, DropZone};

    fn theme() -> UiTheme {
        crate::khora_dark()
    }

    fn group(panels: &[&str], active: usize) -> TabGroupLayout {
        TabGroupLayout {
            rect: [0.0, 0.0, 400.0, 300.0],
            panels: panels.iter().map(|s| (*s).to_owned()).collect(),
            active,
        }
    }

    /// Every tab is labelled, so a group of three is legible without hovering.
    #[test]
    fn tab_strip_labels_every_tab() {
        let mut ui = RecordingUiBuilder::new(400.0, 300.0);
        dock_tab_strip(
            &mut ui,
            &theme(),
            &group(&["khora.editor.console", "khora.editor.assets"], 0),
            "g0",
        );
        assert!(ui.painted_text("Console"));
        assert!(ui.painted_text("Assets"));
    }

    /// A lone panel keeps its strip: it is the drag handle, so hiding it would
    /// make that panel the one thing in the dock the user cannot move.
    #[test]
    fn single_tab_still_draws_a_strip() {
        let mut ui = RecordingUiBuilder::new(400.0, 300.0);
        dock_tab_strip(
            &mut ui,
            &theme(),
            &group(&["khora.editor.viewport"], 0),
            "g0",
        );
        assert!(ui.painted_text("Viewport"));
    }

    /// Ids are namespaced for the saved layout; tabs show words.
    #[test]
    fn short_title_reads_as_words() {
        assert_eq!(short_title("khora.editor.scene_tree"), "Scene Tree");
        assert_eq!(short_title("khora.editor.control_plane"), "Control Plane");
        assert_eq!(short_title("viewport"), "Viewport");
    }

    /// Widths of every filled rect, so a test can assert on the previewed area.
    fn filled_widths(ui: &RecordingUiBuilder) -> Vec<f32> {
        ui.rects_filled()
            .into_iter()
            .filter_map(|c| match c {
                crate::testing::PaintCall::RectFilled { size, .. } => Some(size[0]),
                _ => None,
            })
            .collect()
    }

    /// The overlay shows the area the panel will occupy — a left drop must not
    /// highlight the whole pane, or the preview tells the user nothing.
    #[test]
    fn drop_overlay_covers_only_the_target_half() {
        let mut ui = RecordingUiBuilder::new(400.0, 300.0);
        dock_drop_overlay(&mut ui, &theme(), [0.0, 0.0, 400.0, 300.0], DropZone::Left);
        let widths = filled_widths(&ui);
        assert!(
            widths.iter().any(|w| (w - 200.0).abs() < 0.01),
            "left drop should preview half the width, got {widths:?}"
        );
        assert!(
            !widths.iter().any(|w| (w - 400.0).abs() < 0.01),
            "and must not also highlight the whole pane"
        );
    }

    #[test]
    fn centre_drop_overlay_covers_the_whole_group() {
        let mut ui = RecordingUiBuilder::new(400.0, 300.0);
        dock_drop_overlay(
            &mut ui,
            &theme(),
            [0.0, 0.0, 400.0, 300.0],
            DropZone::Center,
        );
        assert!(filled_widths(&ui).iter().any(|w| (w - 400.0).abs() < 0.01));
    }

    /// A splitter at rest paints nothing: the dock should read as panels with
    /// gaps, not as a grid of bars.
    #[test]
    fn splitter_is_invisible_until_touched() {
        let tree = {
            let mut t = DockTree::single("a");
            t.insert("b", Some("a"), DropZone::Right);
            t
        };
        let sp = tree.layout([0.0, 0.0, 400.0, 300.0]).splitters[0].clone();

        let mut ui = RecordingUiBuilder::new(400.0, 300.0);
        let before = ui.rects_filled().len();
        let ratio = dock_splitter(&mut ui, &theme(), &sp, "sp0");
        assert_eq!(ui.rects_filled().len(), before, "nothing painted at rest");
        assert!(ratio.is_none(), "no drag reported without a press");
    }
}
