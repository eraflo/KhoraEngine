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
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::khora_core::asset::AssetUUID;
use crate::runtime_config::RuntimeConfig;
use crate::winit_adapters::WinitWindowProvider;
use crate::{
    run_winit, AgentProvider, AssetService, AudioDevice, AudioMixBus, AudioStream, CpalAudioDevice,
    DccService, DefaultMixBus, EngineApp, GameWorld, InputEvent, LayoutSystem, MetricsRegistry,
    PackLoader, PhaseProvider, PhysicsProvider, PipelineSystem, RapierPhysicsWorld, RenderSystem,
    Runtime, SceneFile, SerializationService, StandardTextRenderer, StreamInfo, TaffyLayoutSystem,
    TextRenderer, WgpuPipelineSystem, WgpuRenderSystem, WindowConfig, TEXT_WGSL,
};
use khora_io::asset::{AssetIdRegistry, PackManifest};

/// Runtime config the launcher (editor's "Build Game") drops next to the
/// binary. Read at startup; sensible defaults are used when the file is
/// missing (typical for engine contributors running the runtime against a
/// loose `assets/` directory).
static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

/// Builds an [`AssetService`] by auto-detecting the loader. See module
/// docs for the precedence rules.
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

    let (svc, mode_label) = if pack.is_file() && idx.is_file() {
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
        // The same identity registry the editor writes, so an asset renamed
        // in the editor keeps the UUID its scenes reference.
        let registry = AssetIdRegistry::load(exe_dir);
        let svc = AssetService::open_loose_files(exe_dir, &registry, metrics)
            .context("Failed to build dev-mode in-memory index")?;
        (svc, "FileLoader")
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

/// The default `EngineApp` used by [`run_default`]. It loads the scene
/// named in `runtime.json` and ticks idly afterwards — gameplay scripts
/// will hook into `update` once the scripting runtime lands.
struct DefaultRuntimeApp {
    frame_count: u64,
}

impl EngineApp for DefaultRuntimeApp {
    fn window_config() -> WindowConfig {
        let cfg = RUNTIME_CONFIG.get_or_init(RuntimeConfig::defaults);
        WindowConfig {
            title: cfg.window_title(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        log::info!("DefaultRuntimeApp: instantiated");
        Self { frame_count: 0 }
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        let svc = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();
        let cfg = RUNTIME_CONFIG
            .get()
            .cloned()
            .unwrap_or_else(RuntimeConfig::defaults);

        let Some(svc) = svc else {
            log::error!("DefaultRuntimeApp: AssetService missing from runtime.services");
            return;
        };

        let uuid = AssetUUID::new_v5(&cfg.default_scene);
        let bytes = match svc.lock() {
            Ok(mut s) => s.load_raw(&uuid).ok(),
            Err(_) => None,
        };
        let Some(bytes) = bytes else {
            log::warn!(
                "DefaultRuntimeApp: default scene '{}' not found in VFS — \
                 starting with an empty world",
                cfg.default_scene
            );
            return;
        };

        match SceneFile::from_bytes(&bytes) {
            Ok(scene) => {
                let service = SerializationService::new();
                match service.load_world(&scene, world.inner_world_mut()) {
                    Ok(report) => {
                        for entry in &report.entries {
                            log::warn!("default scene '{}': {entry}", cfg.default_scene);
                        }
                        log::info!(
                            "khora-sdk run_default: loaded scene '{}' ({} bytes)",
                            cfg.default_scene,
                            bytes.len()
                        );
                    }
                    Err(e) => log::error!("Failed to load default scene: {e}"),
                }
            }
            Err(e) => log::error!(
                "DefaultRuntimeApp: invalid scene file '{}': {:?}",
                cfg.default_scene,
                e
            ),
        }
    }

    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(600) {
            log::info!("khora-sdk run_default: frame {}", self.frame_count);
        }
    }
}

impl AgentProvider for DefaultRuntimeApp {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
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

    log::info!(
        "khora-sdk run_default: project='{}' from {} (preset={}, verify_integrity={})",
        cfg.project_name,
        exe_dir.display(),
        cfg.preset.as_deref().unwrap_or("unset"),
        cfg.verify_integrity,
    );

    let verify_integrity = cfg.verify_integrity;
    run_winit::<WinitWindowProvider, DefaultRuntimeApp>(move |window, runtime, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
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

        // Physics — Rapier3D
        let physics: Box<dyn PhysicsProvider> = Box::new(RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));

        // UI layout — Taffy
        let layout: Box<dyn LayoutSystem> = Box::new(TaffyLayoutSystem::new());
        runtime.backends.insert(Arc::new(Mutex::new(layout)));

        // Text renderer — StandardTextRenderer
        let text: Arc<dyn TextRenderer> = Arc::new(StandardTextRenderer::new(TEXT_WGSL.to_owned()));
        runtime.backends.insert(text);

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

        let metrics = Arc::new(MetricsRegistry::new());
        match build_asset_service(&exe_dir, metrics, verify_integrity) {
            Ok(svc) => {
                runtime.services.insert(Arc::new(Mutex::new(svc)));
            }
            Err(e) => {
                log::error!("khora-sdk run_default: AssetService init failed: {:#}", e);
            }
        }

        // Hot-reload for both `.wgsl` and `.erg`, rooted beside the executable.
        // One watcher over the whole tree is one OS handle, and the two data
        // systems read the same stream from their own cursors. With no such
        // directory — a packed runtime — nothing is watched and the backends
        // serve their embedded sources.
        //
        // The queues themselves are inserted by the engine bootstrap for every
        // application; only the root is this launcher's business.
        crate::scripts::mount(runtime, &exe_dir.join("assets"));
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::khora_core::renderer::api::gpu_scene::Mesh;
    use std::path::PathBuf;

    /// The `runtime.json` the editor's "Build Game" writes for a project named
    /// "Khora Demo" with the Release preset. The editor's own test compares
    /// what it writes against the same file, so the writer and this reader are
    /// held to one schema.
    const WRITTEN_BY_THE_EDITOR: &str = include_str!("../tests/fixtures/runtime.json");

    /// One file per asset type the index builder knows. The bytes are not a
    /// valid asset of any kind: the probe only asks whether a decoder is
    /// registered for the type, not whether it accepts the file.
    const PROBES: &[&str] = &[
        "probe.obj",
        "probe.png",
        "probe.wav",
        "probe.wgsl",
        "probe.ttf",
        "probe.kscene",
        "probe.kmat",
        "probe.erg",
        "probe.kprefab",
    ];

    /// The asset types a loose-files project can decode. The editor's
    /// `ProjectVfs` test pins the same list.
    const DECODABLE: &[&str] = &[
        "audio", "font", "material", "mesh", "script", "shader", "texture",
    ];

    fn fixture_field(name: &str) -> serde_json::Value {
        let value: serde_json::Value =
            serde_json::from_str(WRITTEN_BY_THE_EDITOR).expect("the fixture is valid JSON");
        value[name].clone()
    }

    /// A fresh scratch directory for one test, unique across the tests of this
    /// process.
    fn scratch_dir(tag: &str) -> std::io::Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("khora-sdk-{tag}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// The asset types among [`PROBES`] that `svc` has a decoder for, sorted.
    fn decodable_types(svc: &mut AssetService) -> Result<Vec<String>> {
        let mut decodable = Vec::new();
        for file in PROBES {
            let uuid = AssetUUID::new_v5(file);
            let type_name = svc
                .index()
                .get_metadata(&uuid)
                .ok_or_else(|| anyhow!("{file} is not indexed"))?
                .asset_type_name
                .clone();
            let no_decoder = match svc.load::<Mesh>(&uuid) {
                Ok(_) => false,
                Err(e) => format!("{e:#}").contains("No decoder registered"),
            };
            if !no_decoder {
                decodable.push(type_name);
            }
        }
        decodable.sort();
        Ok(decodable)
    }

    #[test]
    fn runtime_json_written_by_the_editor_reads_back_field_for_field() {
        let cfg: RuntimeConfig =
            serde_json::from_str(WRITTEN_BY_THE_EDITOR).expect("the runtime reads it");

        assert_eq!(cfg.project_name, "Khora Demo");
        assert_eq!(cfg.default_scene, "scenes/default.kscene");
        assert_eq!(cfg.window_title.as_deref(), Some("Khora Demo"));
        assert_eq!(cfg.window_title(), "Khora Demo");
        assert_eq!(cfg.preset.as_deref(), Some("release"));
        assert!(cfg.verify_integrity);
    }

    /// A `runtime.json` written before the window title existed still boots,
    /// and the window is titled after the project.
    #[test]
    fn runtime_json_without_a_window_title_titles_the_window_after_the_project() {
        let cfg: RuntimeConfig = serde_json::from_str(
            r#"{ "project_name": "Old Build", "default_scene": "scenes/a.kscene",
                 "preset": "debug", "verify_integrity": false }"#,
        )
        .expect("an older runtime.json still reads");

        assert_eq!(cfg.window_title, None);
        assert_eq!(cfg.window_title(), "Old Build");
    }

    /// The scene the runtime loads when `runtime.json` does not name one is the
    /// scene the editor saves by default.
    #[test]
    fn default_scene_path_is_the_one_the_editor_writes() {
        let from_empty: RuntimeConfig =
            serde_json::from_str("{}").expect("every field has a default");
        let written = fixture_field("default_scene");

        assert_eq!(
            from_empty.default_scene,
            written.as_str().unwrap_or_default()
        );
        assert_eq!(
            RuntimeConfig::defaults().default_scene,
            written.as_str().unwrap_or_default()
        );
    }

    #[test]
    fn a_loose_files_runtime_registers_every_decoder() -> Result<()> {
        let dir = scratch_dir("loose-decoders")?;
        let assets = dir.join("assets");
        std::fs::create_dir_all(&assets)?;
        for file in PROBES {
            std::fs::write(assets.join(file), b"not a real asset")?;
        }

        let mut svc = build_asset_service(&dir, Arc::new(MetricsRegistry::new()), false)?;
        let decodable = decodable_types(&mut svc);
        drop(svc);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(decodable?, DECODABLE);
        Ok(())
    }

    /// The runtime reads the asset-identity registry the editor writes: a scene
    /// renamed in the editor keeps the UUID every reference to it stored, so a
    /// loose-files runtime finds it under that UUID, not under the one its new
    /// path would derive. The registry sits in `.khora/` beside `assets/`, as in
    /// a project.
    #[test]
    fn a_loose_files_runtime_resolves_the_uuids_the_registry_froze() -> Result<()> {
        use crate::khora_core::asset::AssetUUID;

        let dir = scratch_dir("loose-registry")?;
        let scenes = dir.join("assets").join("scenes");
        std::fs::create_dir_all(&scenes)?;
        std::fs::write(scenes.join("renamed.kscene"), b"scene bytes")?;

        let frozen = AssetUUID::new_v5("scenes/original.kscene");
        let mut registry = crate::AssetIdRegistry::load(&dir);
        registry.freeze("scenes/renamed.kscene", frozen);
        registry.save()?;

        let resolved =
            build_asset_service(&dir, Arc::new(MetricsRegistry::new()), false).map(|mut svc| {
                let by_frozen = svc.load_raw(&frozen).ok();
                let by_path = svc
                    .index()
                    .get_metadata(&AssetUUID::new_v5("scenes/renamed.kscene"))
                    .is_some();
                (by_frozen, by_path)
            });
        let _ = std::fs::remove_dir_all(&dir);
        let (by_frozen, by_path) = resolved?;

        assert_eq!(
            by_frozen.as_deref(),
            Some(&b"scene bytes"[..]),
            "the frozen UUID does not resolve to the renamed scene"
        );
        assert!(
            !by_path,
            "the renamed scene is indexed under its path-derived UUID"
        );
        Ok(())
    }

    /// The start-up error a user sees when nothing is next to the binary reads
    /// as one sentence: the source line break is a `\` continuation, not a run
    /// of spaces inside the message.
    #[test]
    fn the_nothing_to_load_error_has_no_run_of_spaces() -> Result<()> {
        let dir = scratch_dir("nothing-to-load")?;
        let err = build_asset_service(&dir, Arc::new(MetricsRegistry::new()), false)
            .err()
            .map(|e| format!("{e:#}"));
        let _ = std::fs::remove_dir_all(&dir);

        let message = err.expect("an empty directory cannot start");
        assert!(
            !message.contains("  "),
            "the message carries the source indentation: {message:?}"
        );
        assert!(
            message.contains("and no `assets/` next to the binary"),
            "{message:?}"
        );
        Ok(())
    }

    /// A packed runtime registers the same decoders as a loose-files one, and
    /// with `verify_integrity` it checks every asset against `manifest.bin`;
    /// without it, it does not.
    #[test]
    fn a_packed_runtime_registers_every_decoder_and_honours_the_manifest() -> Result<()> {
        let dir = scratch_dir("packed")?;
        let assets = dir.join("project").join("assets");
        let exe_dir = dir.join("exe");
        std::fs::create_dir_all(&assets)?;
        for file in PROBES {
            std::fs::write(assets.join(file), b"not a real asset")?;
        }
        crate::PackBuilder::new(&assets, &exe_dir)
            .with_manifest(true)
            .build()?;

        let outcome = (|| -> Result<_> {
            let mut svc = build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), true)?;
            let decodable = decodable_types(&mut svc)?;

            // A manifest that no longer matches the pack: every asset is
            // refused when verifying, and served when not.
            let probe = AssetUUID::new_v5("probe.kscene");
            let mut forged = PackManifest::new();
            forged.insert(probe, b"other bytes");
            std::fs::write(exe_dir.join("manifest.bin"), forged.encode()?)?;
            let verified = build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), true)?
                .load_raw(&probe)
                .is_ok();
            let unverified =
                build_asset_service(&exe_dir, Arc::new(MetricsRegistry::new()), false)?
                    .load_raw(&probe)
                    .is_ok();
            Ok((decodable, verified, unverified))
        })();
        let _ = std::fs::remove_dir_all(&dir);
        let (decodable, verified, unverified) = outcome?;

        assert_eq!(decodable, DECODABLE);
        assert!(!verified, "a forged manifest did not stop the load");
        assert!(
            unverified,
            "the manifest was applied without verify_integrity"
        );
        Ok(())
    }
}
