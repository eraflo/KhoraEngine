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

//! The popup menus the egui UI builder opens: the context menu of the last
//! item, a menu on a click of the last item, and the context menu of a
//! panel's empty space.

use khora_core::ui::editor::UiBuilder;

use super::ui_builder::EguiUiBuilder;

impl EguiUiBuilder<'_> {
    /// The last item's context menu, with `f` building its content.
    pub(super) fn open_context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
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

    /// A menu opened by a click on the last item, closed by a click outside
    /// it, with `f` building its content.
    pub(super) fn open_menu_on_click_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // A clone, as for the context menu: the response stays the last one.
        // A click inside — into a search field, say — keeps it open; a pick
        // closes it itself.
        if let Some(response) = self.last_response.clone() {
            let vt = self.viewport_textures;
            egui::Popup::menu(&response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    let mut nested = EguiUiBuilder::new(ui, vt);
                    f(&mut nested);
                });
        }
    }

    /// The context menu of the panel's remaining space, with `f` building
    /// its content.
    pub(super) fn open_context_menu_panel(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
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
}
