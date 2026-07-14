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

//! A headless [`UiBuilder`] that records what a widget painted.
//!
//! Widgets in this crate are pure functions of `(&mut dyn UiBuilder, &UiTheme,
//! data)`, which makes them testable without a window, a GPU, or egui: run the
//! widget against a [`RecordingUiBuilder`] and assert on the resulting
//! [`PaintCall`] list.
//!
//! Assert on **semantics** — "the active tab is filled with `surface_active`",
//! "the disabled button ignored the click", "the X field is tinted `axis_x`" —
//! not on exact pixel coordinates. Layout maths is the backend's business and
//! pinning it down makes the tests brittle without making them meaningful.
//!
//! ```
//! use khora_tool_ui::{brand, testing::RecordingUiBuilder, widgets};
//!
//! let theme = brand::khora_dark();
//! let mut ui = RecordingUiBuilder::new(200.0, 40.0);
//! widgets::status_dot(&mut ui, &theme, [10.0, 10.0], widgets::Health::Good);
//!
//! assert_eq!(ui.circles_filled().len(), 1);
//! ```

use std::collections::HashMap;

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction, TextAlign};
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::editor::UiBuilder;

/// One recorded paint or interaction call, in the order the widget made it.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintCall {
    /// A filled rectangle.
    RectFilled {
        /// Top-left corner.
        min: [f32; 2],
        /// Width / height.
        size: [f32; 2],
        /// Fill color.
        color: [f32; 4],
        /// Corner radius.
        rounding: f32,
    },
    /// A stroked (outlined) rectangle.
    RectStroke {
        /// Top-left corner.
        min: [f32; 2],
        /// Width / height.
        size: [f32; 2],
        /// Stroke color.
        color: [f32; 4],
        /// Corner radius.
        rounding: f32,
        /// Stroke width.
        thickness: f32,
    },
    /// A straight line.
    Line {
        /// Start point.
        from: [f32; 2],
        /// End point.
        to: [f32; 2],
        /// Stroke color.
        color: [f32; 4],
        /// Stroke width.
        thickness: f32,
    },
    /// A filled circle.
    CircleFilled {
        /// Centre point.
        center: [f32; 2],
        /// Radius.
        radius: f32,
        /// Fill color.
        color: [f32; 4],
    },
    /// A stroked circle.
    CircleStroke {
        /// Centre point.
        center: [f32; 2],
        /// Radius.
        radius: f32,
        /// Stroke color.
        color: [f32; 4],
        /// Stroke width.
        thickness: f32,
    },
    /// A run of text.
    Text {
        /// Anchor position.
        pos: [f32; 2],
        /// The string painted.
        text: String,
        /// Font size in points.
        size: f32,
        /// Text color.
        color: [f32; 4],
        /// Which font family was requested.
        family: FontFamilyHint,
        /// Horizontal anchoring.
        align: TextAlign,
    },
    /// A filled polygon.
    Path {
        /// Polygon vertices.
        points: Vec<[f32; 2]>,
        /// Fill color.
        color: [f32; 4],
    },
    /// A hot region allocated for interaction.
    Interact {
        /// The disambiguating salt the widget passed.
        id_salt: String,
        /// The rect `[x, y, w, h]` made interactive.
        rect: [f32; 4],
    },
}

/// A [`UiBuilder`] that records paint calls instead of rendering, and can be
/// scripted to report interactions.
///
/// Interactions are keyed by the `id_salt` a widget passes to
/// [`UiBuilder::interact_rect`], so a test can "click" a specific control
/// without simulating a pointer.
#[derive(Debug, Default)]
pub struct RecordingUiBuilder {
    calls: Vec<PaintCall>,
    scripted: HashMap<String, Interaction>,
    panel: [f32; 4],
    cursor: [f32; 2],
}

impl RecordingUiBuilder {
    /// Creates a recorder over a virtual panel of `width` × `height`, anchored
    /// at the origin.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            calls: Vec::new(),
            scripted: HashMap::new(),
            panel: [0.0, 0.0, width, height],
            cursor: [0.0, 0.0],
        }
    }

    /// Scripts the interaction that [`UiBuilder::interact_rect`] will report
    /// for the region with this `id_salt`. Chainable.
    pub fn with_interaction(mut self, id_salt: &str, interaction: Interaction) -> Self {
        self.scripted.insert(id_salt.to_owned(), interaction);
        self
    }

    /// Scripts a plain click (hovered + clicked) on the region with this
    /// `id_salt`. Chainable.
    pub fn with_click(self, id_salt: &str) -> Self {
        self.with_interaction(
            id_salt,
            Interaction {
                hovered: true,
                clicked: true,
                pressed: false,
                double_clicked: false,
            },
        )
    }

    /// Scripts a hover (no click) on the region with this `id_salt`. Chainable.
    pub fn with_hover(self, id_salt: &str) -> Self {
        self.with_interaction(
            id_salt,
            Interaction {
                hovered: true,
                clicked: false,
                pressed: false,
                double_clicked: false,
            },
        )
    }

    /// Every recorded call, in paint order.
    pub fn calls(&self) -> &[PaintCall] {
        &self.calls
    }

    /// All filled rectangles, in paint order.
    pub fn rects_filled(&self) -> Vec<&PaintCall> {
        self.calls
            .iter()
            .filter(|c| matches!(c, PaintCall::RectFilled { .. }))
            .collect()
    }

    /// All filled circles, in paint order.
    pub fn circles_filled(&self) -> Vec<&PaintCall> {
        self.calls
            .iter()
            .filter(|c| matches!(c, PaintCall::CircleFilled { .. }))
            .collect()
    }

    /// Every string painted, in paint order.
    pub fn texts(&self) -> Vec<&str> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                PaintCall::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// `true` if any painted string contains `needle`.
    pub fn painted_text(&self, needle: &str) -> bool {
        self.texts().iter().any(|t| t.contains(needle))
    }

    /// The color of the text run whose content contains `needle`, if any.
    pub fn text_color(&self, needle: &str) -> Option<[f32; 4]> {
        self.calls.iter().find_map(|c| match c {
            PaintCall::Text { text, color, .. } if text.contains(needle) => Some(*color),
            _ => None,
        })
    }

    /// The font family of the text run whose content contains `needle`.
    pub fn text_family(&self, needle: &str) -> Option<FontFamilyHint> {
        self.calls.iter().find_map(|c| match c {
            PaintCall::Text { text, family, .. } if text.contains(needle) => Some(*family),
            _ => None,
        })
    }

    /// Every fill color used by a filled rect or circle, in paint order.
    pub fn fill_colors(&self) -> Vec<[f32; 4]> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                PaintCall::RectFilled { color, .. } | PaintCall::CircleFilled { color, .. } => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    /// `true` if some filled rect or circle used exactly this color.
    pub fn used_fill(&self, color: [f32; 4]) -> bool {
        self.fill_colors().contains(&color)
    }

    /// The `id_salt`s of every interactive region the widget allocated.
    pub fn interact_ids(&self) -> Vec<&str> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                PaintCall::Interact { id_salt, .. } => Some(id_salt.as_str()),
                _ => None,
            })
            .collect()
    }

    fn record(&mut self, call: PaintCall) {
        self.calls.push(call);
    }
}

impl UiBuilder for RecordingUiBuilder {
    // ── Paint primitives — the part widgets actually use ──

    fn paint_rect_filled(&mut self, min: [f32; 2], size: [f32; 2], color: [f32; 4], rounding: f32) {
        self.record(PaintCall::RectFilled {
            min,
            size,
            color,
            rounding,
        });
    }

    fn paint_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        self.record(PaintCall::RectStroke {
            min,
            size,
            color,
            rounding,
            thickness,
        });
    }

    fn paint_line(&mut self, from: [f32; 2], to: [f32; 2], color: [f32; 4], thickness: f32) {
        self.record(PaintCall::Line {
            from,
            to,
            color,
            thickness,
        });
    }

    fn paint_circle_filled(&mut self, center: [f32; 2], radius: f32, color: [f32; 4]) {
        self.record(PaintCall::CircleFilled {
            center,
            radius,
            color,
        });
    }

    fn paint_circle_stroke(
        &mut self,
        center: [f32; 2],
        radius: f32,
        color: [f32; 4],
        thickness: f32,
    ) {
        self.record(PaintCall::CircleStroke {
            center,
            radius,
            color,
            thickness,
        });
    }

    fn paint_text(&mut self, pos: [f32; 2], color: [f32; 4], text: &str) {
        self.record(PaintCall::Text {
            pos,
            text: text.to_owned(),
            size: 12.0,
            color,
            family: FontFamilyHint::Proportional,
            align: TextAlign::Left,
        });
    }

    fn paint_text_styled(
        &mut self,
        pos: [f32; 2],
        text: &str,
        size: f32,
        color: [f32; 4],
        family: FontFamilyHint,
        align: TextAlign,
    ) {
        self.record(PaintCall::Text {
            pos,
            text: text.to_owned(),
            size,
            color,
            family,
            align,
        });
    }

    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        self.record(PaintCall::Path {
            points: points.to_vec(),
            color,
        });
    }

    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        self.record(PaintCall::Interact {
            id_salt: id_salt.to_owned(),
            rect,
        });
        self.scripted.get(id_salt).copied().unwrap_or_default()
    }

    fn region_at(&mut self, _rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }

    fn cursor_pos(&self) -> [f32; 2] {
        self.cursor
    }

    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        let rect = [self.cursor[0], self.cursor[1], size[0], size[1]];
        self.cursor[1] += size[1];
        rect
    }

    fn panel_rect(&self) -> [f32; 4] {
        self.panel
    }

    fn screen_rect(&self) -> [f32; 4] {
        self.panel
    }

    // ── Queries ───────────────────────────────────────

    fn available_width(&self) -> f32 {
        self.panel[2]
    }

    fn available_height(&self) -> f32 {
        self.panel[3]
    }

    // ── Stock widgets — recorded as text, never used by this crate's
    //    widgets (they paint). Present only to satisfy the trait. ──

    fn heading(&mut self, text: &str) {
        self.paint_text([0.0, 0.0], [1.0; 4], text);
    }

    fn label(&mut self, text: &str) {
        self.paint_text([0.0, 0.0], [1.0; 4], text);
    }

    fn colored_label(&mut self, color: [f32; 4], text: &str) {
        self.paint_text([0.0, 0.0], color, text);
    }

    fn small_label(&mut self, text: &str) {
        self.paint_text([0.0, 0.0], [1.0; 4], text);
    }

    fn monospace(&mut self, text: &str) {
        self.paint_text_styled(
            [0.0, 0.0],
            text,
            12.0,
            [1.0; 4],
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
    }

    fn button(&mut self, text: &str) -> bool {
        self.interact_rect(text, [0.0, 0.0, 0.0, 0.0]).clicked
    }

    fn small_button(&mut self, text: &str) -> bool {
        self.interact_rect(text, [0.0, 0.0, 0.0, 0.0]).clicked
    }

    fn selectable_label(&mut self, _active: bool, text: &str) -> bool {
        self.interact_rect(text, [0.0, 0.0, 0.0, 0.0]).clicked
    }

    fn selectable_label_double_clicked(&mut self, _active: bool, text: &str) -> bool {
        self.interact_rect(text, [0.0, 0.0, 0.0, 0.0])
            .double_clicked
    }

    fn checkbox(&mut self, _checked: &mut bool, _text: &str) -> bool {
        false
    }

    fn drag_value_f32(&mut self, _label: &str, _value: &mut f32, _speed: f32) -> bool {
        false
    }

    fn slider_f32(&mut self, _label: &str, _value: &mut f32, _min: f32, _max: f32) -> bool {
        false
    }

    fn text_edit_singleline(&mut self, _text: &mut String) -> bool {
        false
    }

    fn vec3_editor(&mut self, _label: &str, _value: &mut [f32; 3], _speed: f32) -> bool {
        false
    }

    fn color_edit(&mut self, _label: &str, _color: &mut [f32; 4]) -> bool {
        false
    }

    fn combo_box(&mut self, _label: &str, _current: &mut usize, _options: &[&str]) -> bool {
        false
    }

    // ── Layout — run the closure so nested painting is recorded ──

    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }

    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }

    fn collapsing(
        &mut self,
        _header: &str,
        _default_open: bool,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        f(self);
    }

    fn indent(&mut self, _id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }

    fn scroll_area(&mut self, _id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }

    fn separator(&mut self) {}

    fn spacing(&mut self, points: f32) {
        self.cursor[1] += points;
    }

    // ── Item queries ──────────────────────────────────

    fn is_last_item_double_clicked(&self) -> bool {
        false
    }

    fn is_last_item_hovered(&self) -> bool {
        false
    }

    fn is_last_item_enter_pressed(&self) -> bool {
        false
    }

    fn is_last_item_escape_pressed(&self) -> bool {
        false
    }

    fn context_menu_last(&mut self, _f: &mut dyn FnMut(&mut dyn UiBuilder)) {}

    fn viewport_image(
        &mut self,
        _handle: ViewportTextureHandle,
        _size: [f32; 2],
    ) -> Option<[f32; 2]> {
        None
    }
}
