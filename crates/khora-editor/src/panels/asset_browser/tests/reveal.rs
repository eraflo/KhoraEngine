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

//! The browser following an asset an inspector slot asked it to reveal: its
//! folder opened down to it, its tile selected with no filter hiding it,
//! the inspector on it.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetEntry, EditorState, UiTheme};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::MetricsRegistry;

use super::super::AssetBrowserPanel;
use crate::hot_reload::collect_asset_entries;
use crate::project_vfs::ProjectVfs;
use crate::widgets::tile::AssetTileKind;

/// The path an asset slot asks the browser to reveal: forward slashes, under
/// `assets/`.
const ROCKET: &str = "prefabs/weapons/rocket.kprefab";

fn entry(path: &str, asset_type: &str) -> AssetEntry {
    AssetEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        asset_type: asset_type.to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::new_v5(path),
        container: None,
    }
}

/// A browser over a project holding `entries`, its list read, its tree on the
/// root folder with a search typed and a kind filter chosen, an entity
/// selected and inspected.
fn browsing(entries: Vec<AssetEntry>) -> (AssetBrowserPanel, Arc<Mutex<EditorState>>) {
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some("project".to_owned()),
        asset_entries: entries,
        asset_epoch: 1,
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.rescan_if_needed();
    panel.search_filter = "alarm".to_owned();
    panel.selected_filter = Some(AssetTileKind::Audio);
    if let Ok(mut state) = state.lock() {
        state.select(EntityId {
            index: 3,
            generation: 1,
        });
    }
    (panel, state)
}

#[test]
fn a_revealed_asset_is_selected_in_its_opened_folder_and_inspected() {
    let (mut panel, state) = browsing(vec![
        entry("sounds/alarm.wav", "audio"),
        entry(ROCKET, "prefab"),
    ]);
    state.lock().expect("state").pending_reveal_asset = Some(ROCKET.to_owned());

    panel.follow_reveal();

    let state = state.lock().expect("state");
    assert_eq!(state.pending_reveal_asset, None, "the request is consumed");
    assert_eq!(state.selected_asset, Some(1));
    assert_eq!(state.inspected_asset_path.as_deref(), Some(ROCKET));
    assert!(
        state.selection.is_empty(),
        "the entity is no longer selected"
    );
    assert_eq!(panel.selected_index, Some(1));
    assert_eq!(panel.current_folder.as_deref(), Some("prefabs/weapons"));
    for folder in ["", "prefabs", "prefabs/weapons"] {
        assert_eq!(
            panel.expanded_folders.get(folder),
            Some(&true),
            "`{folder}` is expanded"
        );
    }
    assert!(panel.search_filter.is_empty(), "no search hides it");
    assert_eq!(panel.selected_filter, None, "no kind filter hides it");
}

#[test]
fn revealing_an_asset_the_browser_does_not_list_changes_nothing_but_the_request() {
    let (mut panel, state) = browsing(vec![entry("sounds/alarm.wav", "audio")]);
    state.lock().expect("state").pending_reveal_asset = Some(ROCKET.to_owned());

    panel.follow_reveal();

    let state = state.lock().expect("state");
    assert_eq!(state.pending_reveal_asset, None);
    assert_eq!(state.selected_asset, None);
    assert_eq!(state.inspected_asset_path, None);
    assert_eq!(state.selection.len(), 1, "the entity stays selected");
    assert_eq!(panel.search_filter, "alarm");
}

/// The browser lists what the project's index holds, with the paths the
/// index wrote; a slot asks for one with forward slashes, whatever the
/// platform. The browser finds it either way.
#[test]
fn an_asset_listed_from_the_projects_index_is_revealed_by_its_forward_slash_path(
) -> anyhow::Result<()> {
    let root =
        std::env::temp_dir().join(format!("khora-editor-reveal-asset-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let weapons = root.join("assets").join("prefabs").join("weapons");
    std::fs::create_dir_all(&weapons)?;
    std::fs::write(weapons.join("rocket.kprefab"), b"prefab bytes")?;

    let entries = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .map(|pvfs| collect_asset_entries(&pvfs));
    let _ = std::fs::remove_dir_all(&root);
    let entries = entries?;
    let at = entries
        .iter()
        .position(|entry| entry.name == "rocket.kprefab")
        .ok_or_else(|| anyhow::anyhow!("rocket is not indexed: {entries:?}"))?;

    let (mut panel, state) = browsing(entries);
    state.lock().expect("state").pending_reveal_asset = Some(ROCKET.to_owned());
    panel.follow_reveal();

    let state = state.lock().expect("state");
    assert_eq!(
        state.selected_asset,
        Some(at),
        "the rocket is revealed; the browser lists {:?}",
        panel
            .flat
            .iter()
            .map(|asset| asset.rel_path.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(panel.current_folder.as_deref(), Some("prefabs/weapons"));
    Ok(())
}
