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

//! How the default runtime opens its game off the frame: loose assets indexed
//! and their scripts compiled, the default scene loaded, each filed by ticket.

use khora_core::sync::LockExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::game_assets::OpenedGame;
use crate::khora_core::asset::AssetUUID;
use crate::khora_core::scene::LoadTicket;
use crate::{
    AssetReader, AssetService, GameWorld, IndexHub, IndexTicket, Readiness, ReadinessHold, Runtime,
    SceneLoadHub,
};
use khora_data::scene::loading::{LoadKind, SceneFetch, SceneSource};

/// Where the binary that runs the game sits, set once by `run_default`.
pub(super) static GAME_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The game being opened: what was filed, until it lands.
pub(super) struct GameOpening {
    /// The scene the game starts in, by its path under `assets/`.
    scene: String,
    assets: Arc<Mutex<AssetService>>,
    loads: Arc<SceneLoadHub>,
    indexes: Option<Arc<IndexHub>>,
    readiness: Option<Arc<Readiness>>,
    /// Keeps the startup waiting until the scene is filed: it is filed only
    /// once the index landed.
    hold: Option<ReadinessHold>,
    index: Option<IndexTicket>,
    load: Option<LoadTicket>,
}

impl GameOpening {
    /// Starts opening the game the boot filed: its scene filed, or its index awaited.
    ///
    /// A packed game's scene is filed at once; loose `assets/` are indexed
    /// first. Returns `None`, logged, when the engine holds no asset service
    /// or no scene hub.
    pub(super) fn start(
        exe_dir: &Path,
        scene: String,
        world: &mut GameWorld,
        runtime: &Runtime,
    ) -> Option<Self> {
        let Some(assets) = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned() else {
            log::error!("khora-sdk run_default: the engine holds no asset service");
            return None;
        };
        let Some(loads) = runtime.resources.get::<Arc<SceneLoadHub>>().cloned() else {
            log::error!("khora-sdk run_default: the engine holds no scene load hub");
            return None;
        };
        let readiness = runtime.resources.get::<Arc<Readiness>>().cloned();
        let mut opening = Self {
            scene,
            assets,
            loads,
            indexes: runtime.resources.get::<Arc<IndexHub>>().cloned(),
            hold: readiness.as_ref().map(|readiness| readiness.hold()),
            readiness,
            index: None,
            load: None,
        };

        // What `open_game_assets` opened was filed at boot: the index of loose
        // files, the scripts of either. A scene carries the declarations it
        // holds, so it may land before them.
        match runtime.resources.get::<OpenedGame>() {
            Some(opened) if opened.pack_modules.is_some() => opening.file_scene(world),
            Some(OpenedGame {
                index: Some(ticket),
                ..
            }) => opening.index = Some(*ticket),
            _ => {
                log::error!(
                    "khora-sdk run_default: the assets beside {} were not filed to open",
                    exe_dir.display()
                );
                opening.hold = None;
            }
        }
        Some(opening)
    }

    /// Takes in what landed: files the scene once indexed, logs what its load adapted.
    pub(super) fn advance(&mut self, world: &mut GameWorld) {
        if let (Some(ticket), Some(indexes)) = (self.index, &self.indexes) {
            if let Some(outcome) = indexes.take_outcome(ticket) {
                self.index = None;
                match outcome {
                    Ok(()) => self.file_scene(world),
                    Err(why) => {
                        log::error!(
                            "khora-sdk run_default: the game's assets were not indexed: {why}"
                        );
                        self.hold = None;
                    }
                }
            }
        }
        if let Some(ticket) = self.load {
            if let Some(outcome) = self.loads.take_outcome(ticket) {
                self.load = None;
                match outcome {
                    Ok(report) => {
                        for entry in &report.entries {
                            log::warn!("default scene '{}': {entry}", self.scene);
                        }
                        log::info!("khora-sdk run_default: loaded scene '{}'", self.scene);
                    }
                    Err(e) => log::error!(
                        "khora-sdk run_default: the default scene '{}' did not load: {e}",
                        self.scene
                    ),
                }
            }
        }
    }

    /// Files the default scene to be loaded off the frame, read through the asset service.
    ///
    /// Files nothing, logged, when the service holds no such scene. Either way,
    /// the startup then waits on nothing more of this opening.
    fn file_scene(&mut self, world: &mut GameWorld) {
        let uuid = AssetUUID::new_v5(&self.scene);
        let reader = {
            let service = self.assets.lock_recovered();
            service
                .index()
                .get_metadata(&uuid)
                .is_some()
                .then(|| service.reader())
        };
        match reader {
            Some(reader) => {
                // Its prefab instances come from the shipped prefabs.
                let prefabs = Arc::new(khora_io::serialization::AssetPrefabs::new(Arc::clone(
                    &self.assets,
                )));
                let ticket = self.loads.file(
                    world.inner_world_mut(),
                    LoadKind::Replace(SceneSource::Fetch(Arc::new(GameScene { reader, uuid }))),
                    prefabs,
                );
                if let Some(readiness) = &self.readiness {
                    readiness.wait_for_scene(ticket);
                }
                self.load = Some(ticket);
            }
            None => log::warn!(
                "khora-sdk run_default: default scene '{}' not found — starting with an empty world",
                self.scene
            ),
        }
        self.hold = None;
    }
}

/// The game's default scene, its bytes read off the frame through the asset
/// service's reader.
struct GameScene {
    reader: Arc<AssetReader>,
    uuid: AssetUUID,
}

impl SceneFetch for GameScene {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        self.reader
            .read_raw(&self.uuid)
            .map_err(|error| format!("{error:#}"))
    }
}
