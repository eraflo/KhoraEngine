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

use khora_core::ui::editor::ui_builder::{FontFamilyHint, Interaction, TextAlign};
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::editor::UiBuilder;
use std::collections::HashMap;

/// Wraps `&mut egui::Ui` to implement the abstract [`UiBuilder`] trait.
pub struct EguiUiBuilder<'a> {
    ui: &'a mut egui::Ui,
    /// Shared reference to the viewport texture mapping.
    viewport_textures: &'a HashMap<ViewportTextureHandle, egui::TextureId>,
    /// The last widget response (for context menu / double-click queries).
    last_response: Option<egui::Response>,
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
        }
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
        self.ui.checkbox(checked, text).changed()
    }

    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool {
        self.ui
            .horizontal(|ui| {
                ui.label(label);
                ui.add(egui::DragValue::new(value).speed(speed)).changed()
            })
            .inner
    }

    fn slider_f32(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool {
        self.ui
            .add(egui::Slider::new(value, min..=max).text(label))
            .changed()
    }

    fn text_edit_singleline(&mut self, text: &mut String) -> bool {
        self.ui.text_edit_singleline(text).changed()
    }

    fn vec3_editor(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
        // Unity-style: filled colored X/Y/Z badges before each drag value.
        // Red = X, green = Y, blue = Z.
        const X_COLOR: egui::Color32 = egui::Color32::from_rgb(214, 75, 64);
        const Y_COLOR: egui::Color32 = egui::Color32::from_rgb(96, 178, 81);
        const Z_COLOR: egui::Color32 = egui::Color32::from_rgb(78, 132, 222);

        let axis_badge = |ui: &mut egui::Ui, ch: &str, color: egui::Color32| {
            egui::Frame::new()
                .fill(color)
                .corner_radius(egui::CornerRadius::same(3))
                .inner_margin(egui::Margin::symmetric(5, 1))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(ch)
                            .color(egui::Color32::WHITE)
                            .strong()
                            .monospace(),
                    );
                });
        };

        self.ui
            .horizontal(|ui| {
                ui.label(label);
                axis_badge(ui, "X", X_COLOR);
                let x = ui
                    .add(egui::DragValue::new(&mut value[0]).speed(speed))
                    .changed();
                axis_badge(ui, "Y", Y_COLOR);
                let y = ui
                    .add(egui::DragValue::new(&mut value[1]).speed(speed))
                    .changed();
                axis_badge(ui, "Z", Z_COLOR);
                let z = ui
                    .add(egui::DragValue::new(&mut value[2]).speed(speed))
                    .changed();
                x || y || z
            })
            .inner
    }

    fn color_edit(&mut self, label: &str, color: &mut [f32; 4]) -> bool {
        self.ui
            .horizontal(|ui| {
                ui.label(label);
                ui.color_edit_button_rgba_unmultiplied(color).changed()
            })
            .inner
    }

    fn combo_box(&mut self, label: &str, current: &mut usize, options: &[&str]) -> bool {
        let selected_text = options.get(*current).copied().unwrap_or("");
        let mut changed = false;
        egui::ComboBox::from_label(label)
            .selected_text(selected_text)
            .show_ui(self.ui, |ui| {
                for (i, option) in options.iter().enumerate() {
                    if ui.selectable_label(i == *current, *option).clicked() {
                        *current = i;
                        changed = true;
                    }
                }
            });
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
        let r = self.ui.ctx().screen_rect();
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
        let font_id = match family {
            FontFamilyHint::Proportional => egui::FontId::proportional(size),
            FontFamilyHint::Monospace => egui::FontId::monospace(size),
            FontFamilyHint::Icons => {
                egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
            }
        };
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
        let pts: Vec<egui::Pos2> = points
            .iter()
            .map(|p| egui::pos2(p[0], p[1]))
            .collect();
        self.ui.painter().add(egui::Shape::Path(PathShape {
            points: pts,
            closed: true,
            fill: color_to_egui(color),
            stroke: PathStroke::NONE,
        }));
    }

    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect[0], rect[1]),
            egui::vec2(rect[2], rect[3]),
        );
        let id = self.ui.id().with(("khora_hot", id_salt));
        let response = self.ui.interact(r, id, egui::Sense::click_and_drag());
        let interaction = Interaction {
            hovered: response.hovered(),
            clicked: response.clicked(),
            pressed: response.is_pointer_button_down_on(),
            double_clicked: response.double_clicked(),
        };
        self.last_response = Some(response);
        interaction
    }

    fn tooltip_for_last(&mut self, text: &str) {
        if let Some(response) = self.last_response.as_ref() {
            response.clone().on_hover_text(text);
        }
    }

    fn region_at(&mut self, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect[0], rect[1]),
            egui::vec2(rect[2], rect[3]),
        );
        let vt = self.viewport_textures;
        let id_salt = ("khora_region", rect[0] as i32, rect[1] as i32);
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

    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        let font_id = match family {
            FontFamilyHint::Proportional => egui::FontId::proportional(size),
            FontFamilyHint::Monospace => egui::FontId::monospace(size),
            FontFamilyHint::Icons => {
                egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
            }
        };
        // Use the painter's helper to lay out text — handles fonts atlas
        // mutability internally in egui 0.33.
        let galley =
            self.ui
                .painter()
                .layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE);
        let r = galley.rect;
        [r.width(), r.height()]
    }
}
