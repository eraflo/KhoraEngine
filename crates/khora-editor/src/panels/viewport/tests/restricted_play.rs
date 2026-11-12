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

//! **Play is off in a restricted project, and says why.** While the project
//! open is restricted, the transport's Play is drawn disabled and a click on
//! it asks nothing; hovered, its tooltip says why. A trusted project's Play
//! plays, as it always did.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorPanel, EditorState, Icon, UiTheme};
use khora_sdk::ViewportTextureHandle;
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::ViewportPanel;
use crate::camera::EditorCamera;

/// One frame of the viewport over a 1200 × 700 image, `ui` scripted; what
/// it painted and the editor state after.
fn frame(restricted: bool, ui: RecordingUiBuilder) -> (RecordingUiBuilder, EditorState) {
    let state = Arc::new(Mutex::new(EditorState {
        restricted,
        ..EditorState::default()
    }));
    let mut panel = ViewportPanel::new(
        ViewportTextureHandle(1),
        Arc::clone(&state),
        Arc::new(Mutex::new(EditorCamera::default())),
        UiTheme::default(),
    );
    let mut ui = ui;
    panel.ui(&mut ui);
    let after = state.lock().expect("the editor state").clone();
    (ui, after)
}

fn viewport() -> RecordingUiBuilder {
    RecordingUiBuilder::new(1200.0, 700.0).with_viewport_image()
}

/// **A restricted project's Play is drawn disabled, and a click on it asks
/// nothing.**
#[test]
fn a_restricted_projects_play_is_disabled() {
    let theme = UiTheme::default();
    let (ui, after) = frame(true, viewport().with_click("vp-play"));

    assert_eq!(
        ui.text_color(Icon::Play.glyph()),
        Some(theme.text_disabled),
        "Play is drawn disabled"
    );
    assert_eq!(
        after.pending_menu_action, None,
        "a click on Play asks nothing"
    );

    // A trusted project's Play, the same click: it plays, drawn enabled.
    let (trusted, played) = frame(false, viewport().with_click("vp-play"));
    assert_eq!(played.pending_menu_action.as_deref(), Some("play"));
    assert_ne!(
        trusted.text_color(Icon::Play.glyph()),
        Some(theme.text_disabled)
    );
}

/// **Hovered, a restricted project's Play says why it is off.**
#[test]
fn a_restricted_projects_play_says_why_it_is_off() {
    let (ui, _) = frame(true, viewport().with_hover("vp-play"));

    assert!(
        ui.tooltips()
            .iter()
            .any(|tip| tip.contains("Play is off in restricted mode")),
        "the tooltip says why: {:?}",
        ui.tooltips()
    );
}
