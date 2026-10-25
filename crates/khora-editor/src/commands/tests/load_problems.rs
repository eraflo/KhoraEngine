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

//! **What a scene load adapted reaches Problems through the editor's own
//! commands**: a load hands its problems over, the next load replaces them,
//! a new scene clears them — and after Play and Stop, which respawns every
//! entity under a fresh id, a load problem's row still selects the entity it
//! is about, not a slot that no longer holds anything.

use std::path::PathBuf;

use khora_sdk::khora_core::problems::{ProblemSource, Problems};
use khora_sdk::SerializationGoal;

use super::*;

/// A folder on disk, removed when the test ends.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Hands the load's problems to the engine's list, as the editor app does
/// at the start of each frame.
fn mirror(problems: &Problems, state: &Arc<Mutex<EditorState>>) {
    let pending = state
        .lock()
        .expect("the editor state")
        .pending_load_problems
        .take();
    if let Some(load) = pending {
        problems.report(ProblemSource::Load, load);
    }
}

/// A fresh folder for the test `name`.
fn folder(name: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    Folder(dir)
}

/// Saves, at `path`, a scene with one camera whose near plane is `z_near`.
fn save_camera_scene(path: &std::path::Path, z_near: f32) {
    let mut authored = GameWorld::new();
    let camera = authored.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Camera {
            z_near,
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

/// A clean scene opened after a broken one clears the broken one's load
/// problems; so does a new scene.
#[test]
fn a_clean_load_or_a_new_scene_clears_the_load_problems() {
    let dir = folder("load-problems-cleared");
    let broken = dir.0.join("broken.kscene");
    let clean = dir.0.join("clean.kscene");
    save_camera_scene(&broken, 0.0);
    save_camera_scene(&clean, 0.1);
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    let problems = Problems::default();

    load_scene_dispatch(None, &mut world, &state, &broken);
    mirror(&problems, &state);
    assert!(!problems.of_source(&ProblemSource::Load).is_empty());

    load_scene_dispatch(None, &mut world, &state, &clean);
    mirror(&problems, &state);
    assert!(
        problems.of_source(&ProblemSource::Load).is_empty(),
        "the clean scene replaced the list"
    );

    load_scene_dispatch(None, &mut world, &state, &broken);
    mirror(&problems, &state);
    assert!(!problems.of_source(&ProblemSource::Load).is_empty());
    apply_new_scene(&mut world, &state);
    mirror(&problems, &state);
    assert!(
        problems.of_source(&ProblemSource::Load).is_empty(),
        "a new scene has nothing a load adapted"
    );
}

#[test]
fn a_load_problem_still_selects_its_camera_after_stop() {
    let dir = folder("load-problems-after-stop");
    let path = dir.0.join("broken.kscene");
    // A scene whose camera's near plane sits at its eye.
    save_camera_scene(&path, 0.0);

    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    let problems = Problems::default();
    load_scene_dispatch(None, &mut world, &state, &path);
    mirror(&problems, &state);
    let before = problems.of_source(&ProblemSource::Load);
    assert!(
        before.iter().any(|p| p
            .entity
            .is_some_and(|e| world.get_component::<Camera>(e).is_some())),
        "the load reports the camera, on the loaded entity: {before:#?}"
    );

    apply_play(&mut world, &state);
    mirror(&problems, &state);
    apply_stop(&mut world, &state);
    mirror(&problems, &state);

    let after = problems.of_source(&ProblemSource::Load);
    for problem in after
        .iter()
        .filter(|p| p.component.as_deref() == Some("Camera"))
    {
        let entity = problem
            .entity
            .unwrap_or_else(|| panic!("the camera's load problem lost its entity: {problem:#?}"));
        assert!(
            world.get_component::<Camera>(entity).is_some(),
            "the row selects the restored camera, not a dead id: {problem:#?}"
        );
    }
}
