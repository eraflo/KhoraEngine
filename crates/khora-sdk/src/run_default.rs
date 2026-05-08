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

use crate::khora_core::asset::AssetUUID;
use crate::khora_core::renderer::api::scene::Mesh;
use crate::winit_adapters::WinitWindowProvider;
use crate::{
    run_winit, AgentProvider, AssetIo, AssetService, DccService, EngineApp, FileLoader,
    FileSystemResolver, GameWorld, IndexBuilder, InputEvent, MeshDispatcher, MetricsRegistry,
    PackLoader, PhaseProvider, RenderSystem, SceneFile, SerializationService, ServiceRegistry,
    SoundData, SymphoniaDecoder, WgpuRenderSystem, WindowConfig,
};
use khora_io::asset::PackManifest;
use serde::Deserialize;

/// Runtime config the launcher (editor's "Build Game") drops next to the
/// binary. Read at startup; sensible defaults are used when the file is
/// missing (typical for engine contributors running the runtime against a
/// loose `assets/` directory).
static RUNTIME_CONFIG: OnceLock<RuntimeConfig> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
struct RuntimeConfig {
    #[serde(default = "default_project_name")]
    project_name: String,
    #[serde(default = "default_scene_rel_path")]
    default_scene: String,
    #[serde(default)]
    window_title: Option<String>,
    /// Build preset label written by the editor (debug/release/shipping).
    /// Optional for older runtime.json files. Used for diagnostics.
    #[serde(default)]
    #[allow(dead_code)]
    preset: Option<String>,
    /// When `true`, the runtime hashes each loaded asset against
    /// `manifest.bin` and aborts on mismatch. Defaults to `false` so
    /// older packs (without a manifest) keep booting.
    #[serde(default)]
    verify_integrity: bool,
}

fn default_project_name() -> String {
    "Khora Runtime".to_owned()
}
fn default_scene_rel_path() -> String {
    "scenes/default.kscene".to_owned()
}

impl RuntimeConfig {
    fn load_or_default(exe_dir: &Path) -> Self {
        let path = exe_dir.join("runtime.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<RuntimeConfig>(&text) {
                Ok(cfg) => {
                    log::info!(
                        "khora-sdk run_default: loaded {} (project='{}', scene='{}')",
                        path.display(),
                        cfg.project_name,
                        cfg.default_scene
                    );
                    cfg
                }
                Err(e) => {
                    log::warn!(
                        "khora-sdk run_default: malformed runtime.json ({}): {} — \
                         falling back to defaults",
                        path.display(),
                        e
                    );
                    Self::defaults()
                }
            },
            Err(_) => {
                log::info!(
                    "khora-sdk run_default: no runtime.json at {} — running in dev \
                     mode with defaults",
                    path.display()
                );
                Self::defaults()
            }
        }
    }

    fn defaults() -> Self {
        Self {
            project_name: default_project_name(),
            default_scene: default_scene_rel_path(),
            window_title: None,
            preset: None,
            verify_integrity: false,
        }
    }

    fn window_title(&self) -> String {
        self.window_title
            .clone()
            .unwrap_or_else(|| self.project_name.clone())
    }
}

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

    let (index_bytes, io, mode_label, gltf_root, is_pack): (
        _,
        Box<dyn AssetIo>,
        &str,
        PathBuf,
        bool,
    ) = if pack.is_file() && idx.is_file() {
        let bytes =
            std::fs::read(&idx).with_context(|| format!("Failed to read {}", idx.display()))?;
        let pack_file = std::fs::File::open(&pack)
            .with_context(|| format!("Failed to open {}", pack.display()))?;
        let loader = PackLoader::new(pack_file)
            .context("Pack header validation failed — refusing to start")?;
        (
            bytes,
            Box::new(loader) as Box<dyn AssetIo>,
            "PackLoader",
            exe_dir.to_path_buf(),
            true,
        )
    } else if assets.is_dir() {
        let bytes = IndexBuilder::new(&assets)
            .build_index_bytes()
            .context("Failed to build dev-mode in-memory index")?;
        (
            bytes,
            Box::new(FileLoader::new(&assets)),
            "FileLoader",
            assets.clone(),
            false,
        )
    } else {
        return Err(anyhow!(
            "khora-sdk run_default cannot start: no `data.pack`+`index.bin` and no \
             `assets/` next to the binary at {}",
            exe_dir.display()
        ));
    };

    // Manifest is a release-mode artifact emitted alongside `data.pack`.
    // We only consult it when both the runtime explicitly asked for
    // verification and we're booting against a real pack. In dev mode the
    // flag is meaningless (bytes come straight off disk) — note it once
    // and move on.
    let manifest = if verify_integrity {
        if is_pack {
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
        } else {
            log::info!(
                "khora-sdk run_default: verify_integrity=true ignored in dev mode \
                 (no pack to verify against)"
            );
            None
        }
    } else {
        None
    };

    let mut svc = AssetService::new(&index_bytes, io, metrics, manifest)?;
    svc.register_inventory_decoders();
    svc.register_decoder::<SoundData>("audio", SymphoniaDecoder);
    let gltf_resolver = Arc::new(FileSystemResolver::new(&gltf_root));
    svc.register_decoder::<Mesh>("mesh", MeshDispatcher::new(gltf_resolver));

    log::info!(
        "khora-sdk run_default: using {} ({} assets indexed)",
        mode_label,
        svc.vfs().asset_count()
    );

    Ok(svc)
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

    fn setup(&mut self, world: &mut GameWorld, services: &ServiceRegistry) {
        let svc = services.get::<Arc<Mutex<AssetService>>>().cloned();
        let cfg = RUNTIME_CONFIG
            .get()
            .cloned()
            .unwrap_or_else(RuntimeConfig::defaults);

        let Some(svc) = svc else {
            log::error!("DefaultRuntimeApp: AssetService missing from ServiceRegistry");
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
                let serializer = SerializationService::new();
                if let Err(e) = serializer.load_world(&scene, world.inner_world_mut()) {
                    log::error!("Failed to load default scene: {:?}", e);
                } else {
                    log::info!(
                        "khora-sdk run_default: loaded scene '{}' ({} bytes)",
                        cfg.default_scene,
                        bytes.len()
                    );
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
    fn register_agents(&self, _dcc: &DccService, _services: &mut ServiceRegistry) {}
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
    run_winit::<WinitWindowProvider, DefaultRuntimeApp>(move |window, services, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        services.insert(rs.graphics_device());
        let rs_dyn: Box<dyn RenderSystem> = Box::new(rs);
        services.insert(Arc::new(Mutex::new(rs_dyn)));

        let metrics = Arc::new(MetricsRegistry::new());
        match build_asset_service(&exe_dir, metrics, verify_integrity) {
            Ok(svc) => {
                services.insert(Arc::new(Mutex::new(svc)));
            }
            Err(e) => {
                log::error!("khora-sdk run_default: AssetService init failed: {:#}", e);
            }
        }
    })?;
    Ok(())
}
