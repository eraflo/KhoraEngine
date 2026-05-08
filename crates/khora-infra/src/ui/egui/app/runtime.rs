// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `run_native` — boots an [`khora_core::ui::App`] under eframe.
//!
//! This is the concrete equivalent of `eframe::run_native`: it sets
//! the window title / icon / size from a [`WindowConfig`], drives the
//! per-frame `update` against the user's `App` impl through an
//! [`EguiAppContext`] adapter, and surfaces a friendly error type on
//! boot failure.

use anyhow::{anyhow, Result};

use khora_core::ui::App;

use super::EguiAppContext;

/// Bridge struct that adapts a `Box<dyn App>` to the `eframe::App`
/// trait so we can hand it to `eframe::run_native`.
struct AppAdapter {
    inner: Box<dyn App>,
}

impl eframe::App for AppAdapter {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let mut adapter = EguiAppContext::new(ctx, frame);
        self.inner.update(&mut adapter);
    }
}

/// Boot a tool app on the egui+eframe backend.
///
/// `factory` is called once after the renderer is up and must return
/// the `App` instance. Mirrors `eframe::run_native`'s creation
/// callback shape but receives the neutral context type.
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
            Ok(Box::new(AppAdapter { inner: app }) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow!("eframe::run_native failed: {}", e))?;
    Ok(())
}

/// Owned-data window config — mirrors fields of `WindowConfig` but
/// stays in `khora-infra` so we don't drag the platform module
/// definition all the way down to `khora-core` for tool-only use.
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

/// RGBA8 pixel buffer + dimensions for the window icon.
#[derive(Debug, Clone)]
pub struct WindowIconInput {
    /// Row-major RGBA8 pixels.
    pub rgba: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}
