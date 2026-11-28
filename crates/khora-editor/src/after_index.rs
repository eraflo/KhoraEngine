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

use khora_sdk::editor_ui::KeyChange;
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::prelude::ecs::EntityId;

/// What waits for a reindex of the project to land, because it needs the
/// index to hold what was just written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterIndex {
    /// Reveals the asset at this forward-slash path in the browser (queues
    /// `EditorRequest::RevealAsset`).
    Reveal(String),
    /// Opens the scene at this forward-slash project path (queues `EditorRequest::OpenScene`).
    ///
    /// Only if the index now holds it, and no other scene was chosen since
    /// `choice` (`EditorState::scene_choices`) nor is asked for; a scene the
    /// index still lacks is reported, without indexing again.
    OpenScene { rel: String, choice: u64 },
    /// Replaces `entity`'s inline material with `AssetRef::Asset(uuid)`.
    ///
    /// Only if the entity is alive, still wears the inline material saved
    /// (`saved`, its RON text), and no other scene was chosen since `choice`.
    /// The entity is found by `persistent` when it has one: a restore at Stop
    /// gives it another id. Saved while `played`, it is for the played scene
    /// only; saved while editing, it waits for the edited scene to come back.
    AssignMaterial {
        entity: EntityId,
        persistent: Option<PersistentId>,
        uuid: AssetUUID,
        saved: String,
        choice: u64,
        played: bool,
    },
}

impl AfterIndex {
    /// Returns whether this assignment and `other` are for one entity of one
    /// scene state — edited or played — so that the one saved last replaces it.
    pub(crate) fn assigns_as(&self, other: &AfterIndex) -> bool {
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

    /// Returns this action as it reads once `change` is made; `None` when it named what `change` trashed.
    ///
    /// A path moved is named by its new path; a material to assign is named
    /// by its identity, which a move keeps.
    pub(crate) fn followed(self, change: &KeyChange) -> Option<AfterIndex> {
        match self {
            AfterIndex::Reveal(rel) => change.follow(&rel).map(AfterIndex::Reveal),
            AfterIndex::OpenScene { rel, choice } => change
                .follow(&rel)
                .map(|rel| AfterIndex::OpenScene { rel, choice }),
            AfterIndex::AssignMaterial { uuid, .. } if change.trashed(uuid) => None,
            assign @ AfterIndex::AssignMaterial { .. } => Some(assign),
        }
    }
}

/// A reindex of the project that landed: its outcome, and the actions that
/// waited for it.
#[derive(Debug)]
pub struct Landed {
    pub outcome: khora_sdk::IndexOutcome,
    pub actions: Vec<AfterIndex>,
}

#[cfg(test)]
mod tests {
    use khora_sdk::editor_ui::KeyChange;
    use khora_sdk::khora_core::asset::AssetUUID;
    use khora_sdk::prelude::ecs::EntityId;

    use super::AfterIndex;

    fn assign(uuid: AssetUUID) -> AfterIndex {
        AfterIndex::AssignMaterial {
            entity: EntityId {
                index: 0,
                generation: 0,
            },
            persistent: None,
            uuid,
            saved: String::new(),
            choice: 0,
            played: false,
        }
    }

    /// A path moved is followed; a material to assign keeps its identity through a move.
    #[test]
    fn followed_names_a_moved_path_by_its_new_one() {
        let change = KeyChange::Moved {
            from: "scenes".to_owned(),
            to: "levels".to_owned(),
        };
        let material = assign(AssetUUID::new_v5("materials/a.kmat"));

        assert_eq!(
            AfterIndex::Reveal("scenes/a.kscene".to_owned()).followed(&change),
            Some(AfterIndex::Reveal("levels/a.kscene".to_owned()))
        );
        assert_eq!(
            AfterIndex::OpenScene {
                rel: "scenes/a.kscene".to_owned(),
                choice: 3
            }
            .followed(&change),
            Some(AfterIndex::OpenScene {
                rel: "levels/a.kscene".to_owned(),
                choice: 3
            })
        );
        assert_eq!(material.clone().followed(&change), Some(material));
    }

    /// What names a path or an asset trashed is dropped.
    #[test]
    fn followed_drops_what_names_what_was_trashed() {
        let gone = AssetUUID::new_v5("materials/a.kmat");
        let change = KeyChange::Trashed {
            key: "materials".to_owned(),
            identities: vec![gone],
        };

        assert_eq!(
            AfterIndex::Reveal("materials/a.kmat".to_owned()).followed(&change),
            None
        );
        assert_eq!(assign(gone).followed(&change), None);
        let kept = assign(AssetUUID::new_v5("textures/a.png"));
        assert_eq!(kept.clone().followed(&change), Some(kept));
    }
}
