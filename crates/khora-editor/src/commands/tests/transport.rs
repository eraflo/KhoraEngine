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

//! **Play, Pause and Stop, as the engine sees them.** Editing, the scene's
//! scripts, physics and sounds are off; Play snapshots the scene and switches
//! them on; Stop restores the scene and switches them off; Pause stops the
//! clock and switches nothing. While editing, the clock runs at 1.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use khora_sdk::khora_core::agent::Activation;
use khora_sdk::prelude::{SharedTime, Time};
use khora_sdk::{EditorState, GameWorld, PlayMode, Runtime};

use super::super::{process_transport, start_editing, PLAY_FAMILIES};
use khora_sdk::prelude::ecs::*;

/// A runtime holding the engine's activation — knowing the play families and
/// a family the editor never switches — and its clock.
struct Engine {
    runtime: Runtime,
    activation: Arc<Activation>,
    time: SharedTime,
}

/// A family the transport leaves alone.
const RENDERER: khora_sdk::khora_core::agent::gorna::AgentId =
    khora_sdk::khora_core::agent::gorna::AgentId::new("renderer");

fn engine() -> Engine {
    let activation = Arc::new(Activation::new(PLAY_FAMILIES.into_iter().chain([RENDERER])));
    let time: SharedTime = Arc::new(RwLock::new(Time::default()));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    runtime.resources.insert(Arc::clone(&time));
    Engine {
        runtime,
        activation,
        time,
    }
}

impl Engine {
    /// The families off once the frame boundary settles what was asked.
    fn off_at_the_boundary(&self) -> BTreeSet<khora_sdk::khora_core::agent::gorna::AgentId> {
        self.activation.settle();
        self.activation.inactive()
    }

    fn scale(&self) -> f32 {
        self.time.read().expect("the clock").scale()
    }
}

fn play_families() -> BTreeSet<khora_sdk::khora_core::agent::gorna::AgentId> {
    PLAY_FAMILIES.into_iter().collect()
}

/// Asks `action` of the transport, as its buttons do.
fn ask(state: &Arc<Mutex<EditorState>>, action: &str) {
    state.lock().expect("the editor state").pending_menu_action = Some(action.to_owned());
}

fn play_mode(state: &Arc<Mutex<EditorState>>) -> PlayMode {
    state.lock().expect("the editor state").play_mode
}

/// An editor opened over `engine`, its world holding one authored entity.
fn editing(engine: &Engine) -> (GameWorld, Arc<Mutex<EditorState>>) {
    start_editing(&engine.runtime);
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    (world, Arc::new(Mutex::new(EditorState::default())))
}

/// **The editor opens editing**: its setup switches the scene's scripts,
/// physics and sounds off, and nothing else, with the clock at 1.
#[test]
fn setup_switches_the_play_families_off() {
    let engine = engine();

    start_editing(&engine.runtime);

    assert_eq!(engine.off_at_the_boundary(), play_families());
    assert!(
        engine.activation.is_active(RENDERER),
        "the scene still draws"
    );
    assert_eq!(engine.scale(), 1.0, "editing keeps the clock at 1");
}

/// **Play snapshots the scene, then switches the play families on**, the
/// clock at 1.
#[test]
fn play_snapshots_the_scene_then_switches_the_play_families_on() {
    let engine = engine();
    let (mut world, state) = editing(&engine);
    assert_eq!(engine.off_at_the_boundary(), play_families());

    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);

    assert_eq!(play_mode(&state), PlayMode::Playing);
    assert!(
        state
            .lock()
            .expect("the editor state")
            .scene_snapshot
            .is_some(),
        "Play keeps a snapshot"
    );
    assert!(
        engine.off_at_the_boundary().is_empty(),
        "every play family is on"
    );
    assert_eq!(engine.scale(), 1.0);
}

/// **Stop restores the scene, then switches the play families off**, the
/// clock back at 1: what play spawned is gone.
#[test]
fn stop_restores_the_scene_then_switches_the_play_families_off() {
    let engine = engine();
    let (mut world, state) = editing(&engine);
    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    let authored = world.iter_entities().count();
    world.spawn((Transform::identity(), Name::new("Spawned in play")));

    ask(&state, "stop");
    process_transport(&mut world, &state, &engine.runtime);

    assert_eq!(play_mode(&state), PlayMode::Editing);
    assert_eq!(
        world.iter_entities().count(),
        authored,
        "the scene is restored"
    );
    assert_eq!(engine.off_at_the_boundary(), play_families());
    assert_eq!(engine.scale(), 1.0, "editing keeps the clock at 1");
}

/// **Pause stops the clock and nothing else**: the play families stay on;
/// Play again resumes the clock without a new snapshot.
#[test]
fn pause_stops_the_clock_and_switches_nothing() {
    let engine = engine();
    let (mut world, state) = editing(&engine);
    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    engine.off_at_the_boundary();
    let snapshot = state
        .lock()
        .expect("the editor state")
        .scene_snapshot
        .clone();

    ask(&state, "pause");
    process_transport(&mut world, &state, &engine.runtime);

    assert_eq!(play_mode(&state), PlayMode::Paused);
    assert_eq!(engine.scale(), 0.0, "paused: the clock stops");
    assert!(
        engine.off_at_the_boundary().is_empty(),
        "paused, the play families stay on"
    );

    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    assert_eq!(play_mode(&state), PlayMode::Playing);
    assert_eq!(engine.scale(), 1.0, "resumed");
    assert_eq!(
        state
            .lock()
            .expect("the editor state")
            .scene_snapshot
            .clone(),
        snapshot,
        "resuming keeps the snapshot Play took"
    );
}

/// **Only the transport is processed before the frame**: another action is
/// left for `update`.
#[test]
fn another_action_is_left_for_update() {
    let engine = engine();
    let (mut world, state) = editing(&engine);

    ask(&state, "save");
    process_transport(&mut world, &state, &engine.runtime);

    assert_eq!(
        state
            .lock()
            .expect("the editor state")
            .pending_menu_action
            .as_deref(),
        Some("save")
    );
    assert_eq!(play_mode(&state), PlayMode::Editing);
}
