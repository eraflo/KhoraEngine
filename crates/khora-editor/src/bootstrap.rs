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
//! `app/` focused on `EditorApp` and `EngineApp` semantics.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::ui::EditorOverlay;
use khora_sdk::prelude::*;
use khora_sdk::winit;
use khora_sdk::winit_adapters::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AudioDevice, AudioMixBus, AudioStream, CpalAudioDevice, DefaultMixBus, EditorShell,
    LayoutSystem, PhysicsProvider, PipelineSystem, RapierPhysicsWorld, RenderSystem, StreamInfo,
    TaffyLayoutSystem, WgpuPipelineSystem, WgpuRenderSystem,
};

use crate::app::EditorApp;

/// CLI project path passed via `--project <path>`.
pub static PROJECT_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The Khora logo as the editor's window icon.
pub fn load_logo_icon() -> WindowIcon {
    let logo = khora_tool_ui::logo::logo_icon();
    WindowIcon {
        rgba: logo.rgba,
        width: logo.width,
        height: logo.height,
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

    // What a frame needs: the renderer, then the overlay and the shell the
    // loading film is painted with, then the backends the engine boots its
    // families over. Light work only: the project, its scene and its scripts
    // are filed with the engine's families once it runs.
    run_winit::<WinitWindowProvider, EditorApp>(|window, runtime, event_loop_any| {
        let mut rs = WgpuRenderSystem::new();
        match rs.init(window) {
            Ok(monitors) => runtime.resources.insert(khora_sdk::HostMonitors(monitors)),
            Err(e) => {
                log::error!("editor: renderer init failed: {e:?}");
                return;
            }
        }
        runtime.backends.insert(rs.graphics_device());

        let Some(event_loop) = event_loop_any.downcast_ref::<winit::event_loop::ActiveEventLoop>()
        else {
            log::error!("editor: bootstrap expects a winit ActiveEventLoop");
            return;
        };
        let theme = khora_sdk::khora_core::ui::UiTheme::default();
        match rs.create_editor_overlay_and_shell(
            event_loop,
            khora_sdk::EGUI_WGSL,
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

        // Shader / pipeline backend — wgpu + naga_oil. Registering the shader
        // library is a few milliseconds; composing a pipeline happens when a
        // lane first asks for it.
        match WgpuPipelineSystem::new() {
            Ok(sys) => {
                let sys: Arc<dyn PipelineSystem> = Arc::new(sys);
                runtime.resources.insert(sys);
            }
            Err(e) => log::error!("pipeline system init failed: {e}"),
        }

        // The engine boots its physics family at the project's
        // `physics.step`: read from its `project.json` now, before the boot.
        if let Some(Some(project)) = PROJECT_PATH.get() {
            runtime
                .resources
                .insert(crate::project_physics::project_physics(
                    std::path::Path::new(project),
                ));
        }

        // Physics — Rapier3D
        let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));

        // UI layout — Taffy. Text is shaped and rasterised by the engine's
        // own backends, which the boot installs.
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Audio — shared mix bus + CPAL device. The bus is the sole
        // synchronisation boundary between audio lanes (main thread) and the
        // backend's hardware callback (RT thread). The opened AudioStream is
        // stored as a backend handle; dropping it stops the stream.
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 48_000,
        };
        let mix_bus: Arc<dyn AudioMixBus> = Arc::new(DefaultMixBus::new(stream_info, 8192));
        runtime.resources.insert(Arc::clone(&mix_bus));
        let device: Box<dyn AudioDevice> = Box::new(CpalAudioDevice::new());
        match device.open(mix_bus) {
            Ok(stream) => {
                let stream: Arc<dyn AudioStream> = Arc::from(stream);
                runtime.backends.insert(stream);
            }
            Err(e) => log::error!("audio open failed: {}", e),
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The embedded logo decodes into the window icon. A PNG the `image` build
    /// cannot read falls back to a transparent 1x1 icon with only a warning,
    /// so nothing else notices a lost `png` feature or a bad asset.
    #[test]
    fn the_window_icon_is_the_decoded_logo() {
        let icon = super::load_logo_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}
