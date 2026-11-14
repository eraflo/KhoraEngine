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

//! Play, Pause and Stop, the transport, as the engine sees them.
//!
//! The engine has no mode: the editor switches the play families and the clock
//! scale in `before_frame`. See the book: reference/editor.md, "04 — Play mode".

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::Activation;
use khora_sdk::prelude::SharedTime;
use khora_sdk::{EditorState, GameWorld, PlayMode, Runtime, SceneLoadHub};

use crate::ops;

/// The families a scene runs only while it is played.
pub const PLAY_FAMILIES: [AgentId; 3] = [
    khora_sdk::khora_core::script::SCRIPT,
    khora_sdk::khora_core::physics::PHYSICS,
    khora_sdk::khora_core::audio::AUDIO,
];

/// Opens editing: switches the [`PLAY_FAMILIES`] off. The editor's `setup` calls it.
pub fn start_editing(runtime: &Runtime) {
    follow(runtime, PlayMode::Editing);
}

/// Whether `action` is one of the transport's.
pub(crate) fn is_transport(action: &str) -> bool {
    matches!(action, "play" | "pause" | "stop")
}

/// Processes the transport action the editor's state holds, leaving any other for `update`.
///
/// Play snapshots the scene and is refused while a restore is under way; Stop
/// files the scene's restore with the runtime's scene loads. Whatever moved the
/// transport, the families and the clock follow where it stands, every frame.
/// See the book: reference/editor.md, "04 — Play mode".
pub fn process_transport(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    runtime: &Runtime,
) {
    let action = editor_state.lock().ok().and_then(|mut state| {
        if state
            .pending_menu_action
            .as_deref()
            .is_some_and(is_transport)
        {
            state.pending_menu_action.take()
        } else {
            None
        }
    });
    // A project turned restricted while playing — opened restricted over a
    // play — stops playing: its scripts never run.
    let restricted = khora_sdk::trust::ProjectTrustState::of(runtime)
        == khora_sdk::trust::ProjectTrust::Restricted;
    if restricted && play_mode(editor_state) != PlayMode::Editing {
        log::warn!("Play stops: the project is restricted");
        match runtime.resources.get::<Arc<SceneLoadHub>>() {
            Some(loads) => super::apply_stop(world, editor_state, loads),
            None => {
                if let Ok(mut state) = editor_state.lock() {
                    state.play_mode = PlayMode::Editing;
                }
            }
        }
        if let Ok(mut state) = editor_state.lock() {
            state.refusal = Some(PLAY_REFUSED.to_owned());
        }
        ops::sync_scene_cameras_for_mode(world, play_mode(editor_state));
    }
    if let Some(action) = action {
        match action.as_str() {
            "play" => {
                // Playing runs the project's scripts: a restricted project
                // is not played.
                if khora_sdk::trust::ProjectTrustState::of(runtime)
                    == khora_sdk::trust::ProjectTrust::Restricted
                {
                    log::warn!("Play is refused: the project is restricted");
                    if let Ok(mut state) = editor_state.lock() {
                        state.refusal = Some(PLAY_REFUSED.to_owned());
                    }
                } else {
                    super::apply_play(world, editor_state);
                }
            }
            "pause" => super::apply_pause(editor_state),
            _ => match runtime.resources.get::<Arc<SceneLoadHub>>() {
                Some(loads) => super::apply_stop(world, editor_state, loads),
                None => {
                    // Play stops all the same: the families go off, and the
                    // world stays as play left it.
                    log::error!(
                        "Stop: the engine holds no scene load hub, so the scene cannot be restored"
                    );
                    if let Ok(mut state) = editor_state.lock() {
                        state.play_mode = PlayMode::Editing;
                    }
                }
            },
        }
        ops::sync_scene_cameras_for_mode(world, play_mode(editor_state));
    }
    follow(runtime, play_mode(editor_state));
}

/// Why Play is refused in a restricted project.
const PLAY_REFUSED: &str =
    "Play is off in restricted mode: playing runs this project's scripts. Trust the project to play it.";

fn play_mode(editor_state: &Arc<Mutex<EditorState>>) -> PlayMode {
    editor_state
        .lock()
        .map(|state| state.play_mode)
        .unwrap_or(PlayMode::Editing)
}

/// Asks the engine for what `play_mode` runs: the [`PLAY_FAMILIES`] on unless
/// edited, the clock stopped only when paused.
///
/// Requests only: the frame boundary settles them.
fn follow(runtime: &Runtime, play_mode: PlayMode) {
    if let Some(activation) = runtime.resources.get::<Arc<Activation>>() {
        let played = play_mode != PlayMode::Editing;
        for family in PLAY_FAMILIES {
            // A family the engine does not run is refused and logged by the
            // activation itself; nothing else is owed to it.
            let _ = if played {
                activation.activate(family)
            } else {
                activation.deactivate(family)
            };
        }
    }
    if let Some(time) = runtime.resources.get::<SharedTime>() {
        let scale = if play_mode == PlayMode::Paused {
            0.0
        } else {
            1.0
        };
        if let Ok(mut time) = time.write() {
            time.set_scale(scale);
        }
    }
}
