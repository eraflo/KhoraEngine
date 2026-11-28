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

//! File operations the asset browser asked for, run on a project on disk.
//!
//! A script asked for appears, the browser lists it and is asked to reveal
//! it, and an existing file is never written over. Operations asked together
//! run in the order asked.

use std::path::PathBuf;

use khora_sdk::editor_ui::AssetFileOp;
use khora_sdk::MetricsRegistry;

use super::super::*;
use crate::tests::requests::{file_ops, reveals};

const PLAYER: &str = "scripts/Player.erg";
const SOURCE: &str = "behavior Player { }";

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

impl Project {
    fn player(&self) -> PathBuf {
        self.root.join("assets").join("scripts").join("Player.erg")
    }
}

/// A project named `name`, holding `files` (forward-slash paths under
/// `assets/`, with their bytes).
fn project(name: &str, files: &[(&str, &[u8])]) -> Project {
    let root = std::env::temp_dir().join(format!(
        "khora-editor-asset-file-ops-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("an old project is removed");
    }
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets).expect("the assets folder");
    for (rel, bytes) in files {
        let path = rel
            .split('/')
            .fold(assets.clone(), |at, part| at.join(part));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's folder");
        }
        std::fs::write(&path, bytes).expect("the file is written");
    }
    let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    Project {
        root,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

/// An editor over `project`, a script asked for at `PLAYER`.
fn asking_for_the_player(project: &Project) -> Arc<Mutex<EditorState>> {
    let mut state = EditorState {
        project_folder: Some(project.root.to_string_lossy().to_string()),
        asset_epoch: 1,
        ..EditorState::default()
    };
    state.push_request(EditorRequest::AssetFile(AssetFileOp::CreateScript {
        path: PLAYER.to_owned(),
        source: SOURCE.to_owned(),
    }));
    Arc::new(Mutex::new(state))
}

#[test]
fn a_queued_script_is_written_listed_and_revealed() {
    let project = project("written", &[]);
    let state = asking_for_the_player(&project);

    run_queued_asset_file_ops(Some(&project.vfs), &state);

    assert_eq!(
        std::fs::read(project.player()).ok().as_deref(),
        Some(SOURCE.as_bytes()),
        "the script is written with its source"
    );
    let state = state.lock().expect("state");
    assert!(file_ops(&state).is_empty(), "the request is consumed");
    assert!(state.asset_epoch > 1, "the browser is told to rescan");
    assert!(
        state
            .asset_entries
            .iter()
            .any(|entry| entry.source_path == PLAYER),
        "the browser's list holds the script: {:?}",
        state.asset_entries
    );
    assert_eq!(reveals(&state), [PLAYER]);
}

#[test]
fn a_queued_script_never_writes_over_a_file_and_reveals_nothing() {
    let project = project("existing", &[(PLAYER, b"what the author wrote")]);
    let state = asking_for_the_player(&project);

    run_queued_asset_file_ops(Some(&project.vfs), &state);

    assert_eq!(
        std::fs::read(project.player()).ok().as_deref(),
        Some(&b"what the author wrote"[..]),
        "the author's file is untouched"
    );
    let state = state.lock().expect("state");
    assert!(file_ops(&state).is_empty(), "the request is consumed");
    assert!(reveals(&state).is_empty());
}

#[test]
fn a_queued_script_with_no_project_open_writes_nothing() {
    let project = project("closed", &[]);
    let state = asking_for_the_player(&project);

    run_queued_asset_file_ops(None, &state);

    assert!(!project.player().exists(), "a script was written");
    let state = state.lock().expect("state");
    assert!(file_ops(&state).is_empty(), "the request is consumed");
    assert!(reveals(&state).is_empty());
}

/// Two scripts asked in one frame are both written and both revealed, in the order asked.
#[test]
fn two_scripts_asked_in_one_frame_are_both_written_and_revealed() {
    const ENEMY: &str = "scripts/Enemy.erg";
    let project = project("two", &[]);
    let state = asking_for_the_player(&project);
    state
        .lock()
        .expect("state")
        .push_request(EditorRequest::AssetFile(AssetFileOp::CreateScript {
            path: ENEMY.to_owned(),
            source: "behavior Enemy { }".to_owned(),
        }));

    run_queued_asset_file_ops(Some(&project.vfs), &state);

    assert!(project.player().exists(), "the first script is written");
    assert!(
        project
            .root
            .join("assets")
            .join("scripts")
            .join("Enemy.erg")
            .exists(),
        "the second script is written"
    );
    let state = state.lock().expect("state");
    assert!(file_ops(&state).is_empty(), "both requests are consumed");
    assert_eq!(reveals(&state), [PLAYER, ENEMY]);
}

/// Operations asked in one frame run in the order asked: a file moved into a
/// folder, then renamed there, ends renamed in that folder.
#[test]
fn file_operations_asked_in_one_frame_run_in_the_order_asked() {
    let project = project("in-order", &[("notes.txt", b"the notes")]);
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().to_string()),
        ..EditorState::default()
    }));
    {
        let mut state = state.lock().expect("state");
        state.push_request(EditorRequest::AssetFile(AssetFileOp::Move {
            path: "notes.txt".to_owned(),
            into: "docs".to_owned(),
        }));
        state.push_request(EditorRequest::AssetFile(AssetFileOp::Rename {
            from: "docs/notes.txt".to_owned(),
            to: "docs/readme.txt".to_owned(),
        }));
    }

    run_queued_asset_file_ops(Some(&project.vfs), &state);

    let assets = project.root.join("assets");
    assert_eq!(
        std::fs::read(assets.join("docs").join("readme.txt"))
            .ok()
            .as_deref(),
        Some(&b"the notes"[..]),
        "moved, then renamed"
    );
    assert!(!assets.join("notes.txt").exists(), "nothing is left behind");
    assert!(
        !assets.join("docs").join("notes.txt").exists(),
        "the rename ran after the move"
    );
    assert!(file_ops(&state.lock().expect("state")).is_empty());
}

/// **The open scene follows its file through a rename**: a save after the browser renamed it writes the renamed file, and brings back no file at the old name.
#[test]
fn a_save_after_renaming_the_open_scene_writes_the_renamed_file() {
    const BEFORE: &[u8] = b"the scene as last saved";
    let project = project("rename-open-scene", &[("scenes/level.kscene", BEFORE)]);
    let assets = project.root.join("assets");
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().to_string()),
        current_scene_path: Some(SceneInProject::new(&assets, "scenes/level.kscene")),
        ..EditorState::default()
    }));
    state
        .lock()
        .expect("state")
        .push_request(EditorRequest::AssetFile(AssetFileOp::Rename {
            from: "scenes/level.kscene".to_owned(),
            to: "scenes/renamed.kscene".to_owned(),
        }));
    run_queued_asset_file_ops(Some(&project.vfs), &state);
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");

    apply_save(Some(&project.vfs), &mut world, &state);

    let scenes = assets.join("scenes");
    assert!(
        !scenes.join("level.kscene").exists(),
        "the save brought the scene back under its old name"
    );
    assert_ne!(
        std::fs::read(scenes.join("renamed.kscene")).ok().as_deref(),
        Some(BEFORE),
        "the renamed scene is not the one saved"
    );
}

/// An editor over `project` whose open scene, also the asset inspected, is its `scenes/level.kscene`.
fn editing_the_level(project: &Project) -> Arc<Mutex<EditorState>> {
    let assets = project.root.join("assets");
    Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().to_string()),
        current_scene_path: Some(SceneInProject::new(&assets, "scenes/level.kscene")),
        inspected_asset_path: Some("scenes/level.kscene".to_owned()),
        ..EditorState::default()
    }))
}

/// Saves a world of one authored entity as `state`'s open scene, as Ctrl+S does.
fn save_a_world(project: &Project, state: &Arc<Mutex<EditorState>>) {
    let mut world = GameWorld::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world
        .inner_world_mut()
        .mark_authored(root)
        .expect("authored");
    apply_save(Some(&project.vfs), &mut world, state);
}

/// **The open scene follows its file into another folder**: a save after the browser moved it writes the moved file, the old one is not made again, and the asset inspected is the moved file.
#[test]
fn a_save_after_moving_the_open_scene_writes_the_moved_file() {
    const BEFORE: &[u8] = b"the scene as last saved";
    let project = project("move-open-scene", &[("scenes/level.kscene", BEFORE)]);
    let state = editing_the_level(&project);
    state
        .lock()
        .expect("state")
        .push_request(EditorRequest::AssetFile(AssetFileOp::Move {
            path: "scenes/level.kscene".to_owned(),
            into: "levels".to_owned(),
        }));

    run_queued_asset_file_ops(Some(&project.vfs), &state);

    let assets = project.root.join("assets");
    {
        let state = state.lock().expect("state");
        assert_eq!(
            state.current_scene_path,
            Some(SceneInProject::new(&assets, "levels/level.kscene"))
        );
        assert_eq!(
            state.inspected_asset_path.as_deref(),
            Some("levels/level.kscene")
        );
    }
    save_a_world(&project, &state);
    assert!(
        !assets.join("scenes").join("level.kscene").exists(),
        "the save brought the scene back where it was"
    );
    assert_ne!(
        std::fs::read(assets.join("levels").join("level.kscene"))
            .ok()
            .as_deref(),
        Some(BEFORE),
        "the moved scene is not the one saved"
    );
}

/// **The open scene follows its folder through a rename**: a save after the browser renamed the folder writes the file in the renamed folder.
#[test]
fn a_save_after_renaming_the_open_scenes_folder_writes_in_the_renamed_folder() {
    const BEFORE: &[u8] = b"the scene as last saved";
    let project = project(
        "rename-open-scene-folder",
        &[("scenes/level.kscene", BEFORE)],
    );
    let state = editing_the_level(&project);
    state
        .lock()
        .expect("state")
        .push_request(EditorRequest::AssetFile(AssetFileOp::Rename {
            from: "scenes".to_owned(),
            to: "levels".to_owned(),
        }));

    run_queued_asset_file_ops(Some(&project.vfs), &state);
    save_a_world(&project, &state);

    let assets = project.root.join("assets");
    assert!(
        !assets.join("scenes").exists(),
        "the save made the old folder again"
    );
    assert_ne!(
        std::fs::read(assets.join("levels").join("level.kscene"))
            .ok()
            .as_deref(),
        Some(BEFORE),
        "the scene in the renamed folder is not the one saved"
    );
}

/// **The open scene trashed has no file any more**: the editor forgets it, so a save asks where, as Save As does, and never makes the trashed file again; the asset inspected is forgotten too.
#[test]
fn trashing_the_open_scene_leaves_the_scene_with_no_file() {
    let project = project("trash-open-scene", &[("scenes/level.kscene", b"saved")]);
    let state = editing_the_level(&project);
    let bin = project.root.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin");

    {
        let mut pvfs = project.vfs.lock().expect("the project");
        let trashed = pvfs
            .trash_with("scenes/level.kscene", |abs| {
                Ok(std::fs::rename(abs, bin.join("level.kscene"))?)
            })
            .expect("the scene is trashed");
        let mut state = state.lock().expect("state");
        super::super::asset_ops::follow_key_changes(&mut pvfs, &mut state, &[trashed]);
    }

    let state = state.lock().expect("state");
    assert_eq!(
        state.current_scene_path, None,
        "a save would make the trashed scene again"
    );
    assert_eq!(state.inspected_asset_path, None);
}
