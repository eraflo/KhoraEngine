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

//! Scene serialization service — on-demand, not an Agent.
//!
//! `save_world` captures a world as a scene record and writes it in the
//! encoding a [`SerializationGoal`] calls for; `load_world` and
//! `replace_world` read one back, atomically. No GORNA negotiation — the goal
//! is the caller's to state.

use khora_core::scene::{SceneFile, SerializationGoal, SCENE_FORMAT_VERSION};
use khora_data::ecs::World;
use khora_data::scene::record::LoadReport;
use khora_data::scene::{
    apply, capture_world, prepare, read_scene_file, write_scene_file, CompactEncoding, Identity,
    LoadFailure, MsgPackEncoding, SaveError, SceneEncoding, SceneFileReadError, TextEncoding,
};

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

/// The serialization service.
///
/// Provides on-demand scene save and load. Stateless: construct it where it
/// is needed.
#[derive(Debug, Default)]
pub struct SerializationService;

impl SerializationService {
    /// Creates the service.
    pub fn new() -> Self {
        Self
    }

    /// The encoding a goal calls for.
    fn encoding_for(goal: SerializationGoal) -> &'static dyn SceneEncoding {
        match goal {
            SerializationGoal::HumanReadableDebug | SerializationGoal::LongTermStability => {
                &TextEncoding
            }
            SerializationGoal::SmallestFileSize
            | SerializationGoal::EditorInterchange
            | SerializationGoal::FastestLoad => &CompactEncoding,
            SerializationGoal::PortableBinary => &MsgPackEncoding,
        }
    }

    /// Saves the current state of the `World` based on a high-level goal.
    pub fn save_world(
        &self,
        world: &World,
        goal: SerializationGoal,
    ) -> Result<SceneFile, SerializationServiceError> {
        let record = capture_world(world).map_err(SerializationServiceError::SaveFailed)?;
        write_scene_file(&record, Self::encoding_for(goal))
            .map_err(|error| SerializationServiceError::SaveFailed(SaveError::Encoding(error.0)))
    }

    /// Brings the scene `file` holds into `world`, beside what is already
    /// there — or, if it cannot be loaded, leaves `world` exactly as it was.
    pub fn load_world(
        &self,
        file: &SceneFile,
        world: &mut World,
    ) -> Result<LoadReport, SerializationServiceError> {
        let record = read_scene_file(file).map_err(SerializationServiceError::ReadFailed)?;
        apply(world, &record, Identity::Keep)
            .map(|applied| applied.report)
            .map_err(SerializationServiceError::LoadFailed)
    }

    /// Replaces everything in `world` with the scene `file` holds — or, if
    /// it cannot be loaded, leaves `world` exactly as it was.
    ///
    /// The scene is read and staged before anything is removed, so a file
    /// that fails to load costs nothing; and the old entities are gone before
    /// the new ones arrive, so every saved identity is restored as saved.
    pub fn replace_world(
        &self,
        file: &SceneFile,
        world: &mut World,
    ) -> Result<LoadReport, SerializationServiceError> {
        let record = read_scene_file(file).map_err(SerializationServiceError::ReadFailed)?;
        let prepared = prepare(world, &record).map_err(SerializationServiceError::LoadFailed)?;
        let old: Vec<_> = world.iter_entities().collect();
        for entity in old {
            world.despawn(entity);
        }
        Ok(prepared.commit(world, Identity::Keep).report)
    }
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
            (SerializationGoal::FastestLoad, "KH_COMPACT_V2"),
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
