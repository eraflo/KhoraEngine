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

//! A script the asset browser asked for, written into a project on disk: the
//! file appears, the browser lists it and is asked to reveal it, and an
//! existing file is never written over.

use std::path::PathBuf;

use khora_sdk::MetricsRegistry;

use super::super::*;

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
        "khora-editor-create-script-{name}-{}",
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
    Arc::new(Mutex::new(EditorState {
        project_folder: Some(project.root.to_string_lossy().to_string()),
        asset_epoch: 1,
        pending_create_script: Some((PLAYER.to_owned(), SOURCE.to_owned())),
        ..EditorState::default()
    }))
}

#[test]
fn a_queued_script_is_written_listed_and_revealed() {
    let project = project("written", &[]);
    let state = asking_for_the_player(&project);

    process_pending_asset_file_ops(Some(&project.vfs), &state);

    assert_eq!(
        std::fs::read(project.player()).ok().as_deref(),
        Some(SOURCE.as_bytes()),
        "the script is written with its source"
    );
    let state = state.lock().expect("state");
    assert_eq!(state.pending_create_script, None, "the request is consumed");
    assert!(state.asset_epoch > 1, "the browser is told to rescan");
    assert!(
        state
            .asset_entries
            .iter()
            .any(|entry| entry.source_path == PLAYER),
        "the browser's list holds the script: {:?}",
        state.asset_entries
    );
    assert_eq!(state.pending_reveal_asset.as_deref(), Some(PLAYER));
}

#[test]
fn a_queued_script_never_writes_over_a_file_and_reveals_nothing() {
    let project = project("existing", &[(PLAYER, b"what the author wrote")]);
    let state = asking_for_the_player(&project);

    process_pending_asset_file_ops(Some(&project.vfs), &state);

    assert_eq!(
        std::fs::read(project.player()).ok().as_deref(),
        Some(&b"what the author wrote"[..]),
        "the author's file is untouched"
    );
    let state = state.lock().expect("state");
    assert_eq!(state.pending_create_script, None, "the request is consumed");
    assert_eq!(state.pending_reveal_asset, None);
}

#[test]
fn a_queued_script_with_no_project_open_writes_nothing() {
    let project = project("closed", &[]);
    let state = asking_for_the_player(&project);

    process_pending_asset_file_ops(None, &state);

    assert!(!project.player().exists(), "a script was written");
    let state = state.lock().expect("state");
    assert_eq!(state.pending_create_script, None, "the request is consumed");
    assert_eq!(state.pending_reveal_asset, None);
}
