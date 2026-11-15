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
    AudioDevice, AudioMixBus, CpalAudioDevice, DefaultMixBus, HostMonitors, LayoutSystem,
    PhysicsProvider, PipelineSystem, RapierPhysicsWorld, RenderSystem, StreamInfo,
    TaffyLayoutSystem, WgpuPipelineSystem, WgpuRenderSystem,
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
        let monitors = rs.init(window).expect("renderer init failed");
        runtime.resources.insert(HostMonitors(monitors));
        // The device is a backend of its own: lanes reach it apart from the render system.
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

        // UI layout — Taffy. Text is shaped and rasterised by the engine's
        // own backends, which the boot installs.
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Audio — shared mix bus + CPAL device. The bus is the only meeting
        // point of the audio lanes and the hardware callback's real-time thread.
        // The opened stream is kept as a backend: dropping it stops the sound.
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

        // `.wgsl` hot-reload, an engine-dev convenience: an edited shader
        // rebuilds its pipelines in place. The tree is found from this crate's
        // compile-time location, so only a run from the checkout has it; a
        // relocated binary serves the embedded shaders.
        let shader_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/khora-infra/src/graphics/shader/shaders");
        if let Ok(shader_dir) = shader_dir.canonicalize() {
            let watched = runtime
                .resources
                .get::<Arc<khora_sdk::AssetWatch>>()
                .map(|watch| watch.watch(&shader_dir));
            match watched {
                Some(Ok(())) => log::info!(
                    "sandbox: watching {} for shader hot-reload",
                    shader_dir.display()
                ),
                Some(Err(e)) => log::warn!(
                    "sandbox: shader hot-reload disabled ({}): {:#}",
                    shader_dir.display(),
                    e
                ),
                None => log::warn!("sandbox: the engine holds no AssetWatch"),
            }
        }

        // Ergon scripts, found like the shaders and hot-reloaded too. Mounted
        // before the boot, not in `setup`: the boot compiles a mount filed
        // before it, so `setup` finds `Hover` declared and can attach it.
        let script_assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        khora_sdk::scripts::mount(runtime, &script_assets);
    })?;
    Ok(())
}
