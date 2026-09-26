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

//! Sandbox example demonstrating high-level game logic with Khora Engine.
//!
//! This example shows how to build a game using only the SDK's public API.
//! No low-level rendering details - pure game logic.
//!
//! Controls:
//! - Right mouse button + drag: Look around
//! - W/A/S/D: Move forward/left/backward/right
//! - Space: Move up
//! - Shift: Move down

use anyhow::Result;
use game::SandboxGame;
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AudioDevice, AudioMixBus, CpalAudioDevice, DefaultMixBus, LayoutSystem, PhysicsProvider,
    PipelineSystem, RapierPhysicsWorld, RenderSystem, StandardTextRenderer, StreamInfo,
    TaffyLayoutSystem, TextRenderer, WgpuPipelineSystem, WgpuRenderSystem, TEXT_WGSL,
};
use std::sync::{Arc, Mutex};

mod assets;
mod game;
mod player;

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

fn main() -> Result<()> {
    use env_logger::{Builder, Env};

    Builder::from_env(Env::default().default_filter_or("info"))
        // Suppress Epic Games / EOS overlay Vulkan loader JSON-not-found noise.
        // These are harmless OS-level loader warnings, not engine errors.
        .filter_module("wgpu_hal::vulkan::instance", log::LevelFilter::Off)
        .init();

    run_winit::<WinitWindowProvider, SandboxGame>(|window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        // Register the graphics device before boxing — required by RenderAgent.
        runtime.backends.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs)));

        // Shader / pipeline backend — wgpu + naga_oil. The app picks the
        // backend; the engine core consumes it as `Arc<dyn PipelineSystem>`.
        match WgpuPipelineSystem::new() {
            Ok(sys) => {
                let sys: Arc<dyn PipelineSystem> = Arc::new(sys);
                runtime.resources.insert(sys);
            }
            Err(e) => log::error!("pipeline system init failed: {e}"),
        }

        // Physics — Rapier3D
        let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));

        // UI layout — Taffy
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Text renderer — StandardTextRenderer
        let text: Arc<dyn TextRenderer> = Arc::new(StandardTextRenderer::new(TEXT_WGSL.to_owned()));
        runtime.backends.insert(text);

        // Audio — shared mix bus + CPAL device. The bus is the sole
        // synchronisation boundary between audio lanes (main thread) and
        // the backend's hardware callback (RT thread). The opened
        // AudioStream is stored as a backend handle; dropping it stops
        // the stream.
        let stream_info = StreamInfo {
            channels: 2,
            sample_rate: 48_000,
        };
        let mix_bus: Arc<dyn AudioMixBus> = Arc::new(DefaultMixBus::new(stream_info, 8192));
        runtime.resources.insert(Arc::clone(&mix_bus));
        let device: Box<dyn AudioDevice> = Box::new(CpalAudioDevice::new());
        match device.open(mix_bus) {
            Ok(stream) => {
                let stream: Arc<dyn khora_sdk::AudioStream> = Arc::from(stream);
                runtime.backends.insert(stream);
            }
            Err(e) => log::error!("audio open failed: {}", e),
        }

        // `.wgsl` hot-reload (engine-dev convenience). Watch the canonical
        // shader source tree so edits to lighting / shadow / material WGSL
        // recompose the affected modules and rebuild the cached pipelines in
        // place — no rebuild, no lane change (the `shader_hot_reload` data
        // system pumps the watcher each tick; lanes re-fetch pipelines by key).
        // The directory is resolved from this crate's compile-time location, so
        // it only exists when the sandbox runs from the source checkout; a
        // relocated binary skips hot-reload and serves the embedded shaders.
        let shader_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/khora-infra/src/graphics/shader/shaders");
        if let Ok(shader_dir) = shader_dir.canonicalize() {
            match khora_sdk::AssetWatcher::new(&shader_dir) {
                Ok(watcher) => {
                    runtime.resources.insert(Arc::new(watcher));
                    log::info!(
                        "sandbox: watching {} for shader hot-reload",
                        shader_dir.display()
                    );
                }
                Err(e) => log::warn!(
                    "sandbox: shader hot-reload disabled ({}): {:#}",
                    shader_dir.display(),
                    e
                ),
            }
        }

        // Ergon. Resolved from this crate's compile-time location for the same
        // reason as the shaders above: the sandbox is run from the checkout.
        //
        // The watcher slot is already spent on the shader tree, so `mount`
        // compiles what is there and leaves watching alone — the scripts run,
        // and editing one takes a restart until the multi-root watcher lands.
        let script_assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        khora_sdk::scripts::mount(runtime, &script_assets);
    })?;
    Ok(())
}
