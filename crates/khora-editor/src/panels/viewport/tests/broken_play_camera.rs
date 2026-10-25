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

//! **A game camera that renders nothing says so over the image**: while the
//! game plays through a camera whose values make no image, the viewport
//! names the camera, says what is wrong, and points to Problems — rather
//! than showing a blank frame.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorPanel, EditorState, EntityIcon, PlayMode, SceneNode, UiTheme};
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::ViewportTextureHandle;
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::ViewportPanel;
use crate::camera::EditorCamera;

const MESSAGE: &str = "near must be greater than 0 (it is 0)";

fn lens() -> EntityId {
    EntityId {
        index: 5,
        generation: 1,
    }
}

/// One frame of the viewport, over a 1200 × 700 image, with `state`.
fn frame(state: EditorState) -> RecordingUiBuilder {
    let state = Arc::new(Mutex::new(state));
    let mut panel = ViewportPanel::new(
        ViewportTextureHandle(1),
        Arc::clone(&state),
        Arc::new(Mutex::new(EditorCamera::default())),
        UiTheme::default(),
    );
    let mut ui = RecordingUiBuilder::new(1200.0, 700.0).with_viewport_image();
    panel.ui(&mut ui);
    ui
}

#[test]
fn a_broken_play_camera_is_named_over_the_image() {
    let state = EditorState {
        play_mode: PlayMode::Playing,
        broken_play_camera: Some((lens(), MESSAGE.to_owned())),
        scene_roots: vec![SceneNode {
            entity: lens(),
            name: "Lens 7".to_owned(),
            icon: EntityIcon::Camera,
            children: Vec::new(),
            tag_count: 0,
        }],
        ..EditorState::default()
    };

    let ui = frame(state);

    let title = ui
        .texts()
        .into_iter()
        .find(|text| text.contains("renders nothing"))
        .unwrap_or_else(|| panic!("a title says the camera renders nothing: {:?}", ui.texts()));
    assert!(title.contains("Lens 7"), "it names the camera: {title:?}");
    assert!(ui.painted_text(MESSAGE), "it says what is wrong");
    assert!(ui.painted_text("Problems"), "it points to Problems");
}

/// Once the game stops, the overlay goes, even on a frame where the broken
/// camera was not cleared yet: an editor editing a scene runs no game
/// camera to blame.
#[test]
fn no_broken_camera_is_named_once_the_game_stops() {
    let state = EditorState {
        play_mode: PlayMode::Editing,
        broken_play_camera: Some((lens(), MESSAGE.to_owned())),
        ..EditorState::default()
    };

    let ui = frame(state);

    assert!(
        !ui.painted_text("renders nothing"),
        "no overlay while editing: {:?}",
        ui.texts()
    );
}
