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

//! The scene serialization service: scenes and game saves written in the
//! encoding a [`SerializationGoal`] calls for, and loaded atomically.
//!
//! See the book: concepts/serialization.md, "One service, one file format".

use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_core::ecs::ReservationHold;
use khora_core::scene::{SceneFile, SerializationGoal, SCENE_FORMAT_VERSION};
use khora_data::ecs::World;
use khora_data::scene::record::{LoadReport, ReportEntry};
use khora_data::scene::save::prepare_game_with;
use khora_data::scene::snapshot::{
    prepare_snapshot, prepare_snapshot_with, write_snapshot, SNAPSHOT_ENCODING_ID,
};
use khora_data::scene::{
    about_what_loads, capture_save, capture_world, collapse, compose_reporting, encoding_of,
    expand_reporting, prepare, prepare_game, prepare_with, read_into, read_save_file,
    read_scene_file, write_save_file, write_scene_file, CompactEncoding, Identity, LoadCatalog,
    LoadFailure, MsgPackEncoding, NoPrefabs, PrefabSource, Prepared, SaveError, SceneEncoding,
    SceneFileReadError, SceneRecord, TextEncoding,
};

use crate::asset::{AssetService, PrefabAsset};

pub use super::asset_prefabs::AssetPrefabs;
pub use super::reader_prefabs::ReaderPrefabs;

/// Scene format version produced by today's writers.
pub const CURRENT_SCENE_VERSION: u32 = SCENE_FORMAT_VERSION as u32;

/// The index type of a prefab.
pub(crate) const PREFAB: &str = "prefab";

/// An error that can occur within the `SerializationService`.
#[derive(Debug)]
pub enum SerializationServiceError {
    /// The world could not be written.
    SaveFailed(SaveError),
    /// The file could not be read: an older or newer format, an encoding
    /// this engine does not have, or a payload that does not decode.
    ReadFailed(SceneFileReadError),
    /// The file was read, but loading it was refused; the world is unchanged.
    LoadFailed(LoadFailure),
}

impl std::fmt::Display for SerializationServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SaveFailed(error) => write!(f, "the scene could not be saved: {error}"),
            Self::ReadFailed(error) => write!(f, "the scene file could not be read: {error}"),
            Self::LoadFailed(failure) => write!(f, "the scene was not loaded: {}", failure.message),
        }
    }
}

impl std::error::Error for SerializationServiceError {}

/// The serialization service, built where it is needed: it holds only where prefabs are read from.
///
/// Without a prefab source, prefab instances are saved expanded, keeping the
/// ids that tie them to their instance, and a scene that links to prefabs
/// cannot be loaded.
#[derive(Default)]
pub struct SerializationService {
    prefabs: Option<Arc<dyn PrefabSource + Send + Sync>>,
}

impl std::fmt::Debug for SerializationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerializationService")
            .field("reads_prefabs", &self.prefabs.is_some())
            .finish()
    }
}

impl SerializationService {
    /// Creates the service.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the service, reading the prefabs a scene links to from `source`.
    ///
    /// Prefab instances are saved as links, and loaded expanded from the
    /// prefabs as they are now.
    pub fn with_prefabs(source: Arc<dyn PrefabSource + Send + Sync>) -> Self {
        Self {
            prefabs: Some(source),
        }
    }

    /// Where this service reads prefabs from.
    fn prefabs(&self) -> &dyn PrefabSource {
        match &self.prefabs {
            Some(source) => source.as_ref(),
            None => &NoPrefabs,
        }
    }

    /// Returns the record encoding `goal` calls for; `None` for `FastestLoad`, a snapshot.
    fn encoding_for(goal: SerializationGoal) -> Option<&'static dyn SceneEncoding> {
        match goal {
            SerializationGoal::HumanReadableDebug | SerializationGoal::LongTermStability => {
                Some(&TextEncoding)
            }
            SerializationGoal::SmallestFileSize | SerializationGoal::EditorInterchange => {
                Some(&CompactEncoding)
            }
            SerializationGoal::PortableBinary => Some(&MsgPackEncoding),
            SerializationGoal::FastestLoad => None,
        }
    }

    /// Saves the current state of the `World` based on a high-level goal.
    pub fn save_world(
        &self,
        world: &World,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        let prefabs = self
            .prefabs
            .as_deref()
            .map(|source| source as &dyn PrefabSource);
        Self::save(world, goal, prefabs)
    }

    /// Saves as [`save_world`](Self::save_world), linking to the prefabs `prefabs` reads.
    pub fn save_world_with(
        &self,
        world: &World,
        goal: SerializationGoal,
        prefabs: &dyn PrefabSource,
    ) -> Result<SceneFile, SerializationServiceError> {
        Self::save(world, goal, Some(prefabs))
    }

    fn save(
        world: &World,
        goal: SerializationGoal,
        prefabs: Option<&dyn PrefabSource>,
    ) -> Result<SceneFile, SerializationServiceError> {
        let encoding = match Self::encoding_for(goal) {
            Some(encoding) => encoding,
            None => match write_snapshot(world) {
                Ok(file) => return Ok(file),
                // A component no fingerprint can guard is never snapshotted:
                // the world is saved as a compact record instead — slower to
                // load, read by any build.
                Err(SaveError::Unguarded(component)) => {
                    log::warn!(
                        "`{component}` cannot be snapshotted; saved as a compact record instead"
                    );
                    &CompactEncoding
                }
                Err(error) => return Err(SerializationServiceError::SaveFailed(error)),
            },
        };
        let mut record = capture_world(world).map_err(SerializationServiceError::SaveFailed)?;
        if let Some(prefabs) = prefabs {
            // A prefab that cannot be read cannot be linked to: the scene is
            // saved expanded rather than not at all, and links again once the
            // prefab is back.
            match collapse(&record, prefabs) {
                Ok(collapsed) => record = collapsed,
                Err(error) => log::warn!("prefab instances saved expanded: {error}"),
            }
        }
        write_scene_file(&record, encoding)
            .map_err(|error| SerializationServiceError::SaveFailed(SaveError::Encoding(error.0)))
    }

    /// Reads and stages the scene `file` holds, in the encoding its header names, adding nothing.
    fn prepare_file(
        file: &SceneFile,
        world: &mut World,
        prefabs: &dyn PrefabSource,
    ) -> Result<(Prepared, Vec<ReportEntry>), SerializationServiceError> {
        if encoding_of(file) == SNAPSHOT_ENCODING_ID {
            return prepare_snapshot(world, file)
                .map(|prepared| (prepared, Vec::new()))
                .map_err(SerializationServiceError::LoadFailed);
        }
        let record = read_scene_file(file).map_err(SerializationServiceError::ReadFailed)?;
        let (record, mut left_out) = read_into(&world.load_catalog(), record);
        let (record, expansion) = Self::expanded(record, prefabs)?;
        left_out.extend(expansion);
        prepare(world, &record)
            .map(|prepared| (prepared, left_out))
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Prepares as `prepare_file` without the world, on any thread.
    ///
    /// What it reads of the world is `catalog`; the ids it reserves are held by
    /// `hold` until the load is committed, abandoned or dropped.
    pub fn prepare_file_with(
        file: Arc<SceneFile>,
        catalog: &LoadCatalog,
        hold: ReservationHold,
        prefabs: &dyn PrefabSource,
    ) -> Result<(Prepared, Vec<ReportEntry>), SerializationServiceError> {
        if encoding_of(&file) == SNAPSHOT_ENCODING_ID {
            return prepare_snapshot_with(catalog, hold, file)
                .map(|prepared| (prepared, Vec::new()))
                .map_err(SerializationServiceError::LoadFailed);
        }
        let record = read_scene_file(&file).map_err(SerializationServiceError::ReadFailed)?;
        let (record, mut left_out) = read_into(catalog, record);
        let (record, expansion) = Self::expanded(record, prefabs)?;
        left_out.extend(expansion);
        prepare_with(catalog, hold, Arc::new(record))
            .map(|prepared| (prepared, left_out))
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Prepares what [`load_game`](Self::load_game) loads without the world, on any thread.
    ///
    /// Read against `catalog`, its ids held by `hold`; returns what reading,
    /// expanding and merging adapted too.
    pub fn prepare_game_with(
        save_file: &SceneFile,
        base_file: &SceneFile,
        catalog: &LoadCatalog,
        hold: ReservationHold,
        prefabs: &dyn PrefabSource,
    ) -> Result<(Prepared, Vec<ReportEntry>), SerializationServiceError> {
        let save = read_save_file(save_file).map_err(SerializationServiceError::ReadFailed)?;
        let base = read_scene_file(base_file).map_err(SerializationServiceError::ReadFailed)?;
        let (base, expanding) = Self::expanded(base, prefabs)?;
        let (record, merging) =
            compose_reporting(&base, &save).map_err(SerializationServiceError::LoadFailed)?;
        let left_out = about_what_loads(&record, expanding.into_iter().chain(merging));
        prepare_game_with(catalog, hold, &record)
            .map(|prepared| (prepared, left_out))
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Returns `record` with its prefab instances expanded, and what their merges left out.
    fn expanded(
        record: SceneRecord,
        prefabs: &dyn PrefabSource,
    ) -> Result<(SceneRecord, Vec<ReportEntry>), SerializationServiceError> {
        if record.instances.is_empty() {
            return Ok((record, Vec::new()));
        }
        expand_reporting(&record, prefabs).map_err(SerializationServiceError::LoadFailed)
    }

    /// Brings the scene `file` holds into `world`, beside what is already there.
    ///
    /// # Errors
    ///
    /// When it cannot be loaded; `world` is then exactly as it was.
    pub fn load_world(
        &self,
        file: &SceneFile,
        world: &mut World,
    ) -> Result<LoadReport, SerializationServiceError> {
        self.load_world_with(file, world, self.prefabs())
    }

    /// Loads as [`load_world`](Self::load_world), expanding the prefabs `prefabs` reads.
    pub fn load_world_with(
        &self,
        file: &SceneFile,
        world: &mut World,
        prefabs: &dyn PrefabSource,
    ) -> Result<LoadReport, SerializationServiceError> {
        let (prepared, left_out) = Self::prepare_file(file, world, prefabs)?;
        let mut report = prepared.commit(world, Identity::Keep).report;
        report.entries.extend(left_out);
        Ok(report)
    }

    /// Replaces everything in `world` with the scene `file` holds.
    ///
    /// The old entities are gone before the new ones arrive, so every saved
    /// identity is restored as saved.
    ///
    /// # Errors
    ///
    /// When it cannot be loaded; `world` is then exactly as it was.
    pub fn replace_world(
        &self,
        file: &SceneFile,
        world: &mut World,
    ) -> Result<LoadReport, SerializationServiceError> {
        self.replace_world_with(file, world, self.prefabs())
    }

    /// Replaces as [`replace_world`](Self::replace_world), expanding the prefabs `prefabs` reads.
    pub fn replace_world_with(
        &self,
        file: &SceneFile,
        world: &mut World,
        prefabs: &dyn PrefabSource,
    ) -> Result<LoadReport, SerializationServiceError> {
        let (prepared, left_out) = Self::prepare_file(file, world, prefabs)?;
        let mut report = Self::replace_with(prepared, world);
        report.entries.extend(left_out);
        Ok(report)
    }

    /// Empties `world`, then commits `prepared` into it, keeping every
    /// recorded identity.
    pub(crate) fn replace_with(prepared: Prepared, world: &mut World) -> LoadReport {
        let old: Vec<_> = world.iter_entities().collect();
        for entity in old {
            world.despawn(entity);
        }
        prepared.commit(world, Identity::Keep).report
    }

    /// Saves the game `world` is running, as its differences from the scene `base_file` holds.
    ///
    /// `base_id` names that scene. A `FastestLoad` goal is written compactly:
    /// a snapshot refuses the engine-written state a save keeps.
    pub fn save_game(
        &self,
        world: &World,
        base_id: AssetUUID,
        base_file: &SceneFile,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        let base = read_scene_file(base_file).map_err(SerializationServiceError::ReadFailed)?;
        // Against the scene as a world: its prefab instances expanded.
        let (base, _) = Self::expanded(base, self.prefabs())?;
        let save =
            capture_save(world, base_id, &base).map_err(SerializationServiceError::SaveFailed)?;
        let encoding = Self::encoding_for(goal).unwrap_or(&CompactEncoding);
        write_save_file(&save, encoding)
            .map_err(|error| SerializationServiceError::SaveFailed(SaveError::Encoding(error.0)))
    }

    /// Replaces everything in `world` with the game `save_file` holds, over the scene `base_file`.
    ///
    /// See the book: concepts/saves.md, "Loading a save: a three-way merge".
    ///
    /// # Errors
    ///
    /// When it cannot be loaded; `world` is then exactly as it was.
    pub fn load_game(
        &self,
        world: &mut World,
        save_file: &SceneFile,
        base_file: &SceneFile,
    ) -> Result<LoadReport, SerializationServiceError> {
        let save = read_save_file(save_file).map_err(SerializationServiceError::ReadFailed)?;
        let base = read_scene_file(base_file).map_err(SerializationServiceError::ReadFailed)?;
        let (base, expanding) = Self::expanded(base, self.prefabs())?;
        let (record, merging) =
            compose_reporting(&base, &save).map_err(SerializationServiceError::LoadFailed)?;
        // Expanding the scene and merging the save each report what they read:
        // one place adapted once, and only what the game loads.
        let left_out = about_what_loads(&record, expanding.into_iter().chain(merging));
        let prepared =
            prepare_game(world, &record).map_err(SerializationServiceError::LoadFailed)?;
        let mut report = Self::replace_with(prepared, world);
        report.entries.extend(left_out);
        Ok(report)
    }

    /// Returns the scene a game save was taken against.
    pub fn save_base(&self, save_file: &SceneFile) -> Result<AssetUUID, SerializationServiceError> {
        read_save_file(save_file)
            .map(|save| save.base)
            .map_err(SerializationServiceError::ReadFailed)
    }
}

/// Returns the prefab `id` names in `assets`, decoded once and kept until `assets` invalidates it.
///
/// # Errors
///
/// When `id` names no asset, or one of another type, or a prefab that cannot
/// be read or decoded.
pub fn read_prefab(assets: &mut AssetService, id: AssetUUID) -> Result<SceneRecord, String> {
    match assets.index().get_metadata(&id) {
        Some(metadata) if metadata.asset_type_name == PREFAB => {}
        Some(metadata) => {
            return Err(format!(
                "the asset {id} is a {}, not a prefab",
                metadata.asset_type_name
            ))
        }
        None => return Err(format!("the project holds no asset {id}")),
    }
    let prefab = assets
        .load::<PrefabAsset>(&id)
        .map_err(|error| error.to_string())?;
    Ok(prefab.0.clone())
}

/// Returns the identities of every prefab `assets` indexes.
pub fn prefab_catalog(assets: &AssetService) -> std::collections::BTreeSet<AssetUUID> {
    assets
        .index()
        .iter_all()
        .filter(|metadata| metadata.asset_type_name == PREFAB)
        .map(|metadata| metadata.uuid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The goal picks the encoding; the format version is the one the
    /// engine reads.
    #[test]
    fn each_goal_names_its_encoding() {
        let world = World::new();
        let service = SerializationService::new();
        for (goal, id) in [
            (SerializationGoal::HumanReadableDebug, "KH_TEXT_V2"),
            (SerializationGoal::LongTermStability, "KH_TEXT_V2"),
            (SerializationGoal::EditorInterchange, "KH_COMPACT_V2"),
            (SerializationGoal::SmallestFileSize, "KH_COMPACT_V2"),
            (SerializationGoal::FastestLoad, "KH_SNAPSHOT_V1"),
            (SerializationGoal::PortableBinary, "KH_MSGPACK_V2"),
        ] {
            let file = service
                .save_world(&world, goal)
                .expect("an empty world saves");
            let written = String::from_utf8_lossy(&file.header.encoding_id);
            assert_eq!(written.trim_end_matches('\0'), id, "{goal:?}");
            assert_eq!(file.header.format_version, SCENE_FORMAT_VERSION);
        }
    }
}
