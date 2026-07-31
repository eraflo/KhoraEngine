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

//! Disclosure — showing and hiding structure.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction, TextAlign};
use khora_core::ui::editor::Icon;
use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill, icon, text, tint};
use super::{text_y, Rect};

/// What a collapsible group header shows.
#[derive(Debug, Clone, Copy)]
pub struct Group<'a> {
    /// Icon for the component's domain.
    pub glyph: Icon,
    /// The component name.
    pub name: &'a str,
    /// Right-aligned category tag (rendered uppercase); empty to omit.
    pub category: &'a str,
    /// Whether the group is currently expanded.
    pub open: bool,
}

impl<'a> Group<'a> {
    /// An expanded group with no category tag.
    pub fn new(name: &'a str, glyph: Icon) -> Self {
        Self {
            glyph,
            name,
            category: "",
            open: true,
        }
    }

    /// Adds the right-aligned category tag.
    pub fn category(mut self, category: &'a str) -> Self {
        self.category = category;
        self
    }

    /// Sets the expanded state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
}

/// A collapsible group header — the inspector's component headers.
///
/// Deliberately **flat**: a chevron, an icon, a name, and a right-aligned
/// category tag, over a hover plate. No card, no border, no nesting. A dense
/// inspector with a bordered box per component turns into boxes-in-boxes, and
/// the structure stops being readable exactly when there is enough of it to
/// need reading. Hierarchy comes from type and spacing instead.
///
/// The caller owns the open/closed set; this just reports the click.
pub fn group_header(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    rect: Rect,
    id_salt: &str,
    spec: Group<'_>,
) -> Interaction {
    let Group {
        glyph,
        name,
        category,
        open,
    } = spec;

    let out = ui.interact_rect(id_salt, rect);

    if out.hovered {
        fill(ui, rect, tint(theme.surface_interactive, 0.35), 0.0);
    }

    let size = theme.font_size_body;
    let y = text_y(rect, size);
    let mut x = rect[0] + 12.0;

    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icon(ui, [x, y], chevron, size + 2.0, theme.text_disabled);
    x += size + 9.0;

    icon(ui, [x, y], glyph, size + 2.0, theme.primary_dim);
    x += size + 9.0;

    text(ui, [x, y], name, size, theme.text);

    if !category.is_empty() {
        let csize = theme.font_size_caption - 1.5;
        ui.paint_text_styled(
            [super::right(rect) - 12.0, text_y(rect, csize)],
            &category.to_uppercase(),
            csize,
            theme.text_disabled,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brand::khora_dark;
    use crate::testing::RecordingUiBuilder;

    #[test]
    fn header_is_flat_no_card_no_border() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 40.0);
        group_header(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 32.0],
            "transform",
            Group::new("Transform", Icon::Move).category("spatial"),
        );
        // No hover, so nothing should be filled, and nothing is ever stroked.
        assert!(
            ui.rects_filled().is_empty(),
            "an idle group header must paint no plate"
        );
        assert!(
            !ui.calls()
                .iter()
                .any(|c| matches!(c, crate::testing::PaintCall::RectStroke { .. })),
            "component groups are flat — no boxes-in-boxes"
        );
    }

    #[test]
    fn chevron_direction_follows_the_open_state() {
        let t = khora_dark();
        let spec = Group::new("Transform", Icon::Move);

        let mut open = RecordingUiBuilder::new(300.0, 40.0);
        group_header(&mut open, &t, [0.0, 0.0, 280.0, 32.0], "g", spec);
        assert!(open.painted_text(Icon::ChevronDown.glyph()));

        let mut shut = RecordingUiBuilder::new(300.0, 40.0);
        group_header(
            &mut shut,
            &t,
            [0.0, 0.0, 280.0, 32.0],
            "g",
            spec.open(false),
        );
        assert!(shut.painted_text(Icon::ChevronRight.glyph()));
    }

    #[test]
    fn category_tag_is_uppercased_monospace() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 40.0);
        group_header(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 32.0],
            "g",
            Group::new("Transform", Icon::Move).category("spatial"),
        );
        assert!(ui.painted_text("SPATIAL"));
        assert_eq!(ui.text_family("SPATIAL"), Some(FontFamilyHint::Monospace));
    }

    #[test]
    fn header_reports_clicks_so_the_caller_can_toggle() {
        let t = khora_dark();
        let mut ui = RecordingUiBuilder::new(300.0, 40.0).with_click("transform");
        let out = group_header(
            &mut ui,
            &t,
            [0.0, 0.0, 280.0, 32.0],
            "transform",
            Group::new("Transform", Icon::Move),
        );
        assert!(out.clicked);
    }
}
