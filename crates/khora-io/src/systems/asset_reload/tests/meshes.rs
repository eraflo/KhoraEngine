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

//! Meshes: a modified mesh replaced in the store, one gone from the index
//! dropped.

use super::*;

/// The modified mesh is replaced in the store, under the same uuid: the
/// arrival the mesh residency makes its GPU copy again on.
#[test]
fn a_modified_mesh_reaches_its_entities_and_is_replaced_in_the_store() {
    let obj_path = "meshes/shape.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the triangle resolves")
            .positions
            .len(),
        3
    );
    project.watch::<Mesh>();

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.tick(&mut world);

    let resolved =
        mesh_of(&project, &world, entity).expect("the store keeps the mesh the entity names");
    assert_eq!(mesh_ref_uuid(&world, entity), mesh, "the same uuid");
    assert_eq!(resolved.positions.len(), 6, "the quad's vertices");
    assert_eq!(
        project
            .stored_mesh(&mesh)
            .expect("the store holds it")
            .positions
            .len(),
        6,
        "the store serves the modified mesh"
    );
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        vec![ArrivalKind::Replaced],
        "replaced once in the store: the residency makes its GPU copy again"
    );
}

#[test]
fn an_asset_gone_from_the_index_is_dropped() {
    let obj_path = "meshes/gone.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    project.watch::<Mesh>();

    project.remove_and_reindex(obj_path);
    project.invalidate(&mesh);
    project.tick(&mut world);

    assert!(
        mesh_of(&project, &world, entity).is_none(),
        "the store holds no copy of the deleted mesh the entity names"
    );
    assert!(project.stored_mesh(&mesh).is_none(), "the store drops it");
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        vec![ArrivalKind::Gone],
        "gone from the store: the residency retires its GPU copy"
    );
    assert!(
        world.get::<AssetRef<Mesh>>(entity).is_some(),
        "the authored reference is the author's, and stays"
    );
}
