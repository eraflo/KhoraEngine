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
//! What waits for a reindex of the project to land off the frame, and what a
//! landing hands back.

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::prelude::ecs::EntityId;

/// What waits for a reindex of the project to land, because it needs the
/// index to hold what was just written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterIndex {
    /// Reveals the asset at this forward-slash path in the browser (sets
    /// `pending_reveal_asset`).
    Reveal(String),
    /// Opens the scene at this forward-slash path of the project (sets
    /// `pending_scene_load`) if the index now holds it, and no other scene
    /// was chosen since `choice` (`EditorState::scene_choices`) nor is
    /// asked for; a scene the index still lacks is reported, without
    /// indexing again.
    OpenScene { rel: String, choice: u64 },
    /// Replaces `entity`'s inline material with `MaterialRef::Asset(uuid)`,
    /// if the entity is alive and still wears the inline material saved,
    /// `saved` being its RON text. The entity is looked up by `persistent`,
    /// its identity, when it has one: a restore at Stop gives it another id.
    /// Nothing once another scene was chosen since `choice`. Saved while
    /// the scene was `played`, it is for the played scene only; saved while
    /// editing, it waits for the edited scene to come back.
    AssignMaterial {
        entity: EntityId,
        persistent: Option<PersistentId>,
        uuid: AssetUUID,
        saved: String,
        choice: u64,
        played: bool,
    },
}

/// A reindex of the project that landed: its outcome, and the actions that
/// waited for it.
#[derive(Debug)]
pub struct Landed {
    pub outcome: khora_sdk::IndexOutcome,
    pub actions: Vec<AfterIndex>,
}

impl AfterIndex {
    /// Whether this assignment and `other` are for one entity of one scene
    /// state — edited or played — so that the one saved last replaces it.
    pub fn assigns_as(&self, other: &AfterIndex) -> bool {
        match (self, other) {
            (
                AfterIndex::AssignMaterial {
                    entity,
                    persistent,
                    played,
                    ..
                },
                AfterIndex::AssignMaterial {
                    entity: other_entity,
                    persistent: other_persistent,
                    played: other_played,
                    ..
                },
            ) => {
                played == other_played
                    && match (persistent, other_persistent) {
                        (Some(id), Some(other_id)) => id == other_id,
                        _ => entity == other_entity,
                    }
            }
            _ => false,
        }
    }
}
