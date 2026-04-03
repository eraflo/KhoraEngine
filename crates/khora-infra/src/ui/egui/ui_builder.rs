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
        self.ui
            .horizontal(|ui| {
                ui.label(label);
                let x = ui
                    .add(
                        egui::DragValue::new(&mut value[0])
                            .speed(speed)
                            .prefix("X: "),
                    )
                    .changed();
                let y = ui
                    .add(
                        egui::DragValue::new(&mut value[1])
                            .speed(speed)
                            .prefix("Y: "),
                    )
                    .changed();
                let z = ui
                    .add(
                        egui::DragValue::new(&mut value[2])
                            .speed(speed)
                            .prefix("Z: "),
                    )
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
        let image = egui::Image::new(egui::load::SizedTexture::new(
            *egui_id,
            egui::vec2(size[0], size[1]),
        ))
        .sense(egui::Sense::click_and_drag());
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
            .is_some_and(|r| self.ui.input(|i| i.key_pressed(egui::Key::Escape)))
    }

    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        if let Some(response) = self.last_response.take() {
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
}
