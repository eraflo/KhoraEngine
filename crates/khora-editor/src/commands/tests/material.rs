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

//! Saving an entity's inline material as a `.kmat` of a project on disk.

use std::path::PathBuf;

use khora_sdk::khora_core::asset::StandardMaterial;
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

/// A project for the test `name`, holding `materials/steel.kmat`.
fn project_with_steel(name: &str) -> Project {
    let root = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let materials = root.join("assets").join("materials");
    std::fs::create_dir_all(&materials).expect("the materials folder");
    std::fs::write(materials.join("steel.kmat"), b"()").expect("the material is written");
    let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .expect("the project opens");
    Project {
        root,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

/// **A material saved over a file under another case is assigned the file's identity.**
///
/// Where file names ignore case, saving a material named `Steel` writes over
/// `materials/steel.kmat`, which keeps its name: the entity must reference
/// the asset the index lists, not the identity of a path no file has.
#[test]
fn a_material_saved_over_a_file_under_another_case_is_assigned_its_identity() {
    let project = project_with_steel("material-over-other-case");
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .push_request(EditorRequest::SaveAsMaterial {
            entity,
            name: "Steel".to_owned(),
        });

    save_queued_materials(Some(&project.vfs), &mut world, &state);

    let pvfs = project.vfs.lock().expect("the project");
    let steel = pvfs.resolve_uuid("materials/steel.kmat");
    assert!(
        pvfs.assets().index().get_metadata(&steel).is_some(),
        "the premise: the file written over is indexed under its own name"
    );
    let assigned = match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Asset(uuid)) => Some(*uuid),
        _ => None,
    };
    assert_eq!(
        assigned,
        Some(steel),
        "the entity references an asset the index does not list"
    );
}

/// **A material saved over a file while the identity registry is unread is refused, nothing written or assigned.**
///
/// The id a renamed file holds is unknown meanwhile: the entity would be
/// assigned another asset than the file once the registry reads.
#[test]
fn a_material_saved_over_a_file_while_the_registry_is_unread_is_refused() {
    let project = project_with_steel("material-unread-registry");
    let registry = khora_sdk::AssetIdRegistry::unread(&project.root).file_path();
    std::fs::File::create(&registry)
        .and_then(|file| file.set_len((64 << 20) + 1))
        .expect("the registry is made too large to read");
    let mut world = GameWorld::new();
    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Crate"),
        MaterialRef::inline(Box::new(StandardMaterial::default())),
    ));
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .push_request(EditorRequest::SaveAsMaterial {
            entity,
            name: "steel".to_owned(),
        });

    save_queued_materials(Some(&project.vfs), &mut world, &state);

    let steel = project
        .root
        .join("assets")
        .join("materials")
        .join("steel.kmat");
    assert_eq!(
        std::fs::read(steel).expect("the material"),
        b"()",
        "the file was written"
    );
    assert!(
        matches!(
            world.get_component::<MaterialRef>(entity),
            Some(MaterialRef::Inline { .. })
        ),
        "the entity was assigned an id the file may not hold"
    );
}
