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

//! An asset activated (double-clicked) in the browser: a prefab is queued to
//! spawn by its identity.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetEntry, EditorState, UiTheme};
use khora_sdk::khora_core::asset::AssetUUID;

use super::super::AssetBrowserPanel;

const ROCKET: &str = "prefabs/weapons/rocket.kprefab";

/// Activating a prefab's tile queues its spawn by the prefab's identity, with
/// no parent: the editor spawns it on its next frame.
#[test]
fn activating_a_prefab_queues_its_spawn_by_identity() {
    let uuid = AssetUUID::new_v5(ROCKET);
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some("project".to_owned()),
        asset_entries: vec![AssetEntry {
            name: "rocket.kprefab".to_owned(),
            asset_type: "prefab".to_owned(),
            source_path: ROCKET.to_owned(),
            uuid,
            container: None,
        }],
        asset_epoch: 1,
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.rescan_if_needed();
    let rocket = panel
        .flat
        .iter()
        .find(|asset| asset.rel_path == ROCKET)
        .cloned()
        .expect("the browser lists the prefab");

    panel.activate_asset(&rocket);

    assert_eq!(
        state.lock().expect("state").pending_prefab_spawn,
        Some((uuid, None))
    );
}
