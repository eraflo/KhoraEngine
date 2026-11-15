// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! [`run_native`]: boots a [`khora_core::ui::App`] under eframe, each frame
//! handed to it through an [`EguiAppContext`].

use anyhow::{anyhow, Result};

use khora_core::ui::App;

use super::EguiAppContext;

/// A `Box<dyn App>` adapted to `eframe::App`, for `eframe::run_native`.
struct AppAdapter {
    inner: Box<dyn App>,
    started: bool,
}

impl eframe::App for AppAdapter {
    // eframe hands the app a root `Ui` to paint into rather than a `Context`
    // to open panels against.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.started {
            {
                let mut adapter = EguiAppContext::new(ui, frame);
                self.inner.on_start(&mut adapter);
            }
            self.started = true;

            // The fonts `on_start` installs take effect at the next pass only:
            // a widget asking now for a named family (`icons`, `display`)
            // would panic. The first frame is left blank.
            ui.ctx().request_repaint();
            return;
        }

        let mut adapter = EguiAppContext::new(ui, frame);
        self.inner.update(&mut adapter);
    }

    // The window behind the panels — the blank first frame included — is the
    // installed theme's panel colour, not eframe's dark default.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.inner.on_exit();
    }
}

/// Boots a tool app on the egui and eframe backend.
///
/// `factory` runs once, after the renderer is up, and returns the app.
///
/// # Errors
///
/// Fails when eframe cannot open the window or start its renderer.
pub fn run_native<F>(window: WindowConfigInput, factory: F) -> Result<()>
where
    F: FnOnce() -> Box<dyn App> + 'static,
{
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(window.title.clone())
        .with_inner_size([window.width as f32, window.height as f32]);
    if let Some(icon) = window.icon {
        viewport = viewport.with_icon(egui::IconData {
            rgba: icon.rgba,
            width: icon.width,
            height: icon.height,
        });
    }
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        &window.title,
        native_options,
        Box::new(move |_cc| {
            let app = factory();
            Ok(Box::new(AppAdapter {
                inner: app,
                started: false,
            }) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow!("eframe::run_native failed: {}", e))?;
    Ok(())
}

/// A tool window's title, size and icon, owned.
///
/// Mirrors `WindowConfig`, kept here so tool-only settings stay out of `khora-core`.
#[derive(Debug, Clone)]
pub struct WindowConfigInput {
    /// Window title.
    pub title: String,
    /// Initial width in pixels.
    pub width: u32,
    /// Initial height in pixels.
    pub height: u32,
    /// Optional window icon.
    pub icon: Option<WindowIconInput>,
}

impl Default for WindowConfigInput {
    fn default() -> Self {
        Self {
            title: "Khora Tool".to_owned(),
            width: 1024,
            height: 720,
            icon: None,
        }
    }
}

/// The window icon, as RGBA8 pixels and their size.
#[derive(Debug, Clone)]
pub struct WindowIconInput {
    /// Row-major RGBA8 pixels.
    pub rgba: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}
