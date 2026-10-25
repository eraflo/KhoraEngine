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

//! **Loading a scene returns what the load adapted**, the report it used to
//! only log, so the editor can list it in Problems.

use std::path::PathBuf;
use std::sync::Arc;

use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_data::scene::record::{LoadReport, ReportKind};
use khora_sdk::MetricsRegistry;

use super::super::*;

const SCENE: &str = "scenes/broken.kscene";

/// A project folder on disk, removed when the test ends.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(name: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("an old folder is removed");
    }
    std::fs::create_dir_all(dir.join("assets").join("scenes")).expect("the scene folder");
    Folder(dir)
}

/// A world holding one authored camera whose near plane sits at its eye,
/// and that camera's persistent id.
fn broken_camera_world() -> (GameWorld, PersistentId) {
    let mut world = GameWorld::new();
    let camera = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Camera {
            z_near: 0.0,
            ..Camera::default_perspective()
        },
        Name::new("Main Camera"),
    ));
    let id = world
        .inner_world_mut()
        .mark_authored(camera)
        .expect("the camera is authored");
    (world, id)
}

/// The report's one `Unusable` entry is the camera's near plane.
fn assert_reports_the_near_plane(report: &LoadReport, id: PersistentId) {
    let unusable: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| matches!(entry.kind, ReportKind::Unusable { .. }))
        .collect();
    assert_eq!(unusable.len(), 1, "{report:#?}");
    assert_eq!(unusable[0].path, "z_near");
    assert_eq!(unusable[0].component.as_deref(), Some("Camera"));
    assert_eq!(unusable[0].entity, Some(id));
}

/// The near plane of the one camera in `world`.
fn near_of(world: &GameWorld) -> f32 {
    let camera = world
        .iter_entities()
        .find_map(|entity| world.get_component::<Camera>(entity).copied())
        .expect("the camera loaded");
    camera.z_near
}

#[test]
fn loading_a_scene_returns_its_report() {
    let dir = folder("load-returns-report");
    let mut pvfs = ProjectVfs::open(dir.0.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    let (world, id) = broken_camera_world();
    assert!(
        save_scene_in_project(&mut pvfs, &world, Path::new(SCENE)),
        "the scene saves"
    );

    let mut opened = GameWorld::new();
    let report =
        load_scene_in_project_with_report(&mut pvfs, &mut opened, SCENE).expect("the scene loads");

    assert_reports_the_near_plane(&report, id);
    assert_eq!(near_of(&opened), 0.0, "the value is kept as written");
}

#[test]
fn loading_a_scene_from_outside_the_project_returns_its_report() {
    let dir = folder("load-path-returns-report");
    let path = dir.0.join("outside.kscene");
    let path = path.to_string_lossy().into_owned();
    let (world, id) = broken_camera_world();
    assert!(
        save_scene_to_path_with_goal(&world, &path, SerializationGoal::EditorInterchange),
        "the scene saves"
    );

    let mut opened = GameWorld::new();
    let report = load_scene_from_path_with_report(&mut opened, &path).expect("the scene loads");

    assert_reports_the_near_plane(&report, id);
    assert_eq!(near_of(&opened), 0.0, "the value is kept as written");
}

/// A scene that cannot be read returns no report, and leaves the world as
/// it was.
#[test]
fn a_scene_that_does_not_load_returns_no_report() {
    let dir = folder("load-missing-no-report");
    let missing = dir.0.join("missing.kscene");
    let mut world = GameWorld::new();
    world.spawn((Transform::identity(), Name::new("Kept")));

    let report = load_scene_from_path_with_report(&mut world, &missing.to_string_lossy());

    assert_eq!(report, None);
    assert_eq!(world.iter_entities().count(), 1, "the world is untouched");
}
