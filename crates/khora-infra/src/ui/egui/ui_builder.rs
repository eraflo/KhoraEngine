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

//! Concrete [`UiBuilder`] backed by `egui::Ui`.
//!
//! The trait's methods are answered here, those that take more than a few
//! egui calls by inherent methods in sibling files: `editors` (multi-part
//! value editors), `paint` (direct painting), `menus` (popup menus),
//! `inset_panels` (nested areas and the modal), `dnd` (drag and drop);
//! `convert` maps the neutral values to egui's.

use khora_core::platform::input::KeyCode;
use khora_core::ui::editor::ui_builder::{FontFamilyHint, InlineEditEvent, Interaction, TextAlign};
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::editor::{LinearGradient, RadialGradient, UiBuilder};
use std::collections::HashMap;

use super::convert::{color_to_egui, font_id_for, map_key, rect_of, text_anchor};
use super::paint::{fill_rect, stroke_rect};

/// Wraps `&mut egui::Ui` to implement the abstract [`UiBuilder`] trait.
pub struct EguiUiBuilder<'a> {
    pub(super) ui: &'a mut egui::Ui,
    /// Shared reference to the viewport texture mapping.
    pub(super) viewport_textures: &'a HashMap<ViewportTextureHandle, egui::TextureId>,
    /// The last widget response (for context menu / double-click queries).
    pub(super) last_response: Option<egui::Response>,
    /// Clip rects saved by `push_clip_rect`, so nesting restores correctly.
    clip_stack: Vec<egui::Rect>,
}

impl<'a> EguiUiBuilder<'a> {
    /// Creates a new builder wrapping the given egui UI region.
    pub fn new(
        ui: &'a mut egui::Ui,
        viewport_textures: &'a HashMap<ViewportTextureHandle, egui::TextureId>,
    ) -> Self {
        Self {
            ui,
            viewport_textures,
            last_response: None,
            clip_stack: Vec::new(),
        }
    }
}

impl UiBuilder for EguiUiBuilder<'_> {
    // ── Text ───────────────────────────────────────────

    fn heading(&mut self, text: &str) {
        self.ui.heading(text);
    }

    fn label(&mut self, text: &str) {
        self.ui.label(text);
    }

    fn colored_label(&mut self, color: [f32; 4], text: &str) {
        self.ui.colored_label(color_to_egui(color), text);
    }

    fn small_label(&mut self, text: &str) {
        self.ui.small(text);
    }

    fn monospace(&mut self, text: &str) {
        self.ui.monospace(text);
    }

    // ── Interactive ────────────────────────────────────

    fn button(&mut self, text: &str) -> bool {
        let r = self.ui.button(text);
        let clicked = r.clicked();
        self.last_response = Some(r);
        clicked
    }

    fn small_button(&mut self, text: &str) -> bool {
        let r = self.ui.small_button(text);
        let clicked = r.clicked();
        self.last_response = Some(r);
        clicked
    }

    fn selectable_label(&mut self, active: bool, text: &str) -> bool {
        let r = self.ui.selectable_label(active, text);
        let clicked = r.clicked();
        self.last_response = Some(r);
        clicked
    }

    fn selectable_label_double_clicked(&mut self, active: bool, text: &str) -> bool {
        let r = self.ui.selectable_label(active, text);
        let double_clicked = r.double_clicked();
        self.last_response = Some(r);
        double_clicked
    }

    fn checkbox(&mut self, checked: &mut bool, text: &str) -> bool {
        let r = self.ui.checkbox(checked, text);
        let changed = r.changed();
        self.last_response = Some(r);
        changed
    }

    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool {
        let inner = self.ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(value).speed(speed))
        });
        let changed = inner.inner.changed();
        self.last_response = Some(inner.inner);
        changed
    }

    fn drag_value_f32_in(
        &mut self,
        label: &str,
        value: &mut f32,
        speed: f32,
        min: f32,
        max: f32,
    ) -> bool {
        self.edit_f32_in(label, value, speed, min, max)
    }

    fn colored_label_with_tooltip(&mut self, color: [f32; 4], text: &str, tooltip: &str) {
        self.ui
            .colored_label(color_to_egui(color), text)
            .on_hover_text(tooltip);
    }

    fn slider_f32(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool {
        let r = self.ui.add(egui::Slider::new(value, min..=max).text(label));
        let changed = r.changed();
        self.last_response = Some(r);
        changed
    }

    fn text_edit_singleline(&mut self, text: &mut String) -> bool {
        let r = self.ui.text_edit_singleline(text);
        let changed = r.changed();
        // Storing the response is what makes `is_last_item_enter_pressed` and
        // `is_last_item_escape_pressed` work at all: without it they inspect a
        // `None` and report `false` forever, which is why Enter and Escape did
        // nothing in the command palette.
        self.last_response = Some(r);
        changed
    }

    fn vec3_editor(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
        self.edit_vec3(label, value, speed)
    }

    fn color_edit(&mut self, label: &str, color: &mut [f32; 4]) -> bool {
        let inner = self.ui.horizontal(|ui| {
            ui.label(label);
            ui.color_edit_button_rgba_unmultiplied(color)
        });
        let changed = inner.inner.changed();
        self.last_response = Some(inner.inner);
        changed
    }

    fn combo_box(
        &mut self,
        id_salt: &str,
        label: &str,
        current: &mut usize,
        options: &[&str],
    ) -> bool {
        self.pick_option(id_salt, label, current, options)
    }

    // ── Layout ─────────────────────────────────────────

    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        // The row's last widget is the last one drawn here: what a tooltip
        // or a context menu asked for after the row attaches to.
        let last = self
            .ui
            .horizontal(|ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
                nested.last_response
            })
            .inner;
        if last.is_some() {
            self.last_response = last;
        }
    }

    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        self.ui.vertical(|ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
    }

    fn collapsing(
        &mut self,
        header: &str,
        default_open: bool,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        let vt = self.viewport_textures;
        let shown = egui::CollapsingHeader::new(header)
            .default_open(default_open)
            .show(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
        // The block's last widget is its header: what a tooltip or a context
        // menu asked for after the block attaches to.
        self.last_response = Some(shown.header_response);
    }

    fn indent(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        self.ui.indent(id, |ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
    }

    fn scroll_area(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::ScrollArea::vertical()
            .id_salt(id)
            .show(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    fn pixels_per_point(&self) -> f32 {
        self.ui.ctx().pixels_per_point()
    }

    fn viewport_image(
        &mut self,
        handle: ViewportTextureHandle,
        size: [f32; 2],
    ) -> Option<[f32; 2]> {
        let egui_id = self.viewport_textures.get(&handle)?;
        // IMPORTANT: use `Sense::hover()` rather than click_and_drag here.
        // egui-winit's `consumed = wants_pointer_input()` short-circuits
        // every mouse press while the cursor is over a click-sensing
        // widget — which previously meant clicks/drags on the 3D viewport
        // were swallowed before the engine's input handler could orbit /
        // pan the camera. Hover sense still drives `Response::hovered()`,
        // which is all `viewport_hovered` needs.
        let image = egui::Image::new(egui::load::SizedTexture::new(
            *egui_id,
            egui::vec2(size[0], size[1]),
        ))
        .sense(egui::Sense::hover());
        let response = self.ui.add(image);
        let min = response.rect.min;
        self.last_response = Some(response);
        Some([min.x, min.y])
    }

    // ── Decoration ─────────────────────────────────────

    fn separator(&mut self) {
        self.ui.separator();
    }

    fn spacing(&mut self, points: f32) {
        self.ui.add_space(points);
    }

    // ── Interaction ────────────────────────────────────

    fn is_last_item_double_clicked(&self) -> bool {
        self.last_response
            .as_ref()
            .is_some_and(|r| r.double_clicked())
    }

    fn is_last_item_hovered(&self) -> bool {
        self.last_response.as_ref().is_some_and(|r| r.hovered())
    }

    fn is_last_item_enter_pressed(&self) -> bool {
        self.last_response
            .as_ref()
            .is_some_and(|r| r.lost_focus() && self.ui.input(|i| i.key_pressed(egui::Key::Enter)))
    }

    fn is_last_item_escape_pressed(&self) -> bool {
        self.last_response
            .as_ref()
            .is_some_and(|_| self.ui.input(|i| i.key_pressed(egui::Key::Escape)))
    }

    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.open_context_menu_last(f);
    }

    fn menu_on_click_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.open_menu_on_click_last(f);
    }

    fn context_menu_panel(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.open_context_menu_panel(f);
    }

    fn close_menu(&mut self) {
        self.ui.close();
    }

    fn menu_button(&mut self, label: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        self.ui.menu_button(label, |ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
    }

    fn paint_line(&mut self, from: [f32; 2], to: [f32; 2], color: [f32; 4], thickness: f32) {
        self.ui.painter().line_segment(
            [egui::pos2(from[0], from[1]), egui::pos2(to[0], to[1])],
            egui::Stroke::new(thickness, color_to_egui(color)),
        );
    }

    fn paint_rect_filled(&mut self, min: [f32; 2], size: [f32; 2], color: [f32; 4], rounding: f32) {
        fill_rect(self.ui.painter(), min, size, color, rounding);
    }

    fn paint_text(&mut self, pos: [f32; 2], color: [f32; 4], text: &str) {
        self.ui.painter().text(
            egui::pos2(pos[0], pos[1]),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(12.0),
            color_to_egui(color),
        );
    }

    // ── Queries ────────────────────────────────────────

    fn available_width(&self) -> f32 {
        self.ui.available_width()
    }

    fn available_height(&self) -> f32 {
        self.ui.available_height()
    }

    fn panel_rect(&self) -> [f32; 4] {
        let r = self.ui.max_rect();
        [r.min.x, r.min.y, r.width(), r.height()]
    }

    fn screen_rect(&self) -> [f32; 4] {
        let r = self.ui.ctx().content_rect();
        [r.min.x, r.min.y, r.width(), r.height()]
    }

    fn paint_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        stroke_rect(self.ui.painter(), min, size, color, rounding, thickness);
    }

    fn paint_circle_filled(&mut self, center: [f32; 2], radius: f32, color: [f32; 4]) {
        self.ui.painter().circle_filled(
            egui::pos2(center[0], center[1]),
            radius,
            color_to_egui(color),
        );
    }

    fn paint_circle_stroke(
        &mut self,
        center: [f32; 2],
        radius: f32,
        color: [f32; 4],
        thickness: f32,
    ) {
        self.ui.painter().circle_stroke(
            egui::pos2(center[0], center[1]),
            radius,
            egui::Stroke::new(thickness, color_to_egui(color)),
        );
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
        self.ui.painter().text(
            egui::pos2(pos[0], pos[1]),
            text_anchor(align),
            text,
            font_id_for(family, size),
            color_to_egui(color),
        );
    }

    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        self.fill_path(points, color);
    }

    fn paint_path_filled_gradient(&mut self, points: &[[f32; 2]], gradient: LinearGradient<'_>) {
        self.fill_path_linear(points, gradient);
    }

    fn paint_path_filled_radial(&mut self, points: &[[f32; 2]], gradient: RadialGradient<'_>) {
        self.fill_path_radial(points, gradient);
    }

    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        let id = self.ui.id().with(("khora_hot", id_salt));
        let response = self
            .ui
            .interact(rect_of(rect), id, egui::Sense::click_and_drag());
        let interaction = Interaction {
            hovered: response.hovered(),
            contains_pointer: response.contains_pointer(),
            clicked: response.clicked(),
            pressed: response.is_pointer_button_down_on(),
            double_clicked: response.double_clicked(),
            focused: response.has_focus(),
        };
        self.last_response = Some(response);
        interaction
    }

    fn key_pressed(&self, key: KeyCode) -> bool {
        // A shortcut must not fire while a text field is taking input, or a
        // panel's single-key bindings would eat the user's typing.
        if self.ui.ctx().egui_wants_keyboard_input() {
            return false;
        }
        let Some(egui_key) = map_key(key) else {
            return false;
        };
        self.ui.input(|i| i.key_pressed(egui_key))
    }

    fn keyboard_captured(&self) -> bool {
        self.ui.ctx().egui_wants_keyboard_input()
    }

    fn raw_key_pressed(&self, key: KeyCode) -> bool {
        let Some(egui_key) = map_key(key) else {
            return false;
        };
        self.ui.input(|i| i.key_pressed(egui_key))
    }

    fn focus_last_item(&mut self) {
        if let Some(response) = self.last_response.as_ref() {
            response.request_focus();
        }
    }

    fn push_clip_rect(&mut self, rect: [f32; 4]) {
        // Intersect rather than replace: a nested clip must never widen the
        // region its parent already restricted.
        self.clip_stack.push(self.ui.clip_rect());
        let clipped = self.ui.clip_rect().intersect(rect_of(rect));
        self.ui.set_clip_rect(clipped);
    }

    fn pop_clip_rect(&mut self) {
        if let Some(previous) = self.clip_stack.pop() {
            self.ui.set_clip_rect(previous);
        }
    }

    fn scroll_delta_in(&self, rect: [f32; 4]) -> f32 {
        let r = rect_of(rect);
        let inside = self
            .ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|p| r.contains(p));
        if !inside {
            return 0.0;
        }
        self.ui.input(|i| i.smooth_scroll_delta.y)
    }

    fn dnd_attach_drag_payload(&mut self, payload: u64) {
        self.attach_drag_payload(payload);
    }

    fn dnd_hover_payload(&self) -> Option<u64> {
        self.hovering_payload()
    }

    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        self.released_payload()
    }

    fn pointer_position(&self) -> Option<[f32; 2]> {
        self.ui.ctx().pointer_interact_pos().map(|p| [p.x, p.y])
    }

    fn is_last_item_dragged(&self) -> bool {
        self.last_item_dragged()
    }

    fn is_drag_active(&self) -> bool {
        self.ui.ctx().dragged_id().is_some()
    }

    fn inline_text_field(
        &mut self,
        rect: [f32; 4],
        id_salt: &str,
        text: &mut String,
        request_focus: bool,
    ) -> InlineEditEvent {
        self.edit_inline_text(rect, id_salt, text, request_focus)
    }

    fn overlay_rect_filled(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
    ) {
        fill_rect(&self.overlay_painter(), min, size, color, rounding);
    }

    fn overlay_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        stroke_rect(
            &self.overlay_painter(),
            min,
            size,
            color,
            rounding,
            thickness,
        );
    }

    fn overlay_text(
        &mut self,
        pos: [f32; 2],
        text: &str,
        size: f32,
        color: [f32; 4],
        family: FontFamilyHint,
    ) {
        self.overlay_painter().text(
            egui::pos2(pos[0], pos[1]),
            egui::Align2::LEFT_TOP,
            text,
            font_id_for(family, size),
            color_to_egui(color),
        );
    }

    fn tooltip_for_last(&mut self, text: &str) {
        if let Some(response) = self.last_response.as_ref() {
            response.clone().on_hover_text(text);
        }
    }

    fn region_at(&mut self, id_salt: &str, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.show_region(id_salt, rect, f);
    }

    fn cursor_pos(&self) -> [f32; 2] {
        let p = self.ui.next_widget_position();
        [p.x, p.y]
    }

    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        let (rect, _) = self
            .ui
            .allocate_exact_size(egui::vec2(size[0], size[1]), egui::Sense::hover());
        [rect.min.x, rect.min.y, rect.width(), rect.height()]
    }

    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        self.text_size(text, size, family)
    }

    // ── Inset panels ───────────────────────────────────

    fn top_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let panel = egui::Panel::top(egui::Id::new(id.to_owned())).exact_size(height);
        self.show_inset_panel(panel, f);
    }

    fn bottom_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let panel = egui::Panel::bottom(egui::Id::new(id.to_owned())).exact_size(height);
        self.show_inset_panel(panel, f);
    }

    fn left_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let panel = egui::Panel::left(egui::Id::new(id.to_owned())).exact_size(width);
        self.show_inset_panel(panel, f);
    }

    fn right_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let panel = egui::Panel::right(egui::Id::new(id.to_owned())).exact_size(width);
        self.show_inset_panel(panel, f);
    }

    fn central_inset(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.show_central_inset(f);
    }

    fn modal(&mut self, id: &str, size: [f32; 2], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.show_modal_dialog(id, size, f);
    }

    fn frame_box(
        &mut self,
        margin: khora_core::ui::Margin,
        fill: Option<khora_core::math::LinearRgba>,
        stroke: khora_core::ui::Stroke,
        radius: khora_core::ui::CornerRadius,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        self.show_frame_box(margin, fill, stroke, radius, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small glow painted over a whole 1440 × 900 screen: the colour
    /// varies only inside the 20-unit disc and is the last stop's
    /// everywhere else, so the mesh stays the size of a small fill instead
    /// of growing with the screen area over the square of the radius.
    #[test]
    fn a_small_radial_glow_over_a_large_polygon_paints_a_bounded_mesh() {
        use khora_core::ui::editor::GradientStop;
        let stops = [
            GradientStop {
                offset: 0.0,
                color: [1.0, 1.0, 1.0, 1.0],
            },
            GradientStop {
                offset: 1.0,
                color: [1.0, 1.0, 1.0, 0.0],
            },
        ];
        let glow = RadialGradient {
            center: [720.0, 450.0],
            radius: 20.0,
            stops: &stops,
        };
        let screen = [[0.0, 0.0], [1440.0, 0.0], [1440.0, 900.0], [0.0, 900.0]];
        let ctx = egui::Context::default();
        let textures = HashMap::new();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1440.0, 900.0),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            EguiUiBuilder::new(ui, &textures).paint_path_filled_radial(&screen, glow);
        });
        let triangles: usize = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) => Some(mesh.indices.len() / 3),
                _ => None,
            })
            .sum();
        assert!(triangles > 0, "the glow painted nothing");
        assert!(
            triangles <= 50_000,
            "a 20-unit glow over 1440 x 900 painted {triangles} triangles"
        );
    }
}
