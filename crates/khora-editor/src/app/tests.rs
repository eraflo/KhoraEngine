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

//! The editor's engine mode: it starts editing, which is not the game, and
//! each frame's mode follows the transport.

use khora_sdk::{EngineApp, EngineMode, PlayMode};

use super::{engine_mode_for, EditorApp};

fn editor_mode() -> EngineMode {
    EngineMode::Custom("editor".to_owned())
}

/// **The editor is not the game.** It starts in its own mode, so no script
/// runs before anyone presses Play.
#[test]
fn the_editor_starts_in_its_own_mode() {
    let app = <EditorApp as EngineApp>::new();

    assert_eq!(app.initial_mode(), editor_mode());
}

/// Editing is the editor's mode; Play is the game.
#[test]
fn editing_is_the_editors_mode_and_playing_is_the_game() {
    assert_eq!(engine_mode_for(PlayMode::Editing), editor_mode());
    assert_eq!(engine_mode_for(PlayMode::Playing), EngineMode::Playing);
}

/// **A paused game is still the game.** Its scripts run — at a delta of
/// zero — as a pause menu needs.
#[test]
fn a_paused_game_is_still_the_game() {
    assert_eq!(engine_mode_for(PlayMode::Paused), EngineMode::Playing);
}
