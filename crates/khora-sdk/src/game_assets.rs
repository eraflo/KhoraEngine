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

//! A game's assets as its binary finds them: a pack sealed by the key the
//! binary carries, or loose files when it carries none.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{IndexKind, IndexTicket};
use khora_core::Runtime;
use khora_io::asset::pack_key::stamped_pack_key;
use khora_io::asset::signing::{PackKey, PackSealError, SignedManifest};
use khora_io::asset::{AssetService, IndexHub, PackLoader, PackManifest};
use khora_io::profiles::ProfilePlace;
use khora_io::script::project_compiler::{ProjectCompiler, ScriptCompiler};
use khora_io::script::sources::pack_modules;
use khora_io::trust::Refusals;
use khora_telemetry::MetricsRegistry;

use crate::engine::Readiness;
use crate::scripts::CompileTicket;
use crate::traits::EngineApp;

/// The game [`open_game_assets`] opened, and what the boot filed for it.
#[derive(Debug, Clone)]
pub(crate) struct OpenedGame {
    /// The folder of the game's binary.
    pub(crate) exe_dir: PathBuf,
    /// A packed game's modules; `None` for loose files.
    pub(crate) pack_modules: Option<BTreeSet<String>>,
    /// The index of loose files, once filed.
    pub(crate) index: Option<IndexTicket>,
    /// The compile of the game's scripts, once filed; `None` when it has none.
    pub(crate) compile: Option<CompileTicket>,
}

/// Opens the assets of the game `A` beside `exe_dir` into `runtime`.
///
/// A pack sealed by the key the running binary carries; else loose `assets/`,
/// only in a binary that carries no key. Call it from the bootstrap closure.
/// See the book: concepts/trust.md, "Signed packs".
///
/// # Errors
///
/// - When a stamped binary finds no complete pack, or the pack's seal does not
///   verify with its key; when a pack sits beside a binary with no key.
/// - When `index.bin` cannot be read, is over its size limit, or does not
///   count what `data.pack` counts.
/// - When there is neither a pack nor `assets/` beside the binary.
pub fn open_game_assets<A: EngineApp>(runtime: &mut Runtime, exe_dir: &Path) -> Result<()> {
    open_game_assets_with::<A>(runtime, exe_dir, stamped_pack_key())
}

/// Opens the game's assets as [`open_game_assets`], its pack verified with `pack_key`.
pub(crate) fn open_game_assets_with<A: EngineApp>(
    runtime: &mut Runtime,
    exe_dir: &Path,
    pack_key: Option<PackKey>,
) -> Result<()> {
    // A packed game runs on a player's machine: its profile and its
    // quarantine are filed in the player's data folder, and it may ship a
    // profile beside it.
    if is_packed(exe_dir) {
        runtime.resources.insert(ProfilePlace::Player);
        runtime
            .resources
            .insert(crate::engine::profiles::ShippedProfileDir(
                exe_dir.to_path_buf(),
            ));
    }
    crate::engine::refusals::install(runtime, &A::game_identity(), A::keeps_user_files());
    let refusals = runtime.resources.get::<Arc<Refusals>>().cloned();
    let svc = build_asset_service(
        exe_dir,
        Arc::new(MetricsRegistry::new()),
        refusals,
        pack_key,
    )?;
    let mut opened = OpenedGame {
        exe_dir: exe_dir.to_path_buf(),
        pack_modules: None,
        index: None,
        compile: None,
    };
    if is_packed(exe_dir) {
        // A sealed game's scripts are the pack's: nothing on the disk beside it is read.
        let compiler: Arc<dyn ScriptCompiler> = Arc::new(ProjectCompiler::for_pack(
            runtime,
            pack_root(exe_dir),
            svc.reader(),
        ));
        runtime.resources.insert(compiler);
        opened.pack_modules = Some(pack_modules(svc.index()).into_keys().collect());
    }
    runtime.services.insert(Arc::new(Mutex::new(svc)));
    runtime.resources.insert(opened);
    Ok(())
}

/// Files what the game `runtime` opened needs off the frame, and the startup waits on it.
///
/// A pack's modules are compiled; loose files are indexed, and their scripts
/// mounted. Call once the hubs and the run's refusals are installed. Does
/// nothing when no game was opened, or once filed.
pub(crate) fn file_opened_game(runtime: &mut Runtime) {
    let Some(mut opened) = runtime.resources.remove::<OpenedGame>() else {
        return;
    };
    if opened.index.is_some() || opened.compile.is_some() {
        runtime.resources.insert(opened);
        return;
    }
    let readiness = runtime.resources.get::<Arc<Readiness>>().cloned();
    match opened.pack_modules.clone() {
        Some(modules) => {
            opened.compile =
                crate::scripts::mount_pack(runtime, &pack_root(&opened.exe_dir), modules);
        }
        None => {
            // Read from the disk whatever the index holds.
            opened.compile = crate::scripts::mount(runtime, &opened.exe_dir.join("assets"));
            match runtime.resources.get::<Arc<IndexHub>>() {
                Some(indexes) => {
                    opened.index = Some(indexes.file(opened.exe_dir.clone(), IndexKind::Open));
                    log::info!(
                        "khora-sdk: indexing {} off the frame",
                        opened.exe_dir.join("assets").display()
                    );
                }
                None => log::error!("khora-sdk: the engine holds no index hub"),
            }
        }
    }
    if let Some(readiness) = &readiness {
        if let Some(ticket) = opened.compile {
            readiness.wait_for_compile(ticket);
        }
        if let Some(ticket) = opened.index {
            readiness.wait_for_index(ticket);
        }
    }
    runtime.resources.insert(opened);
}

/// Builds the [`AssetService`] the game starts with: a pack's, or an empty one for loose `assets/`.
///
/// Loose assets are indexed off the frame once the engine runs, unverified. A
/// pack opens only if its seal verifies with `pack_key` ([`read_manifest`]);
/// each load is then hashed against the manifest. See the book:
/// concepts/trust.md, "Signed packs".
pub(crate) fn build_asset_service(
    exe_dir: &Path,
    metrics: Arc<MetricsRegistry>,
    refusals: Option<Arc<Refusals>>,
    pack_key: Option<PackKey>,
) -> Result<AssetService> {
    let pack = exe_dir.join("data.pack");
    let idx = exe_dir.join("index.bin");
    let assets = exe_dir.join("assets");

    let (svc, mode_label) = if is_packed(exe_dir) {
        let bytes = read_index(&idx)?;
        let pack_file = std::fs::File::open(&pack)
            .with_context(|| format!("Failed to open {}", pack.display()))?;
        let loader = PackLoader::new(pack_file)
            .context("Pack header validation failed — refusing to start")?;
        let manifest = read_manifest(exe_dir, &bytes, pack_key)?;
        let counted = loader.header().asset_count;
        let mut svc = AssetService::new(&bytes, Box::new(loader), metrics, Some(manifest))?;
        // A pack and an index from two builds do not go together.
        let indexed = svc.index().entries_read();
        if counted as usize != indexed {
            return Err(anyhow!(
                "{} counts {counted} entries but {} holds {indexed}: the pair does not \
                 come from one build",
                pack.display(),
                idx.display()
            ));
        }
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
            "khora-sdk cannot open the game: no `data.pack`+`index.bin` and no \
             `assets/` next to the binary at {}",
            exe_dir.display()
        ));
    };

    log::info!(
        "khora-sdk: the game's assets use {} ({} assets indexed)",
        mode_label,
        svc.index().asset_count()
    );

    Ok(svc)
}

/// Refuses the pack beside `exe_dir` when its seal does not verify with `pack_key`.
///
/// A loose game has nothing to verify, unless `pack_key` says it was built.
pub(crate) fn check_pack_seal(exe_dir: &Path, pack_key: Option<PackKey>) -> Result<()> {
    if !is_packed(exe_dir) {
        return refuse_loose_when_stamped(exe_dir, pack_key);
    }
    let bytes = read_index(&exe_dir.join("index.bin"))?;
    read_manifest(exe_dir, &bytes, pack_key).map(|_| ())
}

/// Whether the game beside `exe_dir` is packed: `data.pack` and its
/// precomputed `index.bin` sit beside the binary.
pub(crate) fn is_packed(exe_dir: &Path) -> bool {
    exe_dir.join("data.pack").is_file() && exe_dir.join("index.bin").is_file()
}

/// Returns the root a packed game's scripts are filed and mounted under: its `data.pack`.
pub(crate) fn pack_root(exe_dir: &Path) -> PathBuf {
    exe_dir.join("data.pack")
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
                "khora-sdk: the pack's seal verifies ({} entries)",
                manifest.len()
            );
            Ok(manifest)
        }
        Err(error) => {
            log::error!("khora-sdk: the pack is refused: {error}");
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
        "khora-sdk: this built game finds no complete pack beside it at {}",
        exe_dir.display()
    );
    Err(anyhow!(
        "a built game runs from its pack: `data.pack` and `index.bin` are not both beside {} — \
         build the game again",
        exe_dir.display()
    ))
}

/// Returns the bytes of the pack index at `idx`, read whole: one past its limit is refused.
fn read_index(idx: &Path) -> Result<Vec<u8>> {
    let cap = khora_io::asset::MAX_INDEX_BYTES as u64;
    khora_io::fs::read_bounded(idx, cap).map_err(|error| match error {
        khora_io::fs::BoundedReadError::TooLarge { size, .. } => anyhow!(
            "{} is {size} bytes, over the {cap} an index may hold",
            idx.display()
        ),
        khora_io::fs::BoundedReadError::Io(error) => {
            anyhow::Error::new(error).context(format!("Failed to read {}", idx.display()))
        }
    })
}

#[cfg(test)]
mod tests {
    use khora_core::asset::AssetUUID;
    use khora_core::sync::LockExt;

    use super::*;
    use crate::game_seal::GameSeal;
    use crate::run_default::tests::{
        frames_until, profiles_scratch, scene_bytes, served, GameDir, COUNTER, DEFAULT_SCENE,
    };
    use crate::{EngineCore, GameWorld, InputEvent, PhaseProvider, WindowConfig};

    /// A game of its own `main`: its bootstrap closure opens the game's assets
    /// with `open_game_assets`, and it does nothing else.
    struct OwnMain;

    impl PhaseProvider for OwnMain {}
    impl EngineApp for OwnMain {
        fn window_config() -> WindowConfig {
            WindowConfig::default()
        }
        fn new() -> Self {
            OwnMain
        }
        fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
        fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
    }

    /// Boots [`OwnMain`] beside `exe_dir`, its closure opening the game's assets
    /// as `open_game_assets` opens them in a binary stamped with `key`.
    fn own_main_engine(exe_dir: &Path, key: Option<PackKey>) -> EngineCore<OwnMain> {
        std::env::set_var(khora_io::profiles::PROFILES_VAR, profiles_scratch());
        let mut runtime = crate::winit_adapters::engine_runtime();
        open_game_assets_with::<OwnMain>(&mut runtime, exe_dir, key)
            .expect("the game's assets open");
        let mut engine = EngineCore::<OwnMain>::new();
        engine.bootstrap_sealed(OwnMain, runtime, &GameSeal::new(exe_dir, key));
        engine
    }

    /// **A game opening its own pack compiles the pack's scripts.**
    ///
    /// A game whose closure calls `open_game_assets` over a pack holding
    /// `scripts/counter.erg` has the counter's behaviour declared within
    /// frames, as `run_default` has.
    #[test]
    fn a_game_opening_its_own_pack_compiles_the_packs_scripts() {
        let game = GameDir::new("own-main-pack-scripts");
        let exe_dir = game.packed(&[("scripts/counter.erg", COUNTER.as_bytes())]);
        std::fs::remove_dir_all(game.path().join("project")).expect("the project is taken away");

        let mut engine = own_main_engine(&exe_dir, Some(game.pack_key()));

        frames_until(
            &mut engine,
            "the pack's scripts were not compiled",
            |engine| {
                engine.game_world_mut().is_some_and(|world| {
                    world
                        .inner_world()
                        .components()
                        .key_named("Counter")
                        .is_some()
                })
            },
        );
    }

    /// **A game opening its own loose assets has them indexed.**
    ///
    /// A game whose closure calls `open_game_assets`, in a binary that carries
    /// no key, over a folder holding a scene finds that scene in its asset
    /// service within frames.
    #[test]
    fn a_game_opening_its_own_loose_assets_has_them_indexed() {
        let game = GameDir::new("own-main-loose");
        game.asset(DEFAULT_SCENE, &scene_bytes(&["Loose"]));

        let mut engine = own_main_engine(game.path(), None);

        frames_until(&mut engine, "the loose assets were not indexed", |engine| {
            served(engine)
                .lock_recovered()
                .index()
                .get_metadata(&AssetUUID::new_v5(DEFAULT_SCENE))
                .is_some()
        });
    }
}
