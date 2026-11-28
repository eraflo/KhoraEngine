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

//! **The folder the browser shows follows the file operations on it**: a
//! rename or a move of that folder, or of a folder above it, shows it under
//! its new key; a trash of it shows the nearest folder above it left.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetFileOp, EditorPanel, EditorRequest, EditorState, UiTheme};
use khora_sdk::MetricsRegistry;
use khora_tool_ui::testing::RecordingUiBuilder;

use super::super::AssetBrowserPanel;
use crate::commands::run_queued_asset_file_ops;
use crate::hot_reload::collect_asset_entries;
use crate::project_vfs::ProjectVfs;

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

/// A project named `name` holding `files`, forward-slash paths under `assets/`.
fn project(name: &str, files: &[&str]) -> Project {
    let root = std::env::temp_dir().join(format!(
        "khora-editor-browser-current-folder-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets).expect("the assets folder");
    for rel in files {
        let path = rel
            .split('/')
            .fold(assets.clone(), |at, part| at.join(part));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's folder");
        }
        std::fs::write(&path, b"notes").expect("the file is written");
    }
    let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    Project {
        root,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

/// A browser over `project`, its list read, showing the folder `folder`.
fn showing(project: &Project, folder: &str) -> (AssetBrowserPanel, Arc<Mutex<EditorState>>) {
    let (entries, dirs) = {
        let pvfs = project.vfs.lock().expect("the project");
        (collect_asset_entries(&pvfs), pvfs.list_dirs())
    };
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().into_owned()),
        asset_entries: entries,
        asset_dirs: dirs,
        asset_epoch: 1,
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.ui(&mut RecordingUiBuilder::new(1200.0, 420.0));
    panel.current_folder = Some(folder.to_owned());
    (panel, state)
}

/// Runs `op` as the app runs the browser's file operations, then one frame of the browser.
fn run(
    project: &Project,
    panel: &mut AssetBrowserPanel,
    state: &Arc<Mutex<EditorState>>,
    op: AssetFileOp,
) {
    state
        .lock()
        .expect("the editor state")
        .push_request(EditorRequest::AssetFile(op));
    run_queued_asset_file_ops(Some(&project.vfs), state);
    panel.ui(&mut RecordingUiBuilder::new(1200.0, 420.0));
}

/// **A folder above the one shown, renamed, takes it along**: the browser shows it under the renamed folder.
#[test]
fn the_folder_shown_follows_a_rename_of_a_folder_above_it() {
    let project = project("ancestor-renamed", &["levels/forest/notes.txt"]);
    let (mut panel, state) = showing(&project, "levels/forest");

    run(
        &project,
        &mut panel,
        &state,
        AssetFileOp::Rename {
            from: "levels".to_owned(),
            to: "worlds".to_owned(),
        },
    );

    assert!(
        project
            .root
            .join("assets")
            .join("worlds")
            .join("forest")
            .is_dir(),
        "the premise: the folder is renamed"
    );
    assert_eq!(
        panel.current_folder.as_deref(),
        Some("worlds/forest"),
        "the browser shows a folder that is not there any more"
    );
}

/// **The folder shown, moved, is shown where it went.**
#[test]
fn the_folder_shown_follows_a_move_of_itself() {
    let project = project("moved", &["levels/forest/notes.txt", "worlds/map.txt"]);
    let (mut panel, state) = showing(&project, "levels/forest");

    run(
        &project,
        &mut panel,
        &state,
        AssetFileOp::Move {
            path: "levels/forest".to_owned(),
            into: "worlds".to_owned(),
        },
    );

    assert!(
        project
            .root
            .join("assets")
            .join("worlds")
            .join("forest")
            .is_dir(),
        "the premise: the folder is moved"
    );
    assert_eq!(
        panel.current_folder.as_deref(),
        Some("worlds/forest"),
        "the browser shows a folder that is not there any more"
    );
}

/// **The folder shown, trashed with the folder above it, gives way to the nearest parent left.**
#[test]
fn the_folder_shown_gives_way_to_its_nearest_parent_left_by_a_trash() {
    let project = project(
        "trashed",
        &["levels/forest/caves/notes.txt", "levels/map.txt"],
    );
    let (mut panel, state) = showing(&project, "levels/forest/caves");
    let bin = project.root.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin");

    {
        let mut pvfs = project.vfs.lock().expect("the project");
        let trashed = pvfs
            .trash_with("levels/forest", |abs| {
                Ok(std::fs::rename(abs, bin.join("forest"))?)
            })
            .expect("the folder is trashed");
        let mut state = state.lock().expect("the editor state");
        crate::commands::follow_key_changes(&mut pvfs, &mut state, &[trashed]);
    }
    panel.ui(&mut RecordingUiBuilder::new(1200.0, 420.0));

    assert_eq!(
        panel.current_folder.as_deref(),
        Some("levels"),
        "the browser shows a folder that went to the recycle bin"
    );
}
