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
use khora_io::asset::pack_key::stamped_pack_key;
use khora_io::asset::signing::{PackKey, PackSealError, SignedManifest};
use khora_io::asset::PackManifest;

mod opening;
use opening::{is_packed, GameOpening, GAME_DIR};

/// The `runtime.json` [`run_default`] read, for the app's functions that take no `self`.
static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

/// Builds the [`AssetService`] the game starts with: a pack's, or an empty one for loose `assets/`.
///
/// Loose assets are indexed off the frame once the engine runs, unverified. A
/// pack opens only if its seal verifies with `pack_key` ([`read_manifest`]);
/// each load is then hashed against the manifest. See the book:
/// concepts/trust.md, "Signed packs".
fn build_asset_service(
    exe_dir: &Path,
    metrics: Arc<MetricsRegistry>,
    refusals: Option<Arc<khora_io::trust::Refusals>>,
    pack_key: Option<PackKey>,
) -> Result<AssetService> {
    let pack = exe_dir.join("data.pack");
    let idx = exe_dir.join("index.bin");
    let assets = exe_dir.join("assets");

    let (svc, mode_label) = if is_packed(exe_dir) {
        // An index is read whole: one past its limit is refused before.
        let idx_len = std::fs::metadata(&idx)
            .with_context(|| format!("Failed to read {}", idx.display()))?
            .len();
        if idx_len > khora_io::asset::MAX_INDEX_BYTES as u64 {
            return Err(anyhow!(
                "{} is {idx_len} bytes, over the {} an index may hold",
                idx.display(),
                khora_io::asset::MAX_INDEX_BYTES
            ));
        }
        let bytes =
            std::fs::read(&idx).with_context(|| format!("Failed to read {}", idx.display()))?;
        let pack_file = std::fs::File::open(&pack)
            .with_context(|| format!("Failed to open {}", pack.display()))?;
        let loader = PackLoader::new(pack_file)
            .context("Pack header validation failed — refusing to start")?;
        let manifest = read_manifest(exe_dir, &bytes, pack_key)?;
        let mut svc = AssetService::new(&bytes, Box::new(loader), metrics, Some(manifest))?;
        if let Some(refusals) = refusals {
            svc.set_refusals(refusals);
        }
        // A glTF's buffers and images are read inside the pack.
        svc.register_packed_decoders();
        (svc, "PackLoader")
    } else if assets.is_dir() {
        refuse_loose_when_stamped(exe_dir, pack_key)?;
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

/// Returns the pack's `manifest.bin`, verified with `pack_key` over the index bytes `index_bytes`.
///
/// A missing, malformed or unsigned manifest, another key, another index, or
/// no key at all is a [`PackSealError`], logged, which refuses the pack.
fn read_manifest(
    exe_dir: &Path,
    index_bytes: &[u8],
    pack_key: Option<PackKey>,
) -> Result<PackManifest> {
    let seal = (|| {
        let key = pack_key.ok_or(PackSealError::Unstamped)?;
        SignedManifest::read(&exe_dir.join("manifest.bin"))?.verify(&key, index_bytes)
    })();
    match seal {
        Ok(manifest) => {
            log::info!(
                "khora-sdk run_default: the pack's seal verifies ({} entries)",
                manifest.len()
            );
            Ok(manifest)
        }
        Err(error) => {
            log::error!("khora-sdk run_default: the pack is refused: {error}");
            Err(anyhow::Error::new(error)
                .context(format!("the pack beside {} is refused", exe_dir.display())))
        }
    }
}

/// Refuses loose files to a stamped runtime: a built game runs only from its pack.
///
/// A pack missing a part, or `assets/` in its place, is refused. An unstamped
/// runtime (a dev run) runs loose files.
fn refuse_loose_when_stamped(exe_dir: &Path, pack_key: Option<PackKey>) -> Result<()> {
    if pack_key.is_none() {
        return Ok(());
    }
    log::error!(
        "khora-sdk run_default: this built game finds no complete pack beside it at {}",
        exe_dir.display()
    );
    Err(anyhow!(
        "a built game runs from its pack: `data.pack` and `index.bin` are not both beside {} — \
         build the game again",
        exe_dir.display()
    ))
}

/// Refuses the pack beside `exe_dir` before anything starts when its seal
/// does not verify with `pack_key`; a loose game has nothing to verify.
fn check_pack_seal(exe_dir: &Path, pack_key: Option<PackKey>) -> Result<()> {
    if !is_packed(exe_dir) {
        return refuse_loose_when_stamped(exe_dir, pack_key);
    }
    let idx = exe_dir.join("index.bin");
    let len = std::fs::metadata(&idx)
        .with_context(|| format!("Failed to read {}", idx.display()))?
        .len();
    if len > khora_io::asset::MAX_INDEX_BYTES as u64 {
        return Err(anyhow!(
            "{} is {len} bytes, over the {} an index may hold",
            idx.display(),
            khora_io::asset::MAX_INDEX_BYTES
        ));
    }
    let bytes = std::fs::read(&idx).with_context(|| format!("Failed to read {}", idx.display()))?;
    read_manifest(exe_dir, &bytes, pack_key).map(|_| ())
}

/// The [`EngineApp`] of [`run_default`]: it opens the game beside the binary, then idles.
///
/// The scene `runtime.json` names is opened off the frame, and so are loose
/// assets and their scripts; a packed game's index is read before the boot,
/// and it mounts no script. Gameplay lives in the scene's scripts.
struct DefaultRuntimeApp {
    exe_dir: PathBuf,
    config: RuntimeConfig,
    opening: Option<GameOpening>,
    frame_count: u64,
}

impl DefaultRuntimeApp {
    /// Returns the app of the game beside `exe_dir`, run with `config`.
    ///
    /// [`EngineApp::new`] builds the running binary's.
    fn at(exe_dir: PathBuf, config: RuntimeConfig) -> Self {
        Self {
            exe_dir,
            config,
            opening: None,
            frame_count: 0,
        }
    }
}

/// Installs in `runtime` the asset service of the game beside `exe_dir`.
///
/// None is installed, the error logged, when there is neither a pack nor
/// `assets/`, or the pack is refused.
fn install_game_assets(runtime: &mut Runtime, exe_dir: &Path, pack_key: Option<PackKey>) {
    let metrics = Arc::new(MetricsRegistry::new());
    let refusals = runtime
        .resources
        .get::<Arc<khora_io::trust::Refusals>>()
        .cloned();
    match build_asset_service(exe_dir, metrics, refusals, pack_key) {
        Ok(svc) => runtime.services.insert(Arc::new(Mutex::new(svc))),
        Err(e) => log::error!("khora-sdk run_default: AssetService init failed: {:#}", e),
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
    // A pack that is not sealed by this runtime's key never starts.
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

        // A packed game runs on a player's machine: its profile and its
        // quarantine are filed in the player's data folder, and it may ship
        // a profile beside it.
        if opening::is_packed(&exe_dir) {
            runtime
                .resources
                .insert(khora_io::profiles::ProfilePlace::Player);
            runtime
                .resources
                .insert(crate::engine::profiles::ShippedProfileDir(exe_dir.clone()));
        }
        // The run's refusals before the asset service: its readers report
        // to them from the first.
        crate::engine::refusals::install(
            runtime,
            &DefaultRuntimeApp::game_identity(),
            DefaultRuntimeApp::keeps_user_files(),
        );
        // A pack's index is read now, loose files indexed once the engine
        // runs; the scene and scripts are filed in `setup`.
        install_game_assets(runtime, &exe_dir, stamped_pack_key());
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
