// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Editor bootstrap: CLI parsing, window icon, winit + overlay/shell setup.
//!
//! Owns the binary entry point and the closure handed to `run_winit` that
//! constructs the renderer, the egui overlay, and the editor shell. Keeps
//! `app.rs` focused on `EditorApp` and `EngineApp` semantics.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::ui::EditorOverlay;
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::EditorShell;
use khora_sdk::RenderSystem;
use khora_sdk::WgpuRenderSystem;

use crate::app::EditorApp;

/// CLI project path passed via `--project <path>`.
pub static PROJECT_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Decode the embedded PNG logo into a `WindowIcon`.
pub fn load_logo_icon() -> WindowIcon {
    let png_bytes = include_bytes!("../assets/khora_small_logo.png");
    match image::load_from_memory(png_bytes) {
        Ok(img) => {
            let rgba_img = img.to_rgba8();
            let (w, h) = rgba_img.dimensions();
            WindowIcon {
                rgba: rgba_img.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(e) => {
            log::warn!("Failed to decode logo PNG: {}", e);
            WindowIcon {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}

/// Editor binary entry point. Parses `--project`, then hands control to
/// `run_winit` with a setup closure that wires renderer + overlay + shell.
pub fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let project = args
        .windows(2)
        .find(|w| w[0] == "--project")
        .map(|w| w[1].clone());
    let _ = PROJECT_PATH.set(project);

    run_winit::<WinitWindowProvider, EditorApp>(|window, runtime, event_loop_any| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        runtime.backends.insert(rs.graphics_device());

        // Build the editor overlay (egui) + shell (dock + panels) so the
        // editor UI renders on top of the 3D scene each frame.
        let event_loop = event_loop_any
            .downcast_ref::<winit::event_loop::ActiveEventLoop>()
            .expect("editor: bootstrap expects a winit ActiveEventLoop");
        let theme = khora_sdk::khora_core::ui::UiTheme::default();
        match rs.create_editor_overlay_and_shell(
            event_loop,
            khora_sdk::khora_lanes::render_lane::shaders::EGUI_WGSL,
            khora_sdk::khora_lanes::render_lane::shaders::GRID_WGSL,
            theme,
            khora_sdk::PRIMARY_VIEWPORT,
        ) {
            Ok((overlay, shell)) => {
                let overlay: Box<dyn EditorOverlay> = Box::new(overlay);
                let shell: Box<dyn EditorShell> = Box::new(shell);
                runtime.backends.insert(Arc::new(Mutex::new(overlay)));
                runtime.backends.insert(Arc::new(Mutex::new(shell)));
                log::info!("editor: overlay + shell created");
            }
            Err(e) => {
                log::error!("editor: failed to create overlay+shell: {e:?}");
            }
        }

        let rs: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs)));
    })?;
    Ok(())
}
