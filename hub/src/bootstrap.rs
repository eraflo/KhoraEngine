// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Boot — wires the `HubApp` into the engine's `App` runtime, sets up
//! the window icon, and surfaces the per-frame `update` impl.

use crate::async_pump::pump_async_messages;
use crate::chrome::{paint_banner, show_status_bar, show_topbar};
use crate::screens;
use crate::state::Screen;
use crate::theme::pal;
use crate::widgets::rgba;
use crate::{fonts, theme, HubApp};
use khora_sdk::tool_ui::{
    self as kui, App, AppContext, UiBuilder, WindowConfigInput, WindowIconInput,
};

impl App for HubApp {
    fn on_start(&mut self, ctx: &mut dyn AppContext) {
        ctx.set_fonts(&fonts::build_pack());
        ctx.set_theme(&theme::khora_hub_dark());
    }

    fn update(&mut self, ctx: &mut dyn AppContext) {
        pump_async_messages(self, ctx);

        if self.banner.is_some() {
            ctx.request_repaint();
        }

        ctx.central(&mut |ui| {
            let r = ui.panel_rect();
            ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], rgba(pal::BG), 0.0);

            ui.top_inset_panel("hub_topbar", 44.0, &mut |ui| show_topbar(self, ui));
            ui.bottom_inset_panel("hub_status_bar", 24.0, &mut |ui| show_status_bar(self, ui));

            if let Some(banner) = self.banner.as_ref() {
                paint_banner(ui, banner);
            }

            ui.central_inset(&mut |ui: &mut dyn UiBuilder| match self.screen {
                Screen::Home => screens::show_home(self, ui),
                Screen::NewProject => screens::show_new_project(self, ui),
                Screen::EngineManager => screens::show_engine_manager(self, ui),
                Screen::Settings => screens::show_settings(self, ui),
            });
        });
    }
}

/// Loads the embedded Khora logo and converts it to a [`WindowIconInput`].
fn load_logo_icon() -> WindowIconInput {
    let png_bytes = include_bytes!("../assets/khora_small_logo.png");
    match image::load_from_memory(png_bytes) {
        Ok(img) => {
            let rgba_img = img.to_rgba8();
            let (w, h) = rgba_img.dimensions();
            WindowIconInput {
                rgba: rgba_img.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(e) => {
            log::warn!("Failed to decode logo PNG: {}", e);
            WindowIconInput {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}

/// Hub binary entry point. Sets up logging, builds the window config,
/// hands the `HubApp` to the engine runtime.
pub fn run() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let window = WindowConfigInput {
        title: "Khora Engine Hub".to_owned(),
        width: 1100,
        height: 680,
        icon: Some(load_logo_icon()),
    };

    kui::run_native(window, || Box::new(HubApp::new()))
}
