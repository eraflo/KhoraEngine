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

//! **A quarantined asset is shown refused, and can be released.** Its tile
//! in the browser is drawn refused — its name in the error colour; its
//! details pane shows a `Quarantined` card — why, what to do, what refused
//! it, the trust it was read at, what holds it — and a `Release` button
//! that asks the app to release it.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetEntry, EditorPanel, EditorState, QuarantinedAsset, UiTheme};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::AssetBrowserPanel;
use crate::commands::CommandHistory;
use crate::panels::PropertiesPanel;

const HUGE: &str = "textures/huge.png";

fn entry(path: &str) -> AssetEntry {
    AssetEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        asset_type: "texture".to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::new_v5(path),
        container: None,
    }
}

/// `textures/huge.png` quarantined: too large to be read restricted.
fn huge_quarantined() -> QuarantinedAsset {
    QuarantinedAsset {
        rel_path: HUGE.to_owned(),
        reason: "is too large".to_owned(),
        remedy: "Make it smaller.".to_owned(),
        refused_by: "file guard".to_owned(),
        read_as: "restricted project".to_owned(),
        content: "size and date".to_owned(),
    }
}

/// One frame of the browser listing `huge.png` and `calm.png`, the first
/// quarantined, its tree opened on `textures`.
fn browse() -> RecordingUiBuilder {
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some("project".to_owned()),
        asset_entries: vec![entry(HUGE), entry("textures/calm.png")],
        asset_epoch: 1,
        quarantined: vec![huge_quarantined()],
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.rescan_if_needed();
    panel.current_folder = Some("textures".to_owned());
    let mut ui = RecordingUiBuilder::new(1200.0, 420.0);
    panel.ui(&mut ui);
    ui
}

/// One frame of the inspector on the asset at `rel`, `ui` scripted; what it
/// painted and the editor state after.
fn inspect(
    rel: &str,
    quarantined: Vec<QuarantinedAsset>,
    ui: RecordingUiBuilder,
) -> (RecordingUiBuilder, EditorState) {
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some(
            std::env::temp_dir()
                .join("khora-editor-quarantine-pane")
                .to_string_lossy()
                .into_owned(),
        ),
        inspected_asset_path: Some(rel.to_owned()),
        quarantined,
        ..EditorState::default()
    }));
    let mut panel = PropertiesPanel::new(
        Arc::clone(&state),
        Arc::new(Mutex::new(CommandHistory::default())),
        UiTheme::default(),
    );
    let mut ui = ui;
    panel.ui(&mut ui);
    let after = state.lock().expect("the editor state").clone();
    (ui, after)
}

fn pane() -> RecordingUiBuilder {
    RecordingUiBuilder::new(360.0, 900.0)
}

/// **A quarantined asset's tile is drawn refused**: its name in the error
/// colour — an error, where the restricted mode's amber is a state. The
/// tile beside it, not quarantined, is drawn as ever.
#[test]
fn a_quarantined_assets_tile_is_drawn_refused() {
    let theme = UiTheme::default();
    let ui = browse();

    assert!(
        ui.painted_text("calm.png"),
        "the premise: the tiles are painted: {:?}",
        ui.texts()
    );
    assert_eq!(
        ui.text_color("huge.png"),
        Some(theme.error),
        "the quarantined asset's name is in the error colour"
    );
    assert_ne!(
        ui.text_color("calm.png"),
        Some(theme.error),
        "the other asset's is not"
    );
}

/// **A quarantined asset's details say why, and by what**: a `Quarantined`
/// card with the reason, the remedy, `Refused by` the file guard, `Read as`
/// a restricted project, `Content` held by its size and date, and a
/// `Release` button. An asset not quarantined shows no such card.
#[test]
fn a_quarantined_assets_details_show_its_quarantine() {
    let (ui, _) = inspect(HUGE, vec![huge_quarantined()], pane());

    for painted in [
        "Quarantined",
        "is too large",
        "Make it smaller.",
        "Refused by",
        "file guard",
        "Read as",
        "restricted project",
        "Content",
        "size and date",
        "Release",
    ] {
        assert!(
            ui.painted_text(painted),
            "`{painted}` is painted: {:?}",
            ui.texts()
        );
    }

    let (other, _) = inspect("textures/calm.png", vec![huge_quarantined()], pane());
    assert!(
        !other.painted_text("Quarantined"),
        "no quarantine card for an asset not quarantined: {:?}",
        other.texts()
    );
}

/// **`Release` asks the app to release the asset**, by its path under
/// `assets/`.
#[test]
fn release_asks_to_release_the_asset() {
    let (_, after) = inspect(
        HUGE,
        vec![huge_quarantined()],
        pane().with_click("asset-quarantine-release"),
    );

    assert_eq!(after.pending_release.as_deref(), Some(HUGE));
}
