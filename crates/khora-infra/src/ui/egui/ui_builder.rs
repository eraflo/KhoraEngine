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

use khora_core::ui::editor::ui_builder::{FontFamilyHint, InlineEditEvent, Interaction, TextAlign};
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::platform::input::KeyCode;
use khora_core::ui::editor::UiBuilder;
use std::collections::HashMap;

use super::theme::AXIS_COLORS_KEY;

/// Maps a backend-neutral [`FontFamilyHint`] to an egui [`FontId`](egui::FontId).
///
/// `Display` and `Icons` resolve to named families installed by `set_fonts`;
/// both fall back to the proportional family (via egui's own family fallback)
/// when their face wasn't provided.
fn font_id_for(family: FontFamilyHint, size: f32) -> egui::FontId {
    match family {
        FontFamilyHint::Proportional => egui::FontId::proportional(size),
        FontFamilyHint::Monospace => egui::FontId::monospace(size),
        FontFamilyHint::Display => {
            egui::FontId::new(size, egui::FontFamily::Name("display".into()))
        }
        FontFamilyHint::Icons => egui::FontId::new(size, egui::FontFamily::Name("icons".into())),
    }
}

/// Wraps `&mut egui::Ui` to implement the abstract [`UiBuilder`] trait.
pub struct EguiUiBuilder<'a> {
    ui: &'a mut egui::Ui,
    /// Shared reference to the viewport texture mapping.
    viewport_textures: &'a HashMap<ViewportTextureHandle, egui::TextureId>,
    /// The last widget response (for context menu / double-click queries).
    last_response: Option<egui::Response>,
    /// Clip rects saved by `push_clip_rect`, so nesting restores correctly.
    clip_stack: Vec<egui::Rect>,
}

/// Maps the engine's [`KeyCode`] to egui's key enum.
///
/// Deliberately partial: only the keys the editor's UI actually binds are
/// listed, so an unmapped key reports "not pressed" instead of silently
/// matching the wrong one. Extend it as bindings are added.
fn map_key(key: KeyCode) -> Option<egui::Key> {
    Some(match key {
        KeyCode::ArrowUp => egui::Key::ArrowUp,
        KeyCode::ArrowDown => egui::Key::ArrowDown,
        KeyCode::ArrowLeft => egui::Key::ArrowLeft,
        KeyCode::ArrowRight => egui::Key::ArrowRight,
        KeyCode::Enter => egui::Key::Enter,
        KeyCode::Escape => egui::Key::Escape,
        KeyCode::Tab => egui::Key::Tab,
        KeyCode::Home => egui::Key::Home,
        KeyCode::End => egui::Key::End,
        KeyCode::Delete => egui::Key::Delete,
        KeyCode::Backspace => egui::Key::Backspace,
        KeyCode::F2 => egui::Key::F2,
        _ => return None,
    })
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

    /// A painter on the top foreground layer, clipped only to the whole screen.
    /// Used for overlay affordances (drag ghosts) that must stay visible when
    /// the cursor leaves the current panel's clip rect.
    fn overlay_painter(&self) -> egui::Painter {
        self.ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("khora_overlay"),
        ))
    }
}

fn color_to_egui(c: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        (c[3] * 255.0) as u8,
    )
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
        // The axis colour rides on the *letter*, not on a filled badge behind
        // it. Three saturated badges per row turn a transform-heavy inspector
        // into a rainbow; a tinted letter says the same thing and lets the
        // numbers stay the loudest part of the row.
        //
        // Colours come from the active theme (stashed by `apply_theme`), so
        // the inspector's X/Y/Z always match the viewport gizmo's.
        let axes = self
            .ui
            .ctx()
            .data(|d| d.get_temp::<[egui::Color32; 3]>(egui::Id::new(AXIS_COLORS_KEY)))
            .unwrap_or([
                egui::Color32::from_rgb(246, 109, 103),
                egui::Color32::from_rgb(114, 207, 142),
                egui::Color32::from_rgb(115, 204, 234),
            ]);

        let axis_letter = |ui: &mut egui::Ui, ch: &str, color: egui::Color32| {
            ui.label(
                egui::RichText::new(ch)
                    .color(color)
                    .strong()
                    .monospace()
                    .size(10.0),
            );
        };

        let inner = self.ui.horizontal(|ui| {
            if !label.is_empty() {
                ui.label(label);
            }
            let mut changed = false;
            let mut last = None;
            for (i, ch) in ["X", "Y", "Z"].iter().enumerate() {
                axis_letter(ui, ch, axes[i]);
                let r = ui.add(egui::DragValue::new(&mut value[i]).speed(speed));
                changed |= r.changed();
                last = Some(r);
            }
            (changed, last)
        });
        let (changed, last) = inner.inner;
        // The Z field is the row's "last item" — a context menu or tooltip
        // attached after the row lands on the field the user ended on.
        self.last_response = last;
        changed
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
        let selected_text = options.get(*current).copied().unwrap_or("");
        let mut changed = false;
        // Salted explicitly rather than by label: `from_label` derives the id
        // from the label text, so two combo boxes labelled the same — which the
        // inspector's generic enum walker produces for every switchable enum —
        // shared one popup and one open state.
        let out = egui::ComboBox::new(("khora_combo", id_salt), label)
            .selected_text(selected_text)
            .show_ui(self.ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    if ui.selectable_label(i == *current, *option).clicked() {
                        *current = i;
                        changed = true;
                    }
                }
            });
        self.last_response = Some(out.response);
        changed
    }

    // ── Layout ─────────────────────────────────────────

    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        self.ui.horizontal(|ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
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
        egui::CollapsingHeader::new(header)
            .default_open(default_open)
            .show(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
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
        // Borrow rather than take — the egui `Response::context_menu` API
        // takes `&self`, and removing the response from `last_response`
        // would prevent any follow-up call (`tooltip_for_last`, etc.) from
        // working in the same widget's lifecycle. Cloning is cheap (it's
        // mostly Arc-internal in egui).
        if let Some(response) = self.last_response.clone() {
            let vt = self.viewport_textures;
            response.context_menu(|ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
        }
    }

    fn context_menu_panel(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // Allocate remaining space at the bottom of the scroll area so the
        // context-menu target does not overlap earlier interactive widgets
        // (which would steal left-clicks from selectable_labels above).
        let remaining = self.ui.available_size();
        // Ensure the context-menu area covers at least some space so
        // right-clicking on any empty part of the panel works.
        let min_h = remaining.y.max(40.0);
        let (id, rect) = self.ui.allocate_space(egui::vec2(remaining.x, min_h));
        let response = self.ui.interact(rect, id, egui::Sense::click());
        let vt = self.viewport_textures;
        response.context_menu(|ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
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
        let rect =
            egui::Rect::from_min_size(egui::pos2(min[0], min[1]), egui::vec2(size[0], size[1]));
        let corner = egui::CornerRadius::same(rounding.clamp(0.0, 255.0) as u8);
        self.ui
            .painter()
            .rect_filled(rect, corner, color_to_egui(color));
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
        let rect =
            egui::Rect::from_min_size(egui::pos2(min[0], min[1]), egui::vec2(size[0], size[1]));
        let corner = egui::CornerRadius::same(rounding.clamp(0.0, 255.0) as u8);
        self.ui.painter().rect_stroke(
            rect,
            corner,
            egui::Stroke::new(thickness, color_to_egui(color)),
            egui::epaint::StrokeKind::Inside,
        );
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
        let egui_align = match align {
            TextAlign::Left => egui::Align2::LEFT_TOP,
            TextAlign::Center => egui::Align2::CENTER_TOP,
            TextAlign::Right => egui::Align2::RIGHT_TOP,
        };
        let font_id = font_id_for(family, size);
        self.ui.painter().text(
            egui::pos2(pos[0], pos[1]),
            egui_align,
            text,
            font_id,
            color_to_egui(color),
        );
    }

    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        if points.len() < 3 {
            return;
        }
        use egui::epaint::{PathShape, PathStroke};
        let pts: Vec<egui::Pos2> = points.iter().map(|p| egui::pos2(p[0], p[1])).collect();
        self.ui.painter().add(egui::Shape::Path(PathShape {
            points: pts,
            closed: true,
            fill: color_to_egui(color),
            stroke: PathStroke::NONE,
        }));
    }

    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect[0], rect[1]), egui::vec2(rect[2], rect[3]));
        let id = self.ui.id().with(("khora_hot", id_salt));
        let response = self.ui.interact(r, id, egui::Sense::click_and_drag());
        let interaction = Interaction {
            hovered: response.hovered(),
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
        if self.ui.ctx().wants_keyboard_input() {
            return false;
        }
        let Some(egui_key) = map_key(key) else {
            return false;
        };
        self.ui.input(|i| i.key_pressed(egui_key))
    }

    fn keyboard_captured(&self) -> bool {
        self.ui.ctx().wants_keyboard_input()
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
        let r =
            egui::Rect::from_min_size(egui::pos2(rect[0], rect[1]), egui::vec2(rect[2], rect[3]));
        // Intersect rather than replace: a nested clip must never widen the
        // region its parent already restricted.
        self.clip_stack.push(self.ui.clip_rect());
        let clipped = self.ui.clip_rect().intersect(r);
        self.ui.set_clip_rect(clipped);
    }

    fn pop_clip_rect(&mut self) {
        if let Some(previous) = self.clip_stack.pop() {
            self.ui.set_clip_rect(previous);
        }
    }

    fn scroll_delta_in(&self, rect: [f32; 4]) -> f32 {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect[0], rect[1]), egui::vec2(rect[2], rect[3]));
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
        // Attach to the response left by the last `interact_rect` call.
        // That response was created with `Sense::click_and_drag()`, so
        // egui already detects drags — we just need to publish the payload.
        if let Some(response) = self.last_response.as_ref() {
            if response.dragged() {
                response.dnd_set_drag_payload::<u64>(payload);
            }
        }
    }

    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        self.last_response
            .as_ref()
            .and_then(|r| r.dnd_release_payload::<u64>())
            .map(|payload| *payload)
    }

    fn pointer_position(&self) -> Option<[f32; 2]> {
        self.ui
            .ctx()
            .pointer_interact_pos()
            .map(|p| [p.x, p.y])
    }

    fn is_last_item_dragged(&self) -> bool {
        self.last_response
            .as_ref()
            .map(|r| r.dragged())
            .unwrap_or(false)
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
        let r =
            egui::Rect::from_min_size(egui::pos2(rect[0], rect[1]), egui::vec2(rect[2], rect[3]));
        let mut child = self.ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r)
                .id_salt(("khora_inline", id_salt))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let field_id = egui::Id::new(("khora_inline_edit", id_salt));
        let resp = child.add(
            egui::TextEdit::singleline(text)
                .id(field_id)
                .desired_width(rect[2]),
        );
        if request_focus {
            resp.request_focus();
        }
        if resp.lost_focus() {
            // Escape cancels; Enter or a click elsewhere commits (commit-on-blur
            // — the standard file-explorer rename behaviour, and it avoids a
            // stuck field if the user clicks away).
            let escaped = child.input(|i| i.key_pressed(egui::Key::Escape));
            return if escaped {
                InlineEditEvent::Cancelled
            } else {
                InlineEditEvent::Committed
            };
        }
        if resp.changed() {
            InlineEditEvent::Changed
        } else {
            InlineEditEvent::Idle
        }
    }

    fn overlay_rect_filled(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
    ) {
        let painter = self.overlay_painter();
        let rect =
            egui::Rect::from_min_size(egui::pos2(min[0], min[1]), egui::vec2(size[0], size[1]));
        let corner = egui::CornerRadius::same(rounding.clamp(0.0, 255.0) as u8);
        painter.rect_filled(rect, corner, color_to_egui(color));
    }

    fn overlay_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        let painter = self.overlay_painter();
        let rect =
            egui::Rect::from_min_size(egui::pos2(min[0], min[1]), egui::vec2(size[0], size[1]));
        let corner = egui::CornerRadius::same(rounding.clamp(0.0, 255.0) as u8);
        painter.rect_stroke(
            rect,
            corner,
            egui::Stroke::new(thickness, color_to_egui(color)),
            egui::epaint::StrokeKind::Inside,
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
        let painter = self.overlay_painter();
        let font_id = font_id_for(family, size);
        painter.text(
            egui::pos2(pos[0], pos[1]),
            egui::Align2::LEFT_TOP,
            text,
            font_id,
            color_to_egui(color),
        );
    }

    fn tooltip_for_last(&mut self, text: &str) {
        if let Some(response) = self.last_response.as_ref() {
            response.clone().on_hover_text(text);
        }
    }

    fn region_at(&mut self, id_salt: &str, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect[0], rect[1]), egui::vec2(rect[2], rect[3]));
        let vt = self.viewport_textures;
        // Salted by name, not by position. Deriving the id from the rect's
        // screen coordinates meant moving a panel by one pixel — a splitter
        // drag, a window resize — renumbered every widget inside, so egui
        // dropped focus and edit state mid-typing; and two regions landing on
        // the same integer coordinate collided outright.
        let id_salt = ("khora_region", id_salt);
        let mut child = self.ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r)
                .id_salt(id_salt)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let mut nested = EguiUiBuilder::new(&mut child, vt);
        f(&mut nested);
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
        let font_id = font_id_for(family, size);
        // Use the painter's helper to lay out text — handles fonts atlas
        // mutability internally in egui 0.33.
        let galley =
            self.ui
                .painter()
                .layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE);
        let r = galley.rect;
        [r.width(), r.height()]
    }

    // ── Inset panels (Phase 7) ─────────────────────────

    fn top_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::TopBottomPanel::top(egui::Id::new(id.to_owned()))
            .exact_height(height)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    fn bottom_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::TopBottomPanel::bottom(egui::Id::new(id.to_owned()))
            .exact_height(height)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    fn left_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::SidePanel::left(egui::Id::new(id.to_owned()))
            .exact_width(width)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    fn right_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::SidePanel::right(egui::Id::new(id.to_owned()))
            .exact_width(width)
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    fn central_inset(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
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
        let mut frame = egui::Frame::new()
            .inner_margin(egui::Margin {
                left: margin.left as i8,
                right: margin.right as i8,
                top: margin.top as i8,
                bottom: margin.bottom as i8,
            })
            .corner_radius(egui::CornerRadius {
                nw: radius.nw as u8,
                ne: radius.ne as u8,
                sw: radius.sw as u8,
                se: radius.se as u8,
            });
        if let Some(fill_color) = fill {
            frame = frame.fill(linear_to_color(fill_color));
        }
        if stroke.width > 0.0 {
            frame = frame.stroke(egui::Stroke::new(
                stroke.width,
                linear_to_color(stroke.color),
            ));
        }
        let vt = self.viewport_textures;
        frame.show(self.ui, |ui| {
            let mut nested = EguiUiBuilder::new(ui, vt);
            f(&mut nested);
        });
    }
}

fn linear_to_color(c: khora_core::math::LinearRgba) -> egui::Color32 {
    color_to_egui([c.r, c.g, c.b, c.a])
}

impl EguiUiBuilder<'_> {
    /// Modal implementation lives outside the `UiBuilder` impl block so
    /// `Self::ui` can be re-borrowed across the egui `Window::show`
    /// call without colliding with the trait method's generics.
    fn show_modal_dialog(
        &mut self,
        id: &str,
        size: [f32; 2],
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        let ctx = self.ui.ctx().clone();
        let vt_clone = self.viewport_textures.clone();

        // Backdrop: dim the whole screen with a foreground-layer
        // painter. The egui Window itself sits in `Order::Foreground`
        // so the backdrop must be just *under* it; we use
        // `Order::Middle` so other content sinks below.
        let screen = ctx.input(|i| i.viewport().inner_rect.unwrap_or(egui::Rect::ZERO));
        let layer = egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new(format!("{}_backdrop", id)),
        );
        let painter = egui::Painter::new(ctx.clone(), layer, screen);
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));

        let window_id = egui::Id::new(format!("{}_modal", id));
        egui::Window::new("")
            .id(window_id)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([size[0], size[1]])
            .resizable(false)
            .collapsible(false)
            .frame(egui::Frame::window(&ctx.style()).inner_margin(egui::Margin::same(0)))
            .show(&ctx, |ui| {
                let mut nested = EguiUiBuilder::new(ui, &vt_clone);
                f(&mut nested);
            });
    }
}
