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

//! The areas the egui UI builder opens for nested content: inset panels on
//! an edge or in the centre, a region at a fixed rectangle, a framed box,
//! and the modal dialog over a dimmed screen.

use khora_core::ui::editor::UiBuilder;

use super::convert::{linear_to_color, rect_of};
use super::ui_builder::EguiUiBuilder;

impl EguiUiBuilder<'_> {
    /// Shows `panel` — a side panel already sized — without a frame or a
    /// resize handle, with `f` building its content.
    pub(super) fn show_inset_panel(
        &mut self,
        panel: egui::Panel,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        let vt = self.viewport_textures;
        panel
            .resizable(false)
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    /// Shows the space the inset panels leave, without a frame, with `f`
    /// building its content.
    pub(super) fn show_central_inset(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = self.viewport_textures;
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut nested = EguiUiBuilder::new(ui, vt);
                f(&mut nested);
            });
    }

    /// Lays `f`'s content out top-down inside `rect`, its ids salted by
    /// `id_salt`.
    pub(super) fn show_region(
        &mut self,
        id_salt: &str,
        rect: [f32; 4],
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        let vt = self.viewport_textures;
        // Salted by name, not by position. Deriving the id from the rect's
        // screen coordinates meant moving a panel by one pixel — a splitter
        // drag, a window resize — renumbered every widget inside, so egui
        // dropped focus and edit state mid-typing; and two regions landing on
        // the same integer coordinate collided outright.
        let id_salt = ("khora_region", id_salt);
        let mut child = self.ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect_of(rect))
                .id_salt(id_salt)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let mut nested = EguiUiBuilder::new(&mut child, vt);
        f(&mut nested);
    }

    /// Wraps `f`'s content in a frame with the given margin, fill, stroke
    /// and corner radius.
    pub(super) fn show_frame_box(
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

    /// Modal implementation lives outside the `UiBuilder` impl block so
    /// `Self::ui` can be re-borrowed across the egui `Window::show`
    /// call without colliding with the trait method's generics.
    pub(super) fn show_modal_dialog(
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
        // In the UI's own points: the viewport's `inner_rect` is the window's
        // place on the desktop, and would dim only part of it.
        let screen = ctx.content_rect();
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
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0)))
            .show(&ctx, |ui| {
                // The dialog paints at absolute positions inside its panel and
                // allocates nothing: claim its size, or the window shrinks to
                // nothing and its anchor puts its corner — not its centre — at
                // the centre of the screen.
                ui.set_min_size(egui::vec2(size[0], size[1]));
                let mut nested = EguiUiBuilder::new(ui, &vt_clone);
                f(&mut nested);
            });
    }
}
