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

//! The menu's requests, as `update` takes them from the editor's queue.

use khora_sdk::editor_ui::{EditorRequest, MenuAction, Transport};
use khora_sdk::trust::ProjectTrust;

use super::*;

/// Runs the menu's requests and the spawns they ask, as one `update` does.
fn run_menu(world: &mut GameWorld, state: &Arc<Mutex<EditorState>>) {
    let history = Arc::new(Mutex::new(CommandHistory::default()));
    process_menu_actions(&mut None, state, &history, world, ProjectTrust::Trusted);
    ops::process_spawns(world, &mut state.lock().expect("the editor state"));
}

/// Two menu actions asked in one frame both run: the second does not overwrite the first.
#[test]
fn two_menu_actions_in_one_frame_both_run() {
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    {
        let mut state = state.lock().expect("the editor state");
        state.push_request(EditorRequest::Menu(MenuAction::SpawnEmpty));
        state.push_request(EditorRequest::Menu(MenuAction::SpawnEmpty));
    }

    run_menu(&mut world, &state);

    let names: Vec<_> = (world.iter_entities())
        .filter_map(|e| {
            world
                .get_component::<Name>(e)
                .map(|n| n.as_str().to_owned())
        })
        .collect();
    assert_eq!(names, ["Empty", "Empty"]);
    assert!(state.lock().expect("the editor state").requests.is_empty());
}

/// The menu leaves a transport request queued for the next frame's `process_transport`.
#[test]
fn the_menu_leaves_the_transport_queued() {
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    {
        let mut state = state.lock().expect("the editor state");
        state.push_request(EditorRequest::Transport(Transport::Play));
        state.push_request(EditorRequest::Menu(MenuAction::SpawnEmpty));
    }

    run_menu(&mut world, &state);

    assert_eq!(world.iter_entities().count(), 1, "the spawn ran");
    assert_eq!(
        state.lock().expect("the editor state").requests,
        [EditorRequest::Transport(Transport::Play)]
    );
}
