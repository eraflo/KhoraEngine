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

//! **A scene saved in the project is written whole or not at all**: a save
//! replaces the file, and one that fails leaves what was there.

use std::path::{Path, PathBuf};

use khora_sdk::MetricsRegistry;

use super::super::*;

/// A project on disk, removed when the test ends.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh project for the test `name`, with its `assets/scenes/` folder, and its VFS.
fn project(name: &str) -> (Folder, ProjectVfs) {
    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("an old folder is removed");
    }
    std::fs::create_dir_all(dir.join("assets").join("scenes")).expect("the scenes folder");
    let pvfs = ProjectVfs::open(dir.clone(), std::sync::Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    (Folder(dir), pvfs)
}

/// The names of the entries of `dir`, sorted.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the folder lists")
        .map(|entry| {
            (entry.expect("an entry").file_name())
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn one_entity_world() -> GameWorld {
    let mut world = GameWorld::new();
    world.spawn((Transform::identity(), Name::new("Kept")));
    world
}

#[test]
fn a_save_over_a_scene_replaces_it_whole_and_leaves_nothing_beside() {
    let (dir, mut pvfs) = project("save-replaces-whole");
    let scenes = dir.0.join("assets").join("scenes");
    let path = scenes.join("level.kscene");
    std::fs::write(&path, vec![b'x'; 1 << 16]).expect("a previous file");

    assert!(save_scene_in_project(
        &mut pvfs,
        &one_entity_world(),
        Path::new("scenes/level.kscene")
    ));

    let bytes = std::fs::read(&path).expect("the scene is there");
    let file = khora_sdk::SceneFile::from_bytes(&bytes).expect("a scene file, whole");
    khora_sdk::khora_data::scene::read_scene_file(&file).expect("the saved scene reads");
    assert_eq!(listed(&scenes), vec!["level.kscene"], "no temporary left");
}

#[test]
fn a_failed_save_leaves_what_was_there() {
    let (dir, mut pvfs) = project("save-fails-whole");
    let scenes = dir.0.join("assets").join("scenes");
    // A folder where the scene goes: the save lands nowhere.
    let path = scenes.join("level.kscene");
    std::fs::create_dir(&path).expect("the folder in the way");
    std::fs::write(path.join("inside.txt"), b"kept").expect("a file inside");

    assert!(!save_scene_in_project(
        &mut pvfs,
        &one_entity_world(),
        Path::new("scenes/level.kscene")
    ));

    assert_eq!(
        std::fs::read(path.join("inside.txt")).expect("still there"),
        b"kept"
    );
    assert_eq!(listed(&scenes), vec!["level.kscene"], "no temporary left");
}
