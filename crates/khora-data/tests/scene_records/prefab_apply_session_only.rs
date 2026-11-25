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

//! Applying an instance to its prefab when a member holds a value only the
//! session holds (a mesh made in code): no prefab file can take it.

use khora_core::asset::AssetUUID;
use khora_data::ecs::{MeshRef, Name, Transform};
use khora_data::scene::record::{ReportEntry, ReportKind};
use khora_data::scene::{
    apply_to_prefab_reporting, capture_subtree, instance_of, instantiate_prefab, PrefabApply,
    SaveError,
};

use super::prefab_sample::Library;
use super::*;

/// A crate prefab's record and its lid's id in it, and a world holding an
/// instance of it under `id` whose lid was given a mesh made in code, with
/// the lid's entity.
fn crate_with_made_lid(id: AssetUUID) -> (Library, World, EntityId) {
    let mut prefab = World::new();
    let root = prefab.spawn((Transform::identity(), Name::new("Crate")));
    let lid = prefab.spawn((
        Transform::identity(),
        Name::new("Lid"),
        MeshRef::Asset(AssetUUID::new()),
    ));
    prefab.set_parent(lid, Some(root));
    prefab.mark_authored(root).expect("authored");
    let lid = prefab.mark_authored(lid).expect("authored");
    let library = Library::default().with(
        id,
        capture_subtree(&prefab, root).expect("the crate captures"),
    );

    let mut world = World::new();
    let root = instantiate_prefab(&mut world, id, &library).expect("the crate instantiates");
    let instance = world.persistent_id(root).expect("an id");
    let lid = world
        .entity_with_id(PersistentId::within(instance, lid))
        .expect("the instance's lid");
    assert!(world.set_component(lid, MeshRef::Made(AssetUUID::new())));
    (library, world, lid)
}

/// Applying the whole instance reports the lid's mesh made in code `NotSaved`
/// on the lid: the prefab keeps its own mesh.
#[test]
fn apply_to_prefab_reporting_reports_a_mesh_made_in_code_it_cannot_write() {
    let id = AssetUUID::new();
    let (library, world, lid) = crate_with_made_lid(id);
    let instance = instance_of(&world, lid, &library).expect("an instance");

    let (_, left_out) =
        apply_to_prefab_reporting(&world, &instance, &PrefabApply::Instance, &library)
            .expect("the instance applies");

    assert_eq!(
        left_out,
        [ReportEntry {
            entity: world.persistent_id(lid),
            component: Some("MeshRef".to_owned()),
            path: String::new(),
            kind: ReportKind::NotSaved,
        }]
    );
}

/// Applying the lid's mesh alone is refused, saying why: the value lives only
/// in this session, and a prefab cannot hold it.
#[test]
fn applying_a_mesh_made_in_code_alone_is_refused_as_session_only() {
    let id = AssetUUID::new();
    let (library, world, lid) = crate_with_made_lid(id);
    let instance = instance_of(&world, lid, &library).expect("an instance");

    let refused = apply_to_prefab_reporting(
        &world,
        &instance,
        &PrefabApply::Component {
            entity: lid,
            component: "MeshRef".to_owned(),
        },
        &library,
    );

    match refused {
        Err(SaveError::Prefab(message)) => assert!(
            message.contains("only in this session"),
            "the refusal does not say why: {message}"
        ),
        other => panic!("applying a mesh made in code is not refused: {other:?}"),
    }
}
