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

//! **The scenes the editor loads land at a later boundary.** Opening a
//! scene and Stop's restore are filed to the engine's scene load hub,
//! prepared off the frame and committed at a frame's boundary; the editor
//! finishes each once its outcome landed. The engine's side of a frame is
//! played here by hand, around the editor's own per-frame calls: the
//! boundary's `scene_intake`, the `scene_io` family's run, then
//! `land_scene_loads` and `process_transport`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::RwLock;

use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::Activation;
use khora_sdk::prelude::{SharedTime, Time};
use khora_sdk::{Runtime, SceneLoadHub, SerializationGoal};

use super::super::PLAY_FAMILIES;
use super::*;
use crate::tests::scene_loads::{engine_frame, install, SETTLE_FRAMES};

/// A runtime as the engine holds it for the editor: its activation, knowing
/// the play families; its clock; the reserve the background stages into;
/// and the scene load hub.
struct Engine {
    runtime: Runtime,
    activation: Arc<Activation>,
}

fn engine() -> Engine {
    let activation = Arc::new(Activation::new(PLAY_FAMILIES));
    let time: SharedTime = Arc::new(RwLock::new(Time::default()));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    runtime.resources.insert(time);
    install(&mut runtime);
    Engine {
        runtime,
        activation,
    }
}

impl Engine {
    /// The world the engine runs: empty, and holding its hub.
    fn world(&self) -> GameWorld {
        crate::tests::scene_loads::engine_world(&self.runtime)
    }

    fn hub(&self) -> Arc<SceneLoadHub> {
        crate::tests::scene_loads::hub(&self.runtime)
    }

    /// The families off once the frame boundary settles what was asked.
    fn off_at_the_boundary(&self) -> BTreeSet<AgentId> {
        self.activation.settle();
        self.activation.inactive()
    }

    /// One whole frame around the editor: the boundary commits what landed,
    /// the background prepares what was filed, then the editor finishes its
    /// landed loads and processes its transport, as `before_frame` does.
    fn frame(&self, world: &mut GameWorld, state: &Arc<Mutex<EditorState>>) {
        engine_frame(&self.runtime, world);
        land_scene_loads(world, state, &self.hub());
        process_transport(world, state, &self.runtime);
    }
}

fn ask(state: &Arc<Mutex<EditorState>>, action: &str) {
    state.lock().expect("the editor state").pending_menu_action = Some(action.to_owned());
}

fn play_mode(state: &Arc<Mutex<EditorState>>) -> PlayMode {
    state.lock().expect("the editor state").play_mode
}

fn restoring(state: &Arc<Mutex<EditorState>>) -> bool {
    state.lock().expect("the editor state").restoring.is_some()
}

fn play_families() -> BTreeSet<AgentId> {
    PLAY_FAMILIES.into_iter().collect()
}

/// **Stop restores the scene at a later boundary.** The mode goes to
/// Editing on the frame Stop is pressed, and the play families — the
/// scripts among them — are off from that frame on. The restore is filed
/// and in flight: the world is still the played one, and Play is refused
/// meanwhile. Once its outcome lands, the world is the scene as Play found
/// it, and the refused Play never fires later.
#[test]
fn stop_restores_the_scene_at_a_later_boundary() {
    let engine = engine();
    start_editing(&engine.runtime);
    let mut world = engine.world();
    let root = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        GlobalTransform::identity(),
        Name::new("Root"),
    ));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    let before = observed(&world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    assert_eq!(play_mode(&state), PlayMode::Playing, "the premise: playing");
    world.spawn((Transform::identity(), Name::new("Spawned in play")));
    let played = world.iter_entities().count();

    ask(&state, "stop");
    process_transport(&mut world, &state, &engine.runtime);

    assert_eq!(play_mode(&state), PlayMode::Editing, "Editing at once");
    assert_eq!(
        engine.off_at_the_boundary(),
        play_families(),
        "the scripts are off from this frame on"
    );
    assert!(restoring(&state), "the restore is in flight");
    assert_eq!(
        world.iter_entities().count(),
        played,
        "the world is restored only once the restore lands"
    );

    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    assert_eq!(
        play_mode(&state),
        PlayMode::Editing,
        "Play is refused while restoring"
    );
    assert!(
        state
            .lock()
            .expect("the editor state")
            .scene_snapshot
            .is_none(),
        "a refused Play takes no snapshot"
    );

    for _ in 0..SETTLE_FRAMES {
        engine.frame(&mut world, &state);
        if !restoring(&state) {
            break;
        }
    }

    assert!(!restoring(&state), "the restore landed");
    assert_eq!(observed(&world), before, "the scene as Play found it");
    assert_eq!(
        play_mode(&state),
        PlayMode::Editing,
        "the Play refused while restoring never fires"
    );
    assert_eq!(engine.off_at_the_boundary(), play_families());
}

/// A folder on disk, removed when the test ends.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// **Opening a scene lands its problems after the commit.** A scene whose
/// camera's near plane sits at its eye is opened: nothing changes on the
/// frame it is opened. Once it lands, the world is the scene, what the
/// editor selected is forgotten, the scene's path is the open one, and the
/// load's problems name the camera the load brought — a live entity of the
/// new world.
#[test]
fn opening_a_scene_lands_its_problems_after_the_commit() {
    let dir = std::env::temp_dir().join(format!(
        "khora-editor-scene-opened-off-the-frame-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    let folder = Folder(dir);
    let path = folder.0.join("broken.kscene");
    {
        let mut authored = GameWorld::new();
        let camera = authored.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            Camera {
                z_near: 0.0,
                ..Camera::default_perspective()
            },
            Name::new("Main Camera"),
        ));
        authored
            .inner_world_mut()
            .mark_authored(camera)
            .expect("authored");
        assert!(
            scene_io::save_scene_to_path_with_goal(
                &authored,
                &path.to_string_lossy(),
                SerializationGoal::EditorInterchange,
            ),
            "the scene saves"
        );
    }
    let engine = engine();
    start_editing(&engine.runtime);
    let mut world = engine.world();
    let old = world.spawn((Transform::identity(), Name::new("Old")));
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .selection
        .insert(old);

    open_scene_dispatch(None, &mut world, &state, &engine.hub(), &path);

    assert!(
        world.get_component::<Name>(old).is_some(),
        "nothing lands on the frame it is opened"
    );
    assert!(
        state
            .lock()
            .expect("the editor state")
            .pending_load_problems
            .is_none(),
        "no problem before the load lands"
    );

    let mut problems = None;
    for _ in 0..SETTLE_FRAMES {
        engine.frame(&mut world, &state);
        problems = state
            .lock()
            .expect("the editor state")
            .pending_load_problems
            .take();
        if problems.is_some() {
            break;
        }
    }

    let problems = problems.expect("the load's problems are handed over once it lands");
    assert!(
        !world.inner_world().contains(old),
        "the scene replaced the world"
    );
    let camera_problems: Vec<_> = problems
        .iter()
        .filter(|problem| problem.component.as_deref() == Some("Camera"))
        .collect();
    assert!(
        !camera_problems.is_empty(),
        "the load reports the camera: {problems:#?}"
    );
    for problem in camera_problems {
        let entity = problem
            .entity
            .unwrap_or_else(|| panic!("the camera's problem names no entity: {problem:#?}"));
        assert!(
            world.get_component::<Camera>(entity).is_some(),
            "the problem names the camera the load brought: {problem:#?}"
        );
    }
    let state = state.lock().expect("the editor state");
    assert!(state.selection.is_empty(), "what was selected is forgotten");
    assert_eq!(
        state.current_scene_path.as_deref(),
        Some(path.to_string_lossy().as_ref()),
        "the opened scene is the open one"
    );
}

/// The names of `world`'s entities.
fn names_of(world: &GameWorld) -> BTreeSet<String> {
    world
        .iter_entities()
        .filter_map(|entity| world.get_component::<Name>(entity))
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// **A new scene made while a scene is opening stays the new scene.** A
/// scene is opened, and before it lands the user makes a new scene: the
/// later action has the last word — the opening lands nothing over it, and
/// the new scene is not saved under the opened scene's path.
#[test]
fn a_new_scene_made_while_a_scene_opens_is_not_replaced_by_it() {
    let dir = std::env::temp_dir().join(format!(
        "khora-editor-new-scene-while-opening-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    let folder = Folder(dir);
    let path = folder.0.join("level.kscene");
    {
        let mut authored = GameWorld::new();
        let door = authored.spawn((Transform::identity(), Name::new("Door")));
        authored
            .inner_world_mut()
            .mark_authored(door)
            .expect("authored");
        assert!(
            scene_io::save_scene_to_path_with_goal(
                &authored,
                &path.to_string_lossy(),
                SerializationGoal::EditorInterchange,
            ),
            "the scene saves"
        );
    }
    let engine = engine();
    start_editing(&engine.runtime);
    let mut world = engine.world();
    world.spawn((Transform::identity(), Name::new("Old")));
    let state = Arc::new(Mutex::new(EditorState::default()));

    open_scene_dispatch(None, &mut world, &state, &engine.hub(), &path);
    super::super::apply_new_scene(&mut world, &state);
    assert!(
        names_of(&world).is_empty(),
        "the premise: a new, empty scene"
    );

    for _ in 0..SETTLE_FRAMES {
        engine.frame(&mut world, &state);
    }

    assert!(
        names_of(&world).is_empty(),
        "the scene opened before the new one replaced it: {:?}",
        names_of(&world)
    );
    assert_ne!(
        state
            .lock()
            .expect("the editor state")
            .current_scene_path
            .as_deref(),
        Some(path.to_string_lossy().as_ref()),
        "the new scene would be saved over the scene opened before it"
    );
}

/// **An edit made between Stop and its restore is not silently lost.**
/// Stop is pressed — Editing at once — and before the restore lands the user
/// deletes the selected entity. Either the edit is refused while the world
/// is still the played one, or it holds once the restore has landed.
#[test]
fn an_edit_made_while_the_scene_is_restored_is_refused_or_kept() {
    let engine = engine();
    start_editing(&engine.runtime);
    let mut world = engine.world();
    let root = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Root"),
    ));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    let state = Arc::new(Mutex::new(EditorState::default()));
    ask(&state, "play");
    process_transport(&mut world, &state, &engine.runtime);
    ask(&state, "stop");
    process_transport(&mut world, &state, &engine.runtime);
    assert_eq!(play_mode(&state), PlayMode::Editing, "the premise: editing");
    assert!(restoring(&state), "the premise: the restore is in flight");

    state
        .lock()
        .expect("the editor state")
        .selection
        .insert(root);
    super::super::apply_delete(&mut world, &state);
    let deleted = !names_of(&world).contains("Root");

    for _ in 0..SETTLE_FRAMES {
        engine.frame(&mut world, &state);
        if !restoring(&state) {
            break;
        }
    }

    assert!(!restoring(&state), "the restore landed");
    if deleted {
        assert!(
            !names_of(&world).contains("Root"),
            "the delete made in Editing was accepted, then undone by the restore"
        );
    }
}

/// **A new scene made once an open is prepared stays the new scene.** The
/// frames run in the editor's order: the transport and the landed loads
/// before the update, the menus in the update, the boundary after. A scene
/// is opened and prepared by the background; the next update makes a new
/// scene; that frame's boundary must not land the opened scene over it.
#[test]
fn a_new_scene_made_after_an_open_was_prepared_is_not_replaced_by_it() {
    let dir = std::env::temp_dir().join(format!(
        "khora-editor-new-scene-after-open-prepared-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    let folder = Folder(dir);
    let path = folder.0.join("level.kscene");
    {
        let mut authored = GameWorld::new();
        let door = authored.spawn((Transform::identity(), Name::new("Door")));
        authored
            .inner_world_mut()
            .mark_authored(door)
            .expect("authored");
        assert!(
            scene_io::save_scene_to_path_with_goal(
                &authored,
                &path.to_string_lossy(),
                SerializationGoal::EditorInterchange,
            ),
            "the scene saves"
        );
    }
    let engine = engine();
    start_editing(&engine.runtime);
    let mut world = engine.world();
    world.spawn((Transform::identity(), Name::new("Old")));
    let state = Arc::new(Mutex::new(EditorState::default()));

    // Frame 1: the update opens the scene; the background prepares it.
    open_scene_dispatch(None, &mut world, &state, &engine.hub(), &path);
    engine_frame(&engine.runtime, &mut world);
    // Frame 2: before the update, the landed loads (none yet); the update
    // makes a new scene; then the boundary.
    land_scene_loads(&mut world, &state, &engine.hub());
    super::super::apply_new_scene(&mut world, &state);
    for _ in 0..SETTLE_FRAMES {
        engine_frame(&engine.runtime, &mut world);
        land_scene_loads(&mut world, &state, &engine.hub());
    }

    assert!(
        names_of(&world).is_empty(),
        "the scene opened before the new one replaced it: {:?}",
        names_of(&world)
    );
}
