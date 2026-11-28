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

use khora_core::asset::{kinds, AssetUUID};
use khora_core::ecs::ReservationHold;
use khora_core::scene::{SceneFile, SerializationGoal, SCENE_FORMAT_VERSION};
use khora_data::ecs::World;
use khora_data::scene::record::{LoadReport, ReportEntry};
use khora_data::scene::save::prepare_game_with;
use khora_data::scene::snapshot::{
    prepare_snapshot, prepare_snapshot_with, write_snapshot, SNAPSHOT_ENCODING_ID,
};
use khora_data::scene::{
    about_what_loads, capture_save_reporting, capture_world_reporting, collapse, compose_reporting,
    encoding_of, expand_reporting, prepare, prepare_game, prepare_with, read_into, read_save_file,
    read_scene_file, write_save_file, write_scene_file, CompactEncoding, Identity, LoadCatalog,
    LoadFailure, MsgPackEncoding, NoPrefabs, PrefabSource, Prepared, SaveError, SceneEncoding,
    SceneFileReadError, SceneRecord, TextEncoding,
};

use crate::asset::{AssetService, PrefabAsset};

pub use super::asset_prefabs::AssetPrefabs;
pub use super::reader_prefabs::ReaderPrefabs;

/// Scene format version produced by today's writers.
pub const CURRENT_SCENE_VERSION: u32 = SCENE_FORMAT_VERSION as u32;

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

    /// Returns where this service reads prefabs from.
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
    ///
    /// # Errors
    ///
    /// [`SerializationServiceError::SaveFailed`] when the world cannot be
    /// captured or encoded.
    pub fn save_world(
        &self,
        world: &World,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        self.save_world_reporting(world, goal).map(|(file, _)| file)
    }

    /// Saves as [`save_world`](Self::save_world), and returns what the save left out.
    ///
    /// A component the save cannot write, such as a mesh made in code, is
    /// reported [`ReportKind::NotSaved`](khora_data::scene::record::ReportKind::NotSaved)
    /// on its entity; the entity is saved without it.
    ///
    /// # Errors
    ///
    /// As [`save_world`](Self::save_world).
    pub fn save_world_reporting(
        &self,
        world: &World,
        goal: SerializationGoal,
    ) -> Result<(SceneFile, Vec<ReportEntry>), SerializationServiceError> {
        let prefabs = self
            .prefabs
            .as_deref()
            .map(|source| source as &dyn PrefabSource);
        Self::save(world, goal, prefabs)
    }

    /// Saves as [`save_world`](Self::save_world), linking to the prefabs `prefabs` reads.
    ///
    /// # Errors
    ///
    /// As [`save_world`](Self::save_world).
    pub fn save_world_with(
        &self,
        world: &World,
        goal: SerializationGoal,
        prefabs: &dyn PrefabSource,
    ) -> Result<SceneFile, SerializationServiceError> {
        Self::save(world, goal, Some(prefabs)).map(|(file, _)| file)
    }

    /// Saves `world` for `goal`, linking to the prefabs `prefabs` reads, and
    /// returns what the save left out.
    fn save(
        world: &World,
        goal: SerializationGoal,
        prefabs: Option<&dyn PrefabSource>,
    ) -> Result<(SceneFile, Vec<ReportEntry>), SerializationServiceError> {
        let encoding = match Self::encoding_for(goal) {
            Some(encoding) => encoding,
            None => match write_snapshot(world) {
                // A snapshot keeps every value: it leaves nothing out.
                Ok(file) => return Ok((file, Vec::new())),
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
        let (mut record, left_out) =
            capture_world_reporting(world).map_err(SerializationServiceError::SaveFailed)?;
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
            .map(|file| (file, left_out))
            .map_err(|error| SerializationServiceError::SaveFailed(SaveError::Encoding(error.0)))
    }

    /// Reads and stages the scene `file` holds, in the encoding its header names, adding nothing.
    fn prepare_file(
        file: &SceneFile,
        world: &mut World,
        prefabs: &dyn PrefabSource,
    ) -> Result<(Prepared, Vec<ReportEntry>), SerializationServiceError> {
        if encoding_of(file) == SNAPSHOT_ENCODING_ID {
            // The load keeps its file to be prepared again; the bytes are
            // copied once, a small part of reading them.
            return prepare_snapshot(world, Arc::new(file.clone()))
                .map(|prepared| (prepared, Vec::new()))
                .map_err(SerializationServiceError::LoadFailed);
        }
        let record = read_scene_file(file).map_err(SerializationServiceError::ReadFailed)?;
        let (record, mut left_out) = read_into(&world.load_catalog(), record);
        let (record, expansion) = Self::expanded(record, prefabs)?;
        left_out.extend(expansion);
        prepare(world, record)
            .map(|prepared| (prepared, left_out))
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Prepares as `prepare_file` without the world, on any thread.
    ///
    /// What it reads of the world is `catalog`; the ids it reserves are held by
    /// `hold` until the load is committed, abandoned or dropped.
    ///
    /// # Errors
    ///
    /// [`SerializationServiceError::ReadFailed`] when `file` cannot be read, and
    /// [`SerializationServiceError::LoadFailed`] when its prefabs cannot be
    /// expanded or the record does not fit `catalog`.
    pub(crate) fn prepare_file_with(
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
        prepare_with(catalog, hold, record)
            .map(|prepared| (prepared, left_out))
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Prepares what [`load_game`](Self::load_game) loads without the world, on any thread.
    ///
    /// Read against `catalog`, its ids held by `hold`; returns what reading,
    /// expanding and merging adapted too.
    ///
    /// # Errors
    ///
    /// [`SerializationServiceError::ReadFailed`] when either file cannot be
    /// read, and [`SerializationServiceError::LoadFailed`] when the prefabs
    /// cannot be expanded, the save does not merge, or the result does not fit
    /// `catalog`.
    pub(crate) fn prepare_game_with(
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
        prepare_game_with(catalog, hold, record)
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
    ///
    /// # Errors
    ///
    /// As [`load_world`](Self::load_world).
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
    ///
    /// # Errors
    ///
    /// As [`replace_world`](Self::replace_world).
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
    ///
    /// # Errors
    ///
    /// [`SerializationServiceError::ReadFailed`] when `base_file` cannot be read,
    /// [`SerializationServiceError::LoadFailed`] when its prefabs cannot be
    /// expanded, and [`SerializationServiceError::SaveFailed`] when the save
    /// cannot be captured or encoded.
    pub fn save_game(
        &self,
        world: &World,
        base_id: AssetUUID,
        base_file: &SceneFile,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        self.save_game_reporting(world, base_id, base_file, goal)
            .map(|(file, _)| file)
    }

    /// Saves as [`save_game`](Self::save_game), and returns what the save left out.
    ///
    /// A value only the session holds (a mesh made in code) is never written:
    /// it is reported `NotSaved` on its entity.
    ///
    /// # Errors
    ///
    /// As [`save_game`](Self::save_game).
    pub fn save_game_reporting(
        &self,
        world: &World,
        base_id: AssetUUID,
        base_file: &SceneFile,
        goal: SerializationGoal,
    ) -> Result<(SceneFile, Vec<ReportEntry>), SerializationServiceError> {
        let base = read_scene_file(base_file).map_err(SerializationServiceError::ReadFailed)?;
        // Against the scene as a world: its prefab instances expanded.
        let (base, _) = Self::expanded(base, self.prefabs())?;
        let (save, left_out) = capture_save_reporting(world, base_id, &base)
            .map_err(SerializationServiceError::SaveFailed)?;
        let encoding = Self::encoding_for(goal).unwrap_or(&CompactEncoding);
        write_save_file(&save, encoding)
            .map(|file| (file, left_out))
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
            prepare_game(world, record).map_err(SerializationServiceError::LoadFailed)?;
        let mut report = Self::replace_with(prepared, world);
        report.entries.extend(left_out);
        Ok(report)
    }

    /// Returns the scene a game save was taken against.
    ///
    /// # Errors
    ///
    /// [`SerializationServiceError::ReadFailed`] when `save_file` is no game save.
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
        Some(metadata) if metadata.kind == kinds::PREFAB => {}
        Some(metadata) => {
            return Err(format!(
                "the asset {id} is a {}, not a prefab",
                metadata.kind
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
        .filter(|metadata| metadata.kind == kinds::PREFAB)
        .map(|metadata| metadata.uuid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_data::ecs::{Name, Transform};
    use khora_data::scene::NoPrefabs;

    /// Returns a world of one authored entity per name.
    fn world_of(names: &[&str]) -> World {
        let mut world = World::new();
        for name in names {
            let entity = world.spawn((Transform::identity(), Name::new((*name).to_owned())));
            world.mark_authored(entity).expect("authored");
        }
        world
    }

    /// Returns the names of `world`'s entities, sorted.
    fn names(world: &World) -> Vec<String> {
        let mut names: Vec<String> = world
            .iter_entities()
            .filter_map(|entity| world.get::<Name>(entity))
            .map(|name| name.as_str().to_owned())
            .collect();
        names.sort();
        names
    }

    /// Saving with a prefab source holding no prefab writes the scene as `save_world` does.
    #[test]
    fn save_world_with_no_prefabs_writes_what_save_world_writes() {
        let world = world_of(&["Hero"]);
        let service = SerializationService::new();
        let goal = SerializationGoal::HumanReadableDebug;

        let with = service
            .save_world_with(&world, goal, &NoPrefabs)
            .expect("the world saves");
        let plain = service.save_world(&world, goal).expect("the world saves");

        assert_eq!(with.to_bytes(), plain.to_bytes());
    }

    /// A scene save reports a mesh made in code left out; a snapshot keeps it
    /// and reports nothing.
    #[test]
    fn save_world_reporting_reports_what_a_scene_leaves_out_and_a_snapshot_keeps() {
        let mut world = world_of(&["Hero"]);
        let made = world.spawn((
            Transform::identity(),
            khora_data::ecs::MeshRef::Made(khora_core::asset::AssetUUID::new()),
        ));
        let id = world.mark_authored(made).expect("authored");
        let service = SerializationService::new();

        let (_, scene) = service
            .save_world_reporting(&world, SerializationGoal::EditorInterchange)
            .expect("the scene saves");
        let (_, snapshot) = service
            .save_world_reporting(&world, SerializationGoal::FastestLoad)
            .expect("the snapshot saves");

        assert!(
            scene.iter().any(|entry| entry.entity == Some(id)
                && entry.kind == khora_data::scene::record::ReportKind::NotSaved),
            "the scene save does not report the mesh made in code: {scene:?}"
        );
        assert!(
            snapshot.is_empty(),
            "a snapshot left something out: {snapshot:?}"
        );
    }

    /// A game save reports a mesh made in code it leaves out.
    #[test]
    fn save_game_reporting_reports_a_mesh_made_in_code_left_out() {
        let mut world = world_of(&["Hero"]);
        let service = SerializationService::new();
        let base = service
            .save_world(&world, SerializationGoal::EditorInterchange)
            .expect("the scene saves");
        let hero = world.iter_entities().next().expect("the hero");
        world
            .add_component(
                hero,
                khora_data::ecs::MeshRef::Made(khora_core::asset::AssetUUID::new()),
            )
            .expect("the hero takes a mesh");

        let (_, left_out) = service
            .save_game_reporting(
                &world,
                khora_core::asset::AssetUUID::new(),
                &base,
                SerializationGoal::EditorInterchange,
            )
            .expect("the game saves");

        assert!(
            left_out
                .iter()
                .any(|entry| entry.entity == world.persistent_id(hero)
                    && entry.kind == khora_data::scene::record::ReportKind::NotSaved),
            "the game save does not report the mesh made in code: {left_out:?}"
        );
    }

    /// Loading with a prefab source adds the scene beside what the world holds.
    #[test]
    fn load_world_with_adds_the_scene_beside_the_world() {
        let service = SerializationService::new();
        let file = service
            .save_world(&world_of(&["Hero"]), SerializationGoal::EditorInterchange)
            .expect("the scene saves");
        let mut world = world_of(&["Old"]);

        service
            .load_world_with(&file, &mut world, &NoPrefabs)
            .expect("the scene loads");

        assert_eq!(names(&world), ["Hero", "Old"]);
    }

    /// Replacing with a prefab source leaves only the scene in the world.
    #[test]
    fn replace_world_with_leaves_only_the_scene() {
        let service = SerializationService::new();
        let file = service
            .save_world(&world_of(&["Hero"]), SerializationGoal::EditorInterchange)
            .expect("the scene saves");
        let mut world = world_of(&["Old"]);

        service
            .replace_world_with(&file, &mut world, &NoPrefabs)
            .expect("the scene loads");

        assert_eq!(names(&world), ["Hero"]);
    }

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
