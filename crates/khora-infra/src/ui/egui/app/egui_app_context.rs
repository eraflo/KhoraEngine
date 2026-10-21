// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Concrete [`AppContext`] implementation backed by `egui::Context`.

use std::collections::HashMap;

use khora_core::ui::editor::ViewportTextureHandle;
use khora_core::ui::{AppContext, FontPack, UiBuilder, UiTheme};

use crate::ui::egui::theme::apply_theme;
use crate::ui::egui::ui_builder::EguiUiBuilder;

/// Wraps the frame's root `egui::Ui` to implement [`AppContext`].
///
/// Constructed once per frame by [`super::run_native`] and handed to
/// the user's [`khora_core::ui::App::update`] implementation.
///
/// Holds a `Ui` rather than a `Context` because eframe hands the app a root
/// `Ui` to paint *into*; panels nest inside it with `show_inside`. The
/// `Context` is still reachable through it for the theme, fonts and viewport
/// commands, which are context-wide rather than panel-local.
pub struct EguiAppContext<'a> {
    ui: &'a mut egui::Ui,
    frame: &'a mut eframe::Frame,
    /// No viewport textures for tool apps — empty map, kept so we can
    /// reuse `EguiUiBuilder::new`.
    viewport_textures: HashMap<ViewportTextureHandle, egui::TextureId>,
}

impl<'a> EguiAppContext<'a> {
    /// Build a context from the per-frame egui handles.
    pub fn new(ui: &'a mut egui::Ui, frame: &'a mut eframe::Frame) -> Self {
        Self {
            ui,
            frame,
            viewport_textures: HashMap::new(),
        }
    }
}

impl AppContext for EguiAppContext<'_> {
    fn central(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let vt = &self.viewport_textures;
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(self.ui, |ui| {
                let mut builder = EguiUiBuilder::new(ui, vt);
                f(&mut builder);
            });
    }

    fn set_theme(&mut self, theme: &UiTheme) {
        let ctx = self.ui.ctx().clone();
        apply_theme(&ctx, theme);
        // The native title bar follows the theme, not the OS setting.
        let system_theme = match theme.scheme {
            khora_core::ui::ColorScheme::Dark => egui::SystemTheme::Dark,
            khora_core::ui::ColorScheme::Light => egui::SystemTheme::Light,
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(system_theme));
    }

    fn set_fonts(&mut self, pack: &FontPack) {
        if pack.is_empty() {
            return;
        }
        self.ui
            .ctx()
            .set_fonts(crate::ui::egui::fonts::font_definitions(pack));
    }

    fn screen_size(&self) -> [f32; 2] {
        let r = self
            .ui
            .ctx()
            .input(|i| i.viewport().inner_rect.unwrap_or(egui::Rect::ZERO));
        [r.width(), r.height()]
    }

    fn pixels_per_point(&self) -> f32 {
        self.ui.ctx().pixels_per_point()
    }

    fn request_repaint(&mut self) {
        self.ui.ctx().request_repaint();
    }

    fn request_close(&mut self) {
        self.ui
            .ctx()
            .send_viewport_cmd(egui::ViewportCommand::Close);
        let _ = &self.frame; // mark used
    }
}
