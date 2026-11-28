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

//! **What a scene save left out reaches Problems through the editor's own
//! commands**: a mesh made in code, on the entity that holds it; a save that
//! leaves nothing out clears the list.

use std::path::PathBuf;

use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;
use khora_sdk::khora_data::ecs::AssetRef;
use khora_sdk::MetricsRegistry;

use super::*;

/// A folder on disk, removed when the test ends.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh folder for the test `name`.
fn folder(name: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    Folder(dir)
}

/// Saving a scene holding a mesh made in code asks to report it as a save
/// problem on its entity; saving again once it is gone clears the problems.
#[test]
fn a_save_leaving_out_a_mesh_made_in_code_reports_it_until_a_save_leaves_nothing_out() {
    let dir = folder("save-problems");
    let mut world = GameWorld::new();
    let made = world.spawn((
        Transform::identity(),
        Name::new("Made"),
        AssetRef::<Mesh>::Made(AssetUUID::new()),
    ));
    world
        .inner_world_mut()
        .mark_authored(made)
        .expect("authored");
    let vfs = ProjectVfs::open(dir.0.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    let vfs = Arc::new(Mutex::new(vfs));
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").current_scene_path =
        Some(SceneInProject::new(dir.0.join("assets"), "level.kscene"));

    apply_save(Some(&vfs), &mut world, &state);

    let problems = take_save_problems(&state).expect("the save reports its problems");
    assert!(
        problems.iter().any(|problem| problem.entity == Some(made)
            && problem.component.as_deref() == Some("AssetRef<Mesh>")),
        "the save does not report the mesh made in code on its entity: {problems:#?}"
    );

    world.remove_component::<AssetRef<Mesh>>(made);
    apply_save(Some(&vfs), &mut world, &state);

    assert_eq!(
        take_save_problems(&state),
        Some(Vec::new()),
        "a save leaving nothing out does not clear the save problems"
    );
}
