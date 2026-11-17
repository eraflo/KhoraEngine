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

//! Reading what the editor's panels queued, without taking it.

use khora_sdk::editor_ui::{AssetFileOp, EditorRequest};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::EditorState;

/// The queued requests `pick` reads, oldest first; the queue is left as it is.
fn queued<T>(state: &EditorState, pick: impl Fn(&EditorRequest) -> Option<T>) -> Vec<T> {
    state.requests.iter().filter_map(pick).collect()
}

/// The prefabs queued to spawn, with the entity each is parented under.
pub(crate) fn prefab_spawns(state: &EditorState) -> Vec<(AssetUUID, Option<EntityId>)> {
    queued(state, |request| match request {
        EditorRequest::SpawnPrefab { prefab, parent } => Some((*prefab, *parent)),
        _ => None,
    })
}

/// The meshes queued to spawn: identity, world point and parent.
pub(crate) fn mesh_spawns(state: &EditorState) -> Vec<(AssetUUID, [f32; 3], Option<EntityId>)> {
    queued(state, |request| match request {
        EditorRequest::SpawnMesh {
            mesh,
            point,
            parent,
        } => Some((*mesh, *point, *parent)),
        _ => None,
    })
}

/// The textures and materials queued to assign to the entity each was dropped on.
pub(crate) fn dropped_assignments(state: &EditorState) -> Vec<(AssetUUID, EntityId)> {
    queued(state, |request| match request {
        EditorRequest::AssignDropped { asset, entity } => Some((*asset, *entity)),
        _ => None,
    })
}

/// The scene files queued to open.
pub(crate) fn scene_opens(state: &EditorState) -> Vec<String> {
    queued(state, |request| match request {
        EditorRequest::OpenScene { path } => Some(path.clone()),
        _ => None,
    })
}

/// The assets queued to reveal in the browser.
pub(crate) fn reveals(state: &EditorState) -> Vec<String> {
    queued(state, |request| match request {
        EditorRequest::RevealAsset { path } => Some(path.clone()),
        _ => None,
    })
}

/// The quarantined assets queued to release.
pub(crate) fn releases(state: &EditorState) -> Vec<String> {
    queued(state, |request| match request {
        EditorRequest::ReleaseAsset { path } => Some(path.clone()),
        _ => None,
    })
}

/// The file operations queued on the project's assets.
pub(crate) fn file_ops(state: &EditorState) -> Vec<AssetFileOp> {
    queued(state, |request| match request {
        EditorRequest::AssetFile(op) => Some(op.clone()),
        _ => None,
    })
}
