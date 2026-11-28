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

//! **A scene is saved in its project, and only there**: a Save As inside the
//! project's assets goes through the project, however the path names its
//! folder; one outside, with no project open, or a save of another project's
//! scene writes nothing. **A scene is saved only as a scene file**: a name
//! with no extension takes `.kscene`, any other kind's file is never written.

use std::path::PathBuf;

use khora_sdk::MetricsRegistry;

use super::*;

/// A project on disk, removed when the test ends.
struct Project {
    root: PathBuf,
    vfs: Arc<Mutex<ProjectVfs>>,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A project for the test `name`, with nothing in its `assets/`.
fn project(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("assets").join("scenes")).expect("the scenes folder");
    let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    Project {
        root,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

/// **A scene saved as at a trashed scene's path, the project's folder named in
/// another case, gets a new identity.**
///
/// Where file names ignore case, the file picked is inside the project
/// however its folder is spelled: the save goes through the project, so the
/// identity the trashed `scenes/b.kscene` left there is not the new scene's.
// Pins a project folder named in another case, where file names ignore it.
#[cfg(windows)]
#[test]
fn a_scene_saved_as_in_the_project_named_in_another_case_gets_a_new_identity() {
    let project = project("save-as-other-case");
    let moved = khora_sdk::khora_core::asset::AssetUUID::new_v5("scenes/a.kscene");
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        pvfs.create_asset("scenes/a.kscene", b"first")
            .expect("a scene is made");
        pvfs.rename_asset("scenes/a.kscene", "scenes/b.kscene")
            .expect("it is renamed");
        // What a trash leaves: the file gone, the registry untouched.
        std::fs::remove_file(project.root.join("assets").join("scenes").join("b.kscene"))
            .expect("it is trashed");
    }
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    let picked = PathBuf::from(project.root.to_string_lossy().to_uppercase())
        .join("assets")
        .join("scenes")
        .join("b.kscene");

    assert!(
        save_scene_dispatch(Some(&project.vfs), &world, &picked.to_string_lossy()),
        "the scene is saved"
    );

    let mut pvfs = project.vfs.lock().expect("the project");
    pvfs.rebuild_index().expect("the project indexes");
    assert_ne!(
        pvfs.resolve_uuid("scenes/b.kscene"),
        moved,
        "the new scene took the identity of the scene trashed at its path"
    );
}

/// The editor's state with the scene at `key` of `project` open, as a scene opened from it leaves it.
fn editing_at(project: &Project, key: &str) -> Arc<Mutex<EditorState>> {
    Arc::new(Mutex::new(EditorState {
        current_scene_path: Some(scene_of(project, key)),
        ..EditorState::default()
    }))
}

/// The scene at `key` of `project`.
fn scene_of(project: &Project, key: &str) -> SceneInProject {
    SceneInProject::new(project.root.join("assets"), key)
}

/// The scene the editor's `state` saves to.
fn scene_path(state: &Arc<Mutex<EditorState>>) -> Option<SceneInProject> {
    state
        .lock()
        .expect("the editor state")
        .current_scene_path
        .clone()
}

/// **A Save As the project refuses keeps the scene's path**: nothing landed, so nothing of the editor's state changes.
#[test]
fn a_save_as_refused_under_the_assets_keeps_the_scenes_path_and_reports_nothing() {
    let project = project("save-as-refused-keeps-path");
    let scenes = project.root.join("assets").join("scenes");
    let state = editing_at(&project, "scenes/level.kscene");
    let before = scene_path(&state);
    let world = GameWorld::new();
    let refused = scenes.join("level.kscene.");

    let landed = save_scene_as(Some(&project.vfs), &world, &state, &refused);

    assert!(!landed, "a refused path is said saved");
    assert_eq!(scene_path(&state), before, "the scene's path moved");
    assert_eq!(take_save_problems(&state), None, "a save problem is filed");
}

/// **A Save As whose write fails keeps the scene's path.**
///
/// A folder stands where the scene file goes: the save lands nowhere.
#[test]
fn a_save_as_that_fails_to_write_keeps_the_scenes_path() {
    let project = project("save-as-failed-keeps-path");
    let state = editing_at(&project, "scenes/a.kscene");
    let before = scene_path(&state);
    let in_the_way = project
        .root
        .join("assets")
        .join("scenes")
        .join("level.kscene");
    std::fs::create_dir(&in_the_way).expect("the folder in the way");
    let world = GameWorld::new();

    let landed = save_scene_as(Some(&project.vfs), &world, &state, &in_the_way);

    assert!(!landed, "a failed write is said saved");
    assert_eq!(scene_path(&state), before, "the scene's path moved");
    assert_eq!(take_save_problems(&state), None, "a save problem is filed");
}

/// **A Save As that lands makes the file picked the scene's path.**
#[test]
fn a_save_as_that_lands_makes_the_file_picked_the_scenes_path() {
    let project = project("save-as-lands");
    let scenes = project.root.join("assets").join("scenes");
    let state = editing_at(&project, "scenes/a.kscene");
    let world = GameWorld::new();
    let picked = scenes.join("b.kscene");

    assert!(save_scene_as(Some(&project.vfs), &world, &state, &picked));

    assert_eq!(
        scene_path(&state),
        Some(scene_of(&project, "scenes/b.kscene"))
    );
    assert_eq!(take_save_problems(&state), Some(Vec::new()));
}

/// **A Save As outside the project's assets writes nothing**: a scene is saved in its project, and keeps the file it had.
#[test]
fn a_save_as_outside_the_projects_assets_writes_nothing() {
    let project = project("save-as-outside-refused");
    let state = editing_at(&project, "scenes/a.kscene");
    let before = scene_path(&state);
    let world = GameWorld::new();
    let outside = project.root.join("level.kscene");

    let landed = save_scene_as(Some(&project.vfs), &world, &state, &outside);

    assert!(!landed, "a scene outside the project is said saved");
    assert!(!outside.exists(), "a scene was written outside the project");
    assert_eq!(scene_path(&state), before, "the scene's path moved");
    assert_eq!(take_save_problems(&state), None, "a save problem is filed");
}

/// **A Save As with no project open writes nothing.**
#[test]
fn a_save_as_with_no_project_open_writes_nothing() {
    let project = project("save-as-no-project");
    let state = Arc::new(Mutex::new(EditorState::default()));
    let picked = project.root.join("assets").join("scenes").join("a.kscene");

    let landed = save_scene_as(None, &GameWorld::new(), &state, &picked);

    assert!(!landed, "a scene is said saved with no project open");
    assert!(!picked.exists(), "a scene was written with no project open");
    assert_eq!(scene_path(&state), None);
}

/// **A save of a scene of another project writes nothing**: the open scene names its project, and the project open is another.
#[test]
fn a_save_of_a_scene_of_another_project_writes_nothing() {
    let open = project("save-other-project-open");
    let other = project("save-other-project-named");
    let state = editing_at(&other, "scenes/level.kscene");
    let mut world = GameWorld::new();

    apply_save(Some(&open.vfs), &mut world, &state);

    assert!(
        !other
            .root
            .join("assets")
            .join("scenes")
            .join("level.kscene")
            .exists(),
        "the other project's scene was written"
    );
    assert!(
        !open
            .root
            .join("assets")
            .join("scenes")
            .join("level.kscene")
            .exists(),
        "the scene was written into the project open under the other's key"
    );
    assert_eq!(take_save_problems(&state), None, "a save problem is filed");
}

/// **A Save As of a name with no extension saves the `.kscene` of that name**, as a file dialog adds it; a name of no scene is refused, and the scene keeps its path.
#[test]
fn a_save_as_names_a_scene_file_by_its_extension() {
    let project = project("save-as-extension");
    let scenes = project.root.join("assets").join("scenes");
    let state = editing_at(&project, "scenes/a.kscene");
    let world = GameWorld::new();

    assert!(save_scene_as(
        Some(&project.vfs),
        &world,
        &state,
        &scenes.join("forest")
    ));
    assert!(
        scenes.join("forest.kscene").is_file(),
        "the scene file is not written"
    );
    assert!(
        !scenes.join("forest").exists(),
        "a file of no kind was written"
    );
    assert_eq!(
        scene_path(&state),
        Some(scene_of(&project, "scenes/forest.kscene"))
    );

    let before = scene_path(&state);
    for picked in ["forest.txt", "forest.kprefab"] {
        let landed = save_scene_as(Some(&project.vfs), &world, &state, &scenes.join(picked));
        assert!(!landed, "{picked} is said saved");
        assert!(!scenes.join(picked).exists(), "{picked} was written");
        assert_eq!(
            scene_path(&state),
            before,
            "the scene's path moved to {picked}"
        );
    }
}

/// **A Save As onto a texture never overwrites it**: the texture stays as it was, and the scene keeps its path.
#[test]
fn a_save_as_onto_a_texture_never_overwrites_it() {
    let project = project("save-as-onto-texture");
    let textures = project.root.join("assets").join("textures");
    std::fs::create_dir_all(&textures).expect("the textures folder");
    let wall = textures.join("wall.png");
    std::fs::write(&wall, b"PNG").expect("the texture is written");
    let state = editing_at(&project, "scenes/a.kscene");
    let before = scene_path(&state);

    let landed = save_scene_as(Some(&project.vfs), &GameWorld::new(), &state, &wall);

    assert!(!landed, "a scene is said saved over a texture");
    assert_eq!(
        std::fs::read(&wall).expect("the texture is kept"),
        b"PNG",
        "the texture was written over"
    );
    assert_eq!(scene_path(&state), before, "the scene's path is a texture");
    assert_eq!(take_save_problems(&state), None, "a save problem is filed");
}
