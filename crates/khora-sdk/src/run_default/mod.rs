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

//! [`run_default`], the entry point of a data-only game: the prebuilt
//! `khora-runtime`, and any project that needs no `EngineApp` of its own.
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     khora_sdk::run_default()
//! }
//! ```

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::game_assets::{check_pack_seal, open_game_assets_with};
use crate::runtime_config::RuntimeConfig;
use crate::winit_adapters::WinitWindowProvider;
use crate::{
    run_winit, AudioDevice, AudioMixBus, AudioStream, CpalAudioDevice, DefaultMixBus, EngineApp,
    GameWorld, InputEvent, LayoutSystem, PhaseProvider, PhysicsProvider, PipelineSystem,
    RapierPhysicsWorld, RenderSystem, Runtime, StreamInfo, TaffyLayoutSystem, WgpuPipelineSystem,
    WgpuRenderSystem, WindowConfig,
};
use khora_io::asset::pack_key::stamped_pack_key;
use khora_io::asset::signing::PackKey;

mod opening;
use opening::{GameOpening, GAME_DIR};

/// The `runtime.json` [`run_default`] read, for the app's functions that take no `self`.
static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

/// The [`EngineApp`] of [`run_default`]: it opens the game beside the binary, then idles.
///
/// The scene `runtime.json` names is opened off the frame, and so are loose
/// assets and the game's scripts; a packed game's index is read before the
/// boot. Gameplay lives in the scene's scripts.
pub(crate) struct DefaultRuntimeApp {
    exe_dir: PathBuf,
    config: RuntimeConfig,
    opening: Option<GameOpening>,
    frame_count: u64,
}

impl DefaultRuntimeApp {
    /// Returns the app of the game beside `exe_dir`, run with `config`.
    ///
    /// [`EngineApp::new`] builds the running binary's.
    pub(crate) fn at(exe_dir: PathBuf, config: RuntimeConfig) -> Self {
        Self {
            exe_dir,
            config,
            opening: None,
            frame_count: 0,
        }
    }
}

impl EngineApp for DefaultRuntimeApp {
    /// A game keeps what the engine learns of the machine it runs on.
    fn keeps_user_files() -> bool {
        true
    }

    /// The game `runtime.json` names: its project, at its version.
    fn game_identity() -> khora_core::app::GameIdentity {
        RUNTIME_CONFIG
            .get_or_init(RuntimeConfig::defaults)
            .game_identity()
    }

    fn window_config() -> WindowConfig {
        let cfg = RUNTIME_CONFIG.get_or_init(RuntimeConfig::defaults);
        WindowConfig {
            title: cfg.window_title(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        log::info!("DefaultRuntimeApp: instantiated");
        Self::at(
            GAME_DIR.get().cloned().unwrap_or_default(),
            RUNTIME_CONFIG
                .get()
                .cloned()
                .unwrap_or_else(RuntimeConfig::defaults),
        )
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        self.opening = GameOpening::start(
            &self.exe_dir,
            self.config.default_scene.clone(),
            world,
            runtime,
        );
    }

    fn update(&mut self, world: &mut GameWorld, _inputs: &[InputEvent]) {
        if let Some(opening) = &mut self.opening {
            opening.advance(world);
        }
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(600) {
            log::info!("khora-sdk run_default: frame {}", self.frame_count);
        }
    }

    fn startup_ready(&mut self) {
        log::info!(
            "khora-sdk run_default: '{}' is loaded",
            self.config.project_name
        );
    }
}

impl PhaseProvider for DefaultRuntimeApp {
    fn custom_phases(&self) -> Vec<crate::ExecutionPhase> {
        Vec::new()
    }
    fn removed_phases(&self) -> Vec<crate::ExecutionPhase> {
        Vec::new()
    }
}

/// Runs the game beside the binary with the default app, until its window closes.
///
/// It reads `runtime.json`, opens the pack beside the binary or its loose
/// `assets/`, and loads the scene `runtime.json` names. The prebuilt
/// `khora-runtime` calls it, as does a project with no components, agents or
/// lanes of its own. See the book: concepts/trust.md, "Signed packs".
///
/// # Errors
///
/// - When the binary's own path cannot be read.
/// - When a stamped runtime finds no complete pack, or the pack's seal does not
///   verify with the key stamped into the binary.
/// - When the binary has no stamped key and a pack sits beside it.
/// - When `index.bin` cannot be read or is over its size limit.
/// - When the winit event loop cannot be created, or ends in an error.
///
/// Errors after start-up are logged, not returned.
///
/// # Panics
///
/// When the platform refuses to open the window, the renderer cannot start, or
/// the engine cannot boot, as [`run_winit`] says.
pub fn run_default() -> Result<()> {
    let exe_dir = std::env::current_exe()
        .context("Failed to query current_exe path")?
        .parent()
        .context("current_exe has no parent directory")?
        .to_path_buf();

    let cfg = RuntimeConfig::load_or_default(&exe_dir);
    let _ = RUNTIME_CONFIG.set(cfg.clone());
    let _ = GAME_DIR.set(exe_dir.clone());

    log::info!(
        "khora-sdk run_default: project='{}' from {} (preset={})",
        cfg.project_name,
        exe_dir.display(),
        cfg.preset.as_deref().unwrap_or("unset"),
    );
    // A pack that is not sealed by this runtime's key never starts: refused
    // here as an error, before a window opens, as the boot would refuse it.
    check_pack_seal(&exe_dir, stamped_pack_key())?;

    let physics_settings = cfg.physics;
    run_winit::<WinitWindowProvider, DefaultRuntimeApp>(move |window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        let monitors = rs.init(window).expect("renderer init failed");
        runtime
            .resources
            .insert(crate::khora_core::telemetry::monitoring::HostMonitors(
                monitors,
            ));
        runtime.backends.insert(rs.graphics_device());
        let rs_dyn: Box<dyn RenderSystem> = Box::new(rs);
        runtime.backends.insert(Arc::new(Mutex::new(rs_dyn)));

        // Shader / pipeline backend — wgpu + naga_oil. The app picks the
        // backend; the engine core consumes it as `Arc<dyn PipelineSystem>`.
        match WgpuPipelineSystem::new() {
            Ok(sys) => {
                let sys: Arc<dyn PipelineSystem> = Arc::new(sys);
                runtime.resources.insert(sys);
            }
            Err(e) => log::error!("pipeline system init failed: {e}"),
        }

        // Physics — Rapier3D, stepped at the game's `physics.step`.
        let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));
        runtime.resources.insert(physics_settings);

        // UI layout — Taffy. Text is shaped and rasterised by the engine's
        // own backends, which the boot installs.
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Audio — shared mix bus + CPAL device. The opened AudioStream
        // is stored as a backend handle; dropping it stops the stream.
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

        // A pack's index is read now, loose files indexed once the engine
        // runs; the scene and scripts are filed in `setup`.
        install_game(runtime, &exe_dir, stamped_pack_key());
    })?;
    Ok(())
}

/// Installs in `runtime` what the game beside `exe_dir` runs with, its pack verified with `pack_key`.
///
/// No asset service is installed, the error logged, when there is neither a
/// pack nor `assets/`, or the pack is refused.
fn install_game(runtime: &mut Runtime, exe_dir: &Path, pack_key: Option<PackKey>) {
    if let Err(e) = open_game_assets_with::<DefaultRuntimeApp>(runtime, exe_dir, pack_key) {
        log::error!("khora-sdk run_default: the game's assets did not open: {e:#}");
    }
}

#[cfg(test)]
pub(crate) mod tests;
