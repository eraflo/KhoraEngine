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

//! Play, Pause and Stop — the transport — as the engine sees them.
//!
//! The engine has no mode: Play and Stop are the editor's. Editing, the
//! families a scene runs only while it is played — its scripts, its physics,
//! its sounds — are off; Play snapshots the scene and switches them on, Stop
//! restores the scene and switches them off, Pause stops the clock and
//! nothing else. Processed in `before_frame`, before the engine fixes the
//! frame, so a frame is wholly edited or wholly played.

use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::Activation;
use khora_sdk::prelude::SharedTime;
use khora_sdk::{EditorState, GameWorld, PlayMode, Runtime};

use crate::ops;

/// The families a scene runs only while it is played.
pub const PLAY_FAMILIES: [AgentId; 3] = [
    khora_sdk::khora_core::script::SCRIPT,
    khora_sdk::khora_core::physics::PHYSICS,
    khora_sdk::khora_core::audio::AUDIO,
];

/// What the editor's `setup` does to the engine: it opens editing, the
/// [`PLAY_FAMILIES`] off.
pub fn start_editing(runtime: &Runtime) {
    follow(runtime, PlayMode::Editing);
}

/// Whether `action` is one of the transport's.
pub(crate) fn is_transport(action: &str) -> bool {
    matches!(action, "play" | "pause" | "stop")
}

/// Processes the transport action the editor's state holds — `"play"`,
/// `"pause"` or `"stop"` — and leaves any other action for `update`. Play
/// snapshots the scene, then switches the [`PLAY_FAMILIES`] on, at a time
/// scale of 1; Stop restores the scene, then switches them off, the scale
/// back at 1; Pause sets the scale to 0 and switches nothing. The scene
/// cameras follow.
///
/// Whatever else moved the transport — a new scene, an opened one — the
/// families and the clock follow where it stands, every frame. Returns
/// whether Stop restored the scene: the world was replaced, and what named
/// its entities names nothing now.
pub fn process_transport(
    world: &mut GameWorld,
    editor_state: &Arc<Mutex<EditorState>>,
    runtime: &Runtime,
) -> bool {
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
    let mut restored = false;
    if let Some(action) = action {
        match action.as_str() {
            "play" => super::apply_play(world, editor_state),
            "pause" => super::apply_pause(editor_state),
            _ => {
                restored = play_mode(editor_state) != PlayMode::Editing;
                super::apply_stop(world, editor_state);
            }
        }
        ops::sync_scene_cameras_for_mode(world, play_mode(editor_state));
    }
    follow(runtime, play_mode(editor_state));
    restored
}

fn play_mode(editor_state: &Arc<Mutex<EditorState>>) -> PlayMode {
    editor_state
        .lock()
        .map(|state| state.play_mode)
        .unwrap_or(PlayMode::Editing)
}

/// Asks the engine for what `play_mode` runs: the [`PLAY_FAMILIES`] on while
/// the scene is played or paused, off while it is edited; the clock stopped
/// only when paused. Requests only — the frame boundary settles them.
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
