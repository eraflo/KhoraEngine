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

//! Default game runtime entry-point shared by `khora-runtime` and any
//! native-Rust user project that doesn't need a custom `EngineApp`.
//!
//! [`run_default`] is the canonical "data-only Khora game" main:
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     khora_sdk::run_default()
//! }
//! ```
//!
//! It auto-detects whether to read assets from a packed archive
//! (`<exe-dir>/data.pack` + `<exe-dir>/index.bin`) or a loose
//! `<exe-dir>/assets/` directory, registers every default decoder, loads
//! the scene named in `<exe-dir>/runtime.json`, and hands control to the
//! main loop. Users who need custom components / agents / lanes write
//! their own `EngineApp` impl and call [`crate::run_winit`] directly.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::runtime_config::RuntimeConfig;
use crate::winit_adapters::WinitWindowProvider;
use crate::{
    run_winit, AssetService, AudioDevice, AudioMixBus, AudioStream, CpalAudioDevice, DefaultMixBus,
    EngineApp, GameWorld, InputEvent, LayoutSystem, MetricsRegistry, PackLoader, PhaseProvider,
    PhysicsProvider, PipelineSystem, RapierPhysicsWorld, RenderSystem, Runtime, StreamInfo,
    TaffyLayoutSystem, WgpuPipelineSystem, WgpuRenderSystem, WindowConfig,
};
use khora_io::asset::PackManifest;

mod opening;
use opening::{is_packed, GameOpening, GAME_DIR};

/// Runtime config the launcher (editor's "Build Game") drops next to the
/// binary. Read at startup; sensible defaults are used when the file is
/// missing (typical for engine contributors running the runtime against a
/// loose `assets/` directory).
static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

/// Builds the [`AssetService`] the game starts with, by auto-detecting the
/// loader: a pack's, read from its precomputed `index.bin`; an empty one for
/// a loose `assets/` directory, which the app files to be indexed off the
/// frame once the engine runs. See module docs for the precedence rules.
///
/// When `verify_integrity` is `true` and a packed runtime is detected,
/// `manifest.bin` is read alongside `data.pack` and threaded into the
/// service so each load is hashed against its recorded BLAKE3 digest.
/// A missing or malformed manifest logs a warning but never aborts
/// startup — the runtime stays bootable on packs built before manifests
/// were emitted.
fn build_asset_service(
    exe_dir: &Path,
    metrics: Arc<MetricsRegistry>,
    verify_integrity: bool,
) -> Result<AssetService> {
    let pack = exe_dir.join("data.pack");
    let idx = exe_dir.join("index.bin");
    let assets = exe_dir.join("assets");

    let (svc, mode_label) = if is_packed(exe_dir) {
        let bytes =
            std::fs::read(&idx).with_context(|| format!("Failed to read {}", idx.display()))?;
        let pack_file = std::fs::File::open(&pack)
            .with_context(|| format!("Failed to open {}", pack.display()))?;
        let loader = PackLoader::new(pack_file)
            .context("Pack header validation failed — refusing to start")?;
        let manifest = if verify_integrity {
            read_manifest(exe_dir)
        } else {
            None
        };
        let mut svc = AssetService::new(&bytes, Box::new(loader), metrics, manifest)?;
        svc.register_default_decoders(exe_dir);
        (svc, "PackLoader")
    } else if assets.is_dir() {
        // The manifest is a release-mode artifact emitted alongside
        // `data.pack`: in dev mode the bytes come straight off disk, so there
        // is nothing to verify against.
        if verify_integrity {
            log::info!(
                "khora-sdk run_default: verify_integrity=true ignored in dev mode \
                 (no pack to verify against)"
            );
        }
        // Indexed off the frame once the engine runs, with the identity
        // registry the editor writes: an asset renamed in the editor keeps
        // the UUID its scenes reference.
        (AssetService::empty(), "FileLoader, indexed off the frame")
    } else {
        return Err(anyhow!(
            "khora-sdk run_default cannot start: no `data.pack`+`index.bin` and no \
             `assets/` next to the binary at {}",
            exe_dir.display()
        ));
    };

    log::info!(
        "khora-sdk run_default: using {} ({} assets indexed)",
        mode_label,
        svc.index().asset_count()
    );

    Ok(svc)
}

/// Reads `manifest.bin` next to `data.pack`. A missing or malformed manifest
/// logs a warning and disables verification rather than aborting startup.
fn read_manifest(exe_dir: &Path) -> Option<PackManifest> {
    let manifest_path = exe_dir.join("manifest.bin");
    match std::fs::read(&manifest_path) {
        Ok(bytes) => match PackManifest::decode(&bytes) {
            Ok(m) => {
                log::info!(
                    "khora-sdk run_default: integrity verification enabled ({} entries)",
                    m.len()
                );
                Some(m)
            }
            Err(e) => {
                log::warn!(
                    "khora-sdk run_default: manifest.bin present but malformed ({}) — \
                     integrity verification disabled",
                    e
                );
                None
            }
        },
        Err(_) => {
            log::warn!(
                "khora-sdk run_default: verify_integrity=true but {} is missing — \
                 integrity verification disabled",
                manifest_path.display()
            );
            None
        }
    }
}

/// The default `EngineApp` used by [`run_default`]. It opens the game
/// beside the binary off the frame — its assets, the scene named in
/// `runtime.json`, its scripts — and ticks idly afterwards: gameplay lives in
/// the scene's scripts, which the engine runs itself while the game plays.
struct DefaultRuntimeApp {
    exe_dir: PathBuf,
    config: RuntimeConfig,
    opening: Option<GameOpening>,
    frame_count: u64,
}

impl DefaultRuntimeApp {
    /// The runtime app of the game beside `exe_dir`, run with `config`.
    /// [`EngineApp::new`] gives the one of the running binary.
    fn at(exe_dir: PathBuf, config: RuntimeConfig) -> Self {
        Self {
            exe_dir,
            config,
            opening: None,
            frame_count: 0,
        }
    }
}

/// Installs in `runtime` the asset service the game beside `exe_dir` starts
/// with: a pack's, read from its precomputed `index.bin`; an empty one for a
/// loose `assets/` directory, whose open the app files once the engine runs;
/// none, with the error logged, when there is neither.
fn install_game_assets(runtime: &mut Runtime, exe_dir: &Path, verify_integrity: bool) {
    let metrics = Arc::new(MetricsRegistry::new());
    match build_asset_service(exe_dir, metrics, verify_integrity) {
        Ok(svc) => runtime.services.insert(Arc::new(Mutex::new(svc))),
        Err(e) => log::error!("khora-sdk run_default: AssetService init failed: {:#}", e),
    }
}

impl EngineApp for DefaultRuntimeApp {
    /// A game keeps what the engine learns of the machine it runs on.
    fn keeps_machine_profiles() -> bool {
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

/// Boots a Khora game with the default runtime app: auto-detects pack vs
/// loose assets, registers every default decoder, and loads the scene
/// named in `runtime.json`. This is what the pre-built `khora-runtime`
/// binary calls and what a user project's `src/main.rs` should call when
/// it doesn't need to register custom components/agents/lanes.
///
/// Returns `Err` only on irrecoverable startup failures (no assets at
/// all, missing exe path). Per-frame errors are logged and the loop
/// continues.
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
        "khora-sdk run_default: project='{}' from {} (preset={}, verify_integrity={})",
        cfg.project_name,
        exe_dir.display(),
        cfg.preset.as_deref().unwrap_or("unset"),
        cfg.verify_integrity,
    );

    let verify_integrity = cfg.verify_integrity;
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

        // The game's assets: a pack's index now, loose files indexed once the
        // engine runs. Its scene and its scripts are filed in `setup`, and
        // loaded off the frame.
        install_game_assets(runtime, &exe_dir, verify_integrity);
        // A packed game runs on a player's machine: its profile is filed in
        // the player's data folder, and it may ship one beside it.
        if opening::is_packed(&exe_dir) {
            runtime
                .resources
                .insert(khora_io::profiles::ProfilePlace::Player);
            runtime
                .resources
                .insert(crate::engine::profiles::ShippedProfileDir(exe_dir.clone()));
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
