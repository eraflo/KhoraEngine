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

//! The value editors of the egui UI builder that take more than one egui
//! call: a bounded dragged number, the X/Y/Z vector row, the combo box and
//! the inline text field.

use khora_core::ui::editor::ui_builder::InlineEditEvent;

use super::convert::rect_of;
use super::theme::AXIS_COLORS_KEY;
use super::ui_builder::EguiUiBuilder;

impl EguiUiBuilder<'_> {
    /// A labelled dragged number kept inside `min..=max` by every edit; a
    /// value already outside is shown as it is until edited.
    pub(super) fn edit_f32_in(
        &mut self,
        label: &str,
        value: &mut f32,
        speed: f32,
        min: f32,
        max: f32,
    ) -> bool {
        let inner = self.ui.horizontal(|ui| {
            ui.label(label);
            // A value outside the bounds is shown as it is until edited.
            ui.add(
                egui::DragValue::new(value)
                    .speed(speed)
                    .range(min..=max)
                    .clamp_existing_to_range(false),
            )
        });
        let changed = inner.inner.changed();
        self.last_response = Some(inner.inner);
        // A keyboard step is not clamped by the widget itself: an edit stays
        // inside the bounds whichever way it came.
        if changed {
            *value = value.max(min).min(max);
        }
        changed
    }

    /// A row of three dragged numbers, each after its axis letter.
    pub(super) fn edit_vec3(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
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

    /// A combo box over `options`, salted by `id_salt`; true when a pick
    /// changed `current`.
    pub(super) fn pick_option(
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

    /// A single-line text field placed at `rect`, as a rename field is:
    /// Escape cancels, Enter or a click elsewhere commits.
    pub(super) fn edit_inline_text(
        &mut self,
        rect: [f32; 4],
        id_salt: &str,
        text: &mut String,
        request_focus: bool,
    ) -> InlineEditEvent {
        let mut child = self.ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect_of(rect))
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
}
