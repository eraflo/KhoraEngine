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

//! Each form of reference resolved into the store under its uuid: an
//! inline material, a procedural mesh, a `.kmat` or an `.obj` file.

use super::*;

#[test]
fn resolves_inline_material_into_the_store_under_its_uuid() {
    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let distinctive = StandardMaterial {
        base_color: LinearRgba::new(0.2, 0.4, 0.6, 1.0),
        roughness: 0.33,
        metallic: 0.7,
        ..Default::default()
    };
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(distinctive.clone()),
    )));

    resolve_once(&mut world, &runtime);

    let standard = stored_standard(&store, &material_uuid(&world, entity));
    assert_eq!(standard.base_color, distinctive.base_color);
    assert_eq!(standard.roughness, distinctive.roughness);
    assert_eq!(standard.metallic, distinctive.metallic);
}

#[test]
fn inline_resolution_is_idempotent() {
    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial::default()),
    )));

    resolve_once(&mut world, &runtime);
    let uuid = material_uuid(&world, entity);
    let first = stored::<Box<dyn Material>>(&store, &uuid).expect("first pass resolves");

    // A second pass finds the copy already stored and changes nothing.
    resolve_once(&mut world, &runtime);
    let second = stored::<Box<dyn Material>>(&store, &uuid).expect("the copy stays");
    assert!(first == second, "the same copy is kept");
}

/// A `AssetRef<Box<dyn Material>>` changed to another inline material resolves under its new uuid.
///
/// The new copy lands in the store under that uuid: the arrival the
/// material residency makes its GPU copy on.
#[test]
fn changing_inline_material_stores_the_new_copy_under_its_uuid() {
    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let mat_a = StandardMaterial {
        base_color: LinearRgba::new(0.1, 0.2, 0.3, 1.0),
        ..Default::default()
    };
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(mat_a),
    )));
    resolve_once(&mut world, &runtime);
    let uuid_a = material_uuid(&world, entity);
    assert!(
        stored::<Box<dyn Material>>(&store, &uuid_a).is_some(),
        "A resolves"
    );
    watch::<Box<dyn Material>>(&store);

    // The developer/editor swaps in a different inline material.
    let mat_b = StandardMaterial {
        base_color: LinearRgba::new(0.9, 0.8, 0.7, 1.0),
        roughness: 0.5,
        ..Default::default()
    };
    assert!(
        world.set_component(
            entity,
            AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(mat_b)))
        ),
        "replacing the AssetRef<Box<dyn Material>> must succeed"
    );
    resolve_once(&mut world, &runtime);

    let uuid_b = material_uuid(&world, entity);
    assert_ne!(
        uuid_a, uuid_b,
        "different material content => different uuid"
    );
    assert_eq!(
        arrived::<Box<dyn Material>>(&store),
        vec![Arrival {
            uuid: uuid_b,
            kind: ArrivalKind::Landed,
        }],
        "the new copy lands under its uuid, for the residency to make it resident"
    );
}

/// Re-running the resolver with no ref change stores nothing again: the
/// store records no arrival, so the residency keeps its GPU copy.
#[test]
fn steady_state_material_resolution_stores_nothing_again() {
    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial::default()),
    )));
    resolve_once(&mut world, &runtime);
    let uuid = material_uuid(&world, entity);
    assert!(
        stored::<Box<dyn Material>>(&store, &uuid).is_some(),
        "resolves"
    );
    watch::<Box<dyn Material>>(&store);

    resolve_once(&mut world, &runtime);

    assert!(
        stored::<Box<dyn Material>>(&store, &uuid).is_some(),
        "steady state keeps the copy"
    );
    assert!(
        arrived::<Box<dyn Material>>(&store).is_empty(),
        "steady state stores nothing again"
    );
}

/// A second entity naming an inline material the store holds shares that
/// copy: nothing is stored again, so no GPU copy is made twice.
#[test]
fn an_entity_naming_a_stored_inline_material_shares_its_copy() {
    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);
    let material = || {
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(
            StandardMaterial::default(),
        )))
    };

    let first = world.spawn(material());
    resolve_once(&mut world, &runtime);
    watch::<Box<dyn Material>>(&store);
    let second = world.spawn(material());
    resolve_once(&mut world, &runtime);

    assert_eq!(
        material_uuid(&world, first),
        material_uuid(&world, second),
        "both entities name the store's one copy"
    );
    assert_eq!(store.store::<Box<dyn Material>>().read().unwrap().len(), 1);
    assert!(
        arrived::<Box<dyn Material>>(&store).is_empty(),
        "the copy held is not stored again"
    );
}

/// A `AssetRef<Mesh>` changed to other procedural params resolves under its new uuid.
///
/// The new copy lands in the store under that uuid: the arrival the mesh
/// residency makes its GPU copy on.
#[test]
fn changing_procedural_mesh_stores_the_new_copy_under_its_uuid() {
    use khora_core::asset::ProceduralMeshKind;

    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let entity = world.spawn(AssetRef::<Mesh>::Local(ProceduralMesh::new(
        ProceduralMeshKind::Cube,
        [1.0, 0.0, 0.0, 0.0],
    )));
    resolve_once(&mut world, &runtime);
    let uuid_a = mesh_uuid(&world, entity);
    assert!(stored::<Mesh>(&store, &uuid_a).is_some(), "mesh A resolves");
    watch::<Mesh>(&store);

    // Resize the cube — different params => different content uuid.
    assert!(
        world.set_component(
            entity,
            AssetRef::<Mesh>::Local(ProceduralMesh::new(
                ProceduralMeshKind::Cube,
                [4.0, 0.0, 0.0, 0.0]
            )),
        ),
        "replacing the AssetRef<Mesh> must succeed"
    );
    resolve_once(&mut world, &runtime);

    let uuid_b = mesh_uuid(&world, entity);
    assert_ne!(uuid_a, uuid_b, "different params => different uuid");
    assert_eq!(
        arrived::<Mesh>(&store),
        vec![Arrival {
            uuid: uuid_b,
            kind: ArrivalKind::Landed,
        }],
        "the new copy lands under its uuid, for the residency to make it resident"
    );
}

#[test]
fn resolves_asset_material_from_kmat_via_service() {
    use std::fs;

    use khora_telemetry::MetricsRegistry;
    use tempfile::tempdir;

    use crate::asset::{FileLoader, IndexBuilder};

    // A project assets dir with one `.kmat`. The on-disk form is RON of the
    // `{ type_name, material }` JSON-value tree the decoder consumes.
    let dir = tempdir().unwrap();
    let assets_root = dir.path();
    fs::create_dir_all(assets_root.join("materials")).unwrap();
    let rel_path = "materials/brass.kmat";
    let kmat = r#"{
            "type_name": "StandardMaterial",
            "material": {
                "base_color": { "r": 0.71, "g": 0.65, "b": 0.26, "a": 1.0 },
                "base_color_texture": (),
                "metallic": 1.0,
                "roughness": 0.18,
                "metallic_roughness_texture": (),
                "normal_map": (),
                "occlusion_map": (),
                "emissive": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                "emissive_texture": (),
                "alpha_mode": "Opaque",
                "alpha_cutoff": 0.5,
                "double_sided": false,
            },
        }"#;
    fs::write(assets_root.join(rel_path), kmat).unwrap();

    // Build the VFS index. `.kmat` UUIDs are keyed by `new_v5` over the
    // forward-slash relative path (the IndexBuilder convention), so an
    // editor minting `AssetRef::Asset` uses the same key.
    let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
        assets_root,
        crate::trust::Trust::Project,
        crate::trust::Refusals::detached(),
    ))
    .build_index_bytes()
    .unwrap();
    let uuid = AssetUUID::new_v5(rel_path);

    let metrics = Arc::new(MetricsRegistry::new());
    let mut service = AssetService::new(
        &index_bytes,
        Box::new(FileLoader::new(assets_root)),
        metrics,
        None,
    )
    .unwrap();
    // Pulls in the inventory-registered material decoder.
    service.register_inventory_decoders();

    let store = AssetStore::new();
    let mut runtime = runtime_with(&store);
    runtime.resources.insert(khora_core::lane::Reserve::new());
    runtime
        .services
        .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Asset(uuid));

    // Decoded off the frame: resolved once its decode was taken in.
    resolve_settled(&mut world, &runtime);

    // The store holds the decoded material under the asset's stable uuid,
    // so the GPU projection (and a second referencing entity) see it.
    let standard = stored_standard(&store, &uuid);
    assert_eq!(standard.metallic, 1.0);
    assert_eq!(standard.roughness, 0.18);
    assert_eq!(standard.base_color.r, 0.71);
}

#[test]
fn resolves_procedural_mesh_into_the_store_with_cube_counts() {
    use khora_core::asset::ProceduralMeshKind;

    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let entity = world.spawn(AssetRef::<Mesh>::Local(ProceduralMesh::new(
        ProceduralMeshKind::Cube,
        [2.0, 0.0, 0.0, 0.0],
    )));

    resolve_once(&mut world, &runtime);

    let mesh = stored::<Mesh>(&store, &mesh_uuid(&world, entity))
        .expect("the procedural mesh is stored under its content uuid");
    assert_eq!(mesh.positions.len(), 24, "cube has 24 vertices");
    assert_eq!(
        mesh.indices.as_ref().map_or(0, |i| i.len()),
        36,
        "cube has 36 indices"
    );
}

/// Two entities with identical procedural meshes dedup to one content uuid.
#[test]
fn identical_procedural_meshes_share_one_uuid() {
    use khora_core::asset::ProceduralMeshKind;

    let mut world = World::new();
    let store = AssetStore::new();
    let runtime = runtime_with(&store);

    let make = || {
        AssetRef::<Mesh>::Local(ProceduralMesh::new(
            ProceduralMeshKind::Sphere,
            [0.5, 16.0, 16.0, 0.0],
        ))
    };
    let a = world.spawn(make());
    let b = world.spawn(make());

    resolve_once(&mut world, &runtime);

    assert_eq!(
        mesh_uuid(&world, a),
        mesh_uuid(&world, b),
        "identical procedural meshes must share a content uuid"
    );
    assert_eq!(
        store.store::<Mesh>().read().unwrap().len(),
        1,
        "one copy is stored for both"
    );
}

/// End-to-end: an `AssetRef::Asset` referencing a tiny `.obj` in the VFS is
/// loaded by the resolver through a real `AssetService`, never an empty placeholder.
#[test]
fn resolves_asset_mesh_from_obj_via_service() {
    use std::fs;

    use khora_telemetry::MetricsRegistry;
    use tempfile::tempdir;

    use crate::asset::{FileLoader, IndexBuilder, MeshDispatcher};

    let dir = tempdir().unwrap();
    let assets_root = dir.path();
    fs::create_dir_all(assets_root.join("meshes")).unwrap();
    let rel_path = "meshes/tri.obj";
    // A single triangle — the smallest renderable OBJ.
    let obj = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";
    fs::write(assets_root.join(rel_path), obj).unwrap();

    let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
        assets_root,
        crate::trust::Trust::Project,
        crate::trust::Refusals::detached(),
    ))
    .build_index_bytes()
    .unwrap();
    let uuid = AssetUUID::new_v5(rel_path);

    let metrics = Arc::new(MetricsRegistry::new());
    let mut service = AssetService::new(
        &index_bytes,
        Box::new(FileLoader::new(assets_root)),
        metrics,
        None,
    )
    .unwrap();
    // The mesh dispatcher has competing impls (gltf vs obj) so it is wired
    // explicitly at the call site rather than via inventory.
    service.register_decoder::<Mesh>(
        khora_core::asset::kinds::MESH.name(),
        MeshDispatcher::default(),
    );

    let store = AssetStore::new();
    let mut runtime = runtime_with(&store);
    runtime.resources.insert(khora_core::lane::Reserve::new());
    runtime
        .services
        .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

    let mut world = World::new();
    world.spawn(AssetRef::<Mesh>::Asset(uuid));

    // Decoded off the frame: resolved once its decode was taken in.
    resolve_settled(&mut world, &runtime);

    let mesh =
        stored::<Mesh>(&store, &uuid).expect("the asset mesh is stored under its stable uuid");
    assert_eq!(mesh.positions.len(), 3, "the triangle has 3 vertices");
}
