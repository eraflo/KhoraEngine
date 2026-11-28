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

//! Through the `loading` family: the frame that sees a change decodes
//! nothing and keeps the last good copy; the refresh runs on the frame the
//! new value lands.

use super::*;

/// The frame that sees a change files the decode and decodes nothing
/// itself.
///
/// The service counts no new load, the entity still shows the value it had,
/// and the `loading` family has one request to take.
#[test]
fn a_change_is_not_decoded_on_the_frame_it_is_seen() {
    let kmat_path = "materials/seen.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    let loaded = project.load_count();

    project.write(kmat_path, &kmat(0.9, None));
    project.invalidate(&mat);
    project.frame(&mut world);

    assert_eq!(
        project.load_count(),
        loaded,
        "the frame that sees the change decodes nothing"
    );
    assert_eq!(
        standard_of(&project, &world, entity).roughness,
        0.5,
        "the entity shows the value it had"
    );
    assert_eq!(
        project.decode(),
        1,
        "the changed material is filed once for the `loading` family"
    );
}

/// Until the new value lands, the engine shows the last good one.
///
/// The store keeps its copy, not replaced, so the GPU copy stays, frame
/// after frame; the change is filed once, however many frames it waits.
#[test]
fn the_last_good_value_serves_until_the_new_one_lands() {
    let obj_path = "meshes/waiting.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    project.watch::<Mesh>();

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    for frame in 0..3 {
        project.frame(&mut world);
        let resolved = mesh_of(&project, &world, entity)
            .unwrap_or_else(|| panic!("frame {frame}: the entity keeps its mesh"));
        assert_eq!(
            (mesh_ref_uuid(&world, entity), resolved.positions.len()),
            (mesh, 3),
            "frame {frame}: the last good triangle"
        );
        assert!(
            project.arrived::<Mesh>(&mesh).is_empty(),
            "frame {frame}: its copy is not replaced"
        );
        assert_eq!(
            project.stored_mesh(&mesh).map(|m| m.positions.len()),
            Some(3),
            "frame {frame}: the store keeps the last good copy"
        );
    }
    assert_eq!(project.decode(), 1, "filed once over the frames it waited");

    project.frame(&mut world);
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the store keeps the mesh the entity names")
            .positions
            .len(),
        6,
        "the quad, on the frame it lands"
    );
}

/// On the frame the new values land, the store's copies are replaced.
///
/// Under the same uuids the entities name; the arrivals are
/// what the residencies make the GPU copies again on.
///
/// Not before.
#[test]
fn on_landing_the_copies_are_replaced_the_same_frame() {
    let kmat_path = "materials/landing.kmat";
    let obj_path = "meshes/landing.obj";
    let mat = AssetUUID::new_v5(kmat_path);
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[
        (kmat_path, kmat(0.5, None)),
        (obj_path, TRIANGLE_OBJ.as_bytes().to_vec()),
    ]);
    let mut world = World::new();
    let entity = world.spawn((
        AssetRef::<Mesh>::Asset(mesh),
        AssetRef::<Box<dyn Material>>::Asset(mat),
    ));
    project.tick(&mut world);
    project.watch::<Mesh>();
    project.watch::<Box<dyn Material>>();

    project.write(kmat_path, &kmat(0.9, None));
    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mat);
    project.invalidate(&mesh);
    project.frame(&mut world);

    assert!(
        project.arrived::<Box<dyn Material>>(&mat).is_empty(),
        "nothing is replaced before the new values land"
    );
    assert!(project.arrived::<Mesh>(&mesh).is_empty());

    assert_eq!(project.decode(), 2, "the material and the mesh are filed");
    project.frame(&mut world);

    assert_eq!(
        material_uuid(&project, &world, entity),
        mat,
        "the same uuid"
    );
    assert_eq!(standard_of(&project, &world, entity).roughness, 0.9);
    let resolved =
        mesh_of(&project, &world, entity).expect("the store keeps the mesh the entity names");
    assert_eq!(
        (mesh_ref_uuid(&world, entity), resolved.positions.len()),
        (mesh, 6)
    );
    assert_eq!(
        project.arrived::<Box<dyn Material>>(&mat),
        vec![ArrivalKind::Replaced],
        "the material is replaced on the frame it lands"
    );
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        vec![ArrivalKind::Replaced],
        "the mesh is replaced on the frame it lands"
    );
}

/// A repainted texture: the store keeps the old image, not replaced, until
/// the new image lands.
///
/// On that frame the store serves it, replaced: the arrival the material
/// residency makes every material naming it again on.
#[test]
fn a_repainted_texture_is_replaced_the_frame_it_lands() {
    let tex_path = "textures/landing.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, png(2, 2))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));
    project.tick(&mut world);
    project.watch::<CpuTexture>();

    project.write(tex_path, &png(5, 5));
    project.invalidate(&tex);
    project.frame(&mut world);

    let held = project.texture(&tex).expect("the store keeps the image");
    assert_eq!(
        (held.size.width, held.size.height),
        (2, 2),
        "the last good image until the new one lands"
    );
    assert!(
        project.arrived::<CpuTexture>(&tex).is_empty(),
        "not replaced meanwhile: the material keeps its GPU copy"
    );

    assert_eq!(project.decode(), 1, "the texture is filed");
    project.frame(&mut world);

    let after = project.texture(&tex).expect("the store keeps the image");
    assert_eq!((after.size.width, after.size.height), (5, 5));
    assert_eq!(
        project.arrived::<CpuTexture>(&tex),
        vec![ArrivalKind::Replaced],
        "replaced on the frame it lands"
    );
    assert!(material_of(&project, &world, entity).is_some());
}

/// A reload whose decode fails is learned when the failure lands from the
/// `loading` family.
///
/// The entity keeps the last good value, not replaced in the store, so its
/// GPU copy stays, and an entity naming the asset afterwards sees that value.
#[test]
fn a_reload_that_fails_off_the_frame_keeps_the_last_good_value() {
    let kmat_path = "materials/broken_later.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    project.watch::<Box<dyn Material>>();

    project.write(kmat_path, b"definitely not a material");
    project.invalidate(&mat);
    project.frame(&mut world);
    assert_eq!(
        project.decode(),
        1,
        "the changed material is decoded by the family, not by the frame"
    );
    project.frame(&mut world);

    assert_eq!(
        standard_of(&project, &world, first).roughness,
        0.5,
        "the entity keeps the last good value once the failure landed"
    );
    assert!(
        project.arrived::<Box<dyn Material>>(&mat).is_empty(),
        "and the store its copy, so the GPU copy stays"
    );

    let later = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    assert_eq!(
        standard_of(&project, &world, later).roughness,
        0.5,
        "an entity naming it afterwards sees the last good value"
    );
}

/// The file changes again while the decode of its first change is out: that
/// decode read bytes the disk has replaced, so it is never installed.
///
/// The entity goes from the last good value straight to the newest one, and
/// no frame shows the one in between.
#[test]
fn a_change_made_while_a_reload_is_in_flight_installs_only_the_newest() {
    let kmat_path = "materials/twice.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);

    project.write(kmat_path, &kmat(0.9, None));
    project.invalidate(&mat);
    project.frame(&mut world);
    assert_ne!(
        standard_of(&project, &world, entity).roughness,
        0.9,
        "the value between is not shown on the frame the change is seen"
    );
    let filed = project.decode_while(|| {
        project.write(kmat_path, &kmat(0.3, None));
        project.invalidate(&mat);
    });
    assert_eq!(filed, 1, "the first change was filed");

    for frame in 0..8 {
        project.frame(&mut world);
        let roughness = standard_of(&project, &world, entity).roughness;
        assert_ne!(
            roughness, 0.9,
            "frame {frame}: the decode of the bytes overwritten is never shown"
        );
        if roughness == 0.3 {
            return;
        }
        assert_eq!(roughness, 0.5, "frame {frame}: the last good value");
        project.decode();
    }
    panic!("the newest value never landed");
}

/// A re-exported glTF shows its old parts until the re-exported ones land.
///
/// Its parts are decoded again by the family: the old parts on the frame the
/// change is seen, the re-exported ones on the frame they land.
#[test]
fn a_reexported_gltf_refreshes_its_parts_through_the_family() {
    let rel = "models/through.gltf";
    let bin = "through.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], bin);
    let project = Project::new(&[(rel, gltf), ("models/through.bin", buffer)]);
    let file = AssetUUID::new_v5(rel);
    let mesh = AssetUUID::sub_asset(file, "mesh0/prim0");
    let material = AssetUUID::sub_asset(file, "material0");
    let mut world = World::new();
    let entity = world.spawn((
        AssetRef::<Mesh>::Asset(mesh),
        AssetRef::<Box<dyn Material>>::Asset(material),
    ));
    project.tick(&mut world);
    let loaded = project.load_count();

    let (gltf, buffer) = gltf_shape(&TWO_TRIANGLES, [0.0, 1.0, 0.0, 1.0], bin);
    project.write(rel, &gltf);
    project.write("models/through.bin", &buffer);
    project.invalidate(&file);
    project.frame(&mut world);

    assert_eq!(project.load_count(), loaded, "no part decoded on the frame");
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh part stays")
            .positions
            .len(),
        3,
        "the old mesh part until the new one lands"
    );
    assert_eq!(standard_of(&project, &world, entity).base_color.g, 0.0);

    assert!(project.decode() > 0, "the parts are filed for the family");
    project.frame(&mut world);

    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh part stays resolved")
            .positions
            .len(),
        6,
        "the re-exported mesh part"
    );
    assert_eq!(
        standard_of(&project, &world, entity).base_color.g,
        1.0,
        "the re-exported material part"
    );
}

/// Only the buffer file beside a glTF is rewritten: the mesh part read
/// from it is decoded again by the family, and lands on the entity.
#[test]
fn a_rewritten_buffer_refreshes_the_gltf_mesh_part_through_the_family() {
    let rel = "models/buffered.gltf";
    let bin = "models/buffered.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "buffered.bin");
    let project = Project::new(&[(rel, gltf), (bin, buffer)]);
    let mesh = AssetUUID::sub_asset(AssetUUID::new_v5(rel), "mesh0/prim0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    let loaded = project.load_count();

    let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "buffered.bin");
    project.write(bin, &buffer);
    project.invalidate_file(bin);
    project.frame(&mut world);

    let x = |world: &World| {
        mesh_of(&project, world, entity)
            .expect("the mesh part stays resolved")
            .positions[1]
            .x
    };
    assert_eq!(project.load_count(), loaded, "no part decoded on the frame");
    assert_eq!(x(&world), 1.0, "the old vertices until the new ones land");

    assert!(
        project.decode() > 0,
        "the mesh part is filed for the family"
    );
    project.frame(&mut world);

    assert_eq!(x(&world), 2.0, "the rewritten buffer's vertices");
}

/// An asset gone from the index waits for no decode: its copies are
/// dropped on the frame the change is seen.
#[test]
fn an_asset_gone_from_the_index_is_dropped_the_frame_it_is_seen() {
    let obj_path = "meshes/gone_now.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    project.watch::<Mesh>();

    project.remove_and_reindex(obj_path);
    project.invalidate(&mesh);
    project.frame(&mut world);

    assert!(
        mesh_of(&project, &world, entity).is_none(),
        "the store holds no copy of the deleted mesh the entity names"
    );
    assert!(project.stored_mesh(&mesh).is_none(), "the store drops it");
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        vec![ArrivalKind::Gone],
        "gone on the frame it is seen: the residency retires its GPU copy"
    );
}

/// Decodes only the filed requests `pick` chooses, leased, and hands
/// them to the reserve; the others are filed again, untouched.
fn decode_only(project: &Project, pick: impl Fn(&AssetUUID) -> bool) -> usize {
    use khora_core::lane::Staging;
    let service = &project.service;
    let filed = service.lock().unwrap().decode_requests().drain();
    let decoding = AssetService::decoding(service);
    let reserve = project.runtime.look_up::<Reserve>().expect("the reserve");
    let mut staging = Staging::new();
    let mut taken = 0;
    for request in filed {
        if pick(&request.uuid) {
            let (decoded, lease) = decoding.decode_leased(&request);
            match lease {
                Some(lease) => staging.stage_while(vec![decoded], lease),
                None => staging.stage(vec![decoded]),
            }
            taken += 1;
        } else {
            service.lock().unwrap().decode_requests().send(request);
        }
    }
    reserve.receive(staging);
    taken
}

/// The changed assets the service notes as still decoding, left in place.
fn reloading(project: &Project) -> std::collections::HashSet<AssetUUID> {
    let mut svc = project.service.lock().unwrap();
    let taken = svc.take_reloading();
    svc.keep_reloading(taken.clone());
    taken
}

/// A material whose own change is still decoding names a texture that
/// changed too and landed first.
///
/// The texture is replaced first, the arrival the material residency makes
/// the material again on with the new image; the material is replaced
/// once its own new value lands.
#[test]
fn a_material_still_decoding_is_remade_when_a_texture_it_names_lands_first() {
    let tex_path = "textures/first.png";
    let tex = AssetUUID::new_v5(tex_path);
    let kmat_path = "materials/second.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(tex_path, png(2, 2)), (kmat_path, kmat(0.5, Some(tex)))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    project.watch::<CpuTexture>();
    project.watch::<Box<dyn Material>>();

    project.write(tex_path, &png(5, 5));
    project.write(kmat_path, &kmat(0.9, Some(tex)));
    project.invalidate(&tex);
    project.invalidate(&mat);
    project.frame(&mut world);

    assert_eq!(decode_only(&project, |uuid| *uuid == tex), 1);
    project.frame(&mut world);
    let image = project.texture(&tex).expect("the image");
    assert_eq!((image.size.width, image.size.height), (5, 5));
    assert_eq!(
        standard_of(&project, &world, entity).roughness,
        0.5,
        "the old material"
    );
    assert_eq!(
        project.arrived::<CpuTexture>(&tex),
        vec![ArrivalKind::Replaced],
        "the new image: the material is made again with it"
    );
    assert!(project.arrived::<Box<dyn Material>>(&mat).is_empty());

    assert_eq!(decode_only(&project, |uuid| *uuid == mat), 1);
    project.frame(&mut world);
    assert_eq!(standard_of(&project, &world, entity).roughness, 0.9);
    assert_eq!(
        project.arrived::<Box<dyn Material>>(&mat),
        vec![ArrivalKind::Replaced],
        "the new material: made again"
    );
    assert!(reloading(&project).is_empty(), "nothing left reloading");
}

/// The entity naming a changed mesh is despawned while the new mesh decodes.
///
/// The store, not the entity, holds the copy: the new mesh replaces it
/// when it lands, and nothing is left reloading.
#[test]
fn a_reload_whose_holder_is_despawned_still_replaces_the_stored_copy() {
    let obj_path = "meshes/despawned.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    assert!(reloading(&project).contains(&mesh));
    world.despawn(entity);
    project.frame(&mut world);
    assert_eq!(
        project.stored_mesh(&mesh).map(|mesh| mesh.positions.len()),
        Some(3),
        "the store keeps the last good copy meanwhile"
    );
    assert_eq!(project.decode(), 1);
    project.frame(&mut world);
    assert_eq!(
        project.stored_mesh(&mesh).map(|mesh| mesh.positions.len()),
        Some(6),
        "the new mesh replaces the stored copy"
    );
    assert!(reloading(&project).is_empty(), "nothing left reloading");
}

/// The editor hands its project's service over while a reload of the
/// engine's is decoding: the entity ends on the incoming project's mesh.
#[test]
fn a_service_handed_over_mid_reload_lands_the_incoming_mesh() {
    let obj_path = "meshes/handover.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    let five = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 2 0\nf 1 2 3\nf 1 3 4\nf 1 4 5\n";
    project.write(obj_path, five.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);

    let other = tempfile::tempdir().unwrap();
    write_file(&other.path().join("assets"), obj_path, QUAD_OBJ.as_bytes());
    let mut own = AssetService::open_loose_files(
        other.path(),
        &AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            other.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        )),
        Arc::new(MetricsRegistry::new()),
    )
    .unwrap();
    {
        let mut target = project.service.lock().unwrap();
        std::mem::swap(&mut own, &mut *target);
        target.announce_replacing(&own);
        target.adopt_decode_requests(&mut own);
    }
    for _ in 0..8 {
        project.decode();
        project.frame(&mut world);
    }
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("resolved")
            .positions
            .len(),
        6,
        "the incoming project's quad"
    );
    assert!(reloading(&project).is_empty());
}

/// The decode backlog is full on the frame a change is seen: the reload
/// is asked for again later and lands.
#[test]
fn a_reload_refused_by_a_full_backlog_lands_later() {
    let obj_path = "meshes/backlogged.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    let requests = project.service.lock().unwrap().decode_requests();
    let mut i = 0;
    let dropped = requests.dropped();
    while requests.dropped() == dropped {
        requests.send(khora_core::asset::DecodeRequest {
            uuid: AssetUUID::new_v5(&format!("filler/{i}")),
            type_id: std::any::TypeId::of::<Mesh>(),
            generation: 0,
        });
        i += 1;
    }
    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    requests.drain();
    assert!(reloading(&project).contains(&mesh));
    project.frame(&mut world);
    assert_eq!(project.decode(), 1, "asked for again");
    project.frame(&mut world);
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("resolved")
            .positions
            .len(),
        6
    );
}

/// An entity naming a mesh while its reload decodes is served what its
/// siblings show, the last good copy, until the new one lands.
///
/// The change is decoded once for both.
#[test]
fn an_entity_naming_an_asset_mid_reload_is_served_the_last_good_copy() {
    let obj_path = "meshes/sibling.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    let second = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.frame(&mut world);
    assert_eq!(project.decode(), 1, "decoded once for both");
    let shown = |world: &World, entity| {
        mesh_of(&project, world, entity).map(|resolved| resolved.positions.len())
    };
    assert_eq!(shown(&world, first), Some(3));
    assert_eq!(
        shown(&world, second),
        Some(3),
        "the sibling spawned mid-reload shows the last good copy too"
    );
}

/// A file removed (and the folder listed again) while its reload
/// decodes: the copies are dropped on the next frame.
#[test]
fn an_asset_removed_mid_reload_is_dropped() {
    let obj_path = "meshes/removed_mid.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    project.remove_and_reindex(obj_path);
    project.decode();
    project.frame(&mut world);
    assert!(mesh_of(&project, &world, entity).is_none());
    assert!(project.stored_mesh(&mesh).is_none());
    assert!(reloading(&project).is_empty());
}

/// The vertex count of the store's copy of the mesh `entity` names, if it holds one.
fn shown_vertices(project: &Project, world: &World, entity: EntityId) -> Option<usize> {
    mesh_of(project, world, entity).map(|mesh| mesh.positions.len())
}

/// An entity served the last good copy mid-reload is given the new value on the frame it lands.
///
/// Like the entity that held the asset before the change; the store's
/// copy is replaced once for both.
#[test]
fn a_sibling_served_the_last_good_copy_is_given_the_new_value_when_it_lands() {
    let obj_path = "meshes/retagged.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    let second = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.frame(&mut world);
    assert_eq!(shown_vertices(&project, &world, second), Some(3));
    project.watch::<Mesh>();

    project.decode();
    project.frame(&mut world);
    assert_eq!(shown_vertices(&project, &world, first), Some(6));
    assert_eq!(shown_vertices(&project, &world, second), Some(6));
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        vec![ArrivalKind::Replaced],
        "one copy replaced for both entities"
    );
}

/// The only holder of a changed mesh leaves while it decodes, and a new
/// entity names it on the same frame.
///
/// The new entity names the store's copy, which the new value replaces
/// once it lands.
#[test]
fn an_entity_naming_an_asset_its_last_holder_left_mid_reload_gets_the_new_value() {
    let obj_path = "meshes/handed_on.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    world.despawn(first);
    let second = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    for _ in 0..8 {
        project.frame(&mut world);
        project.decode();
    }
    project.frame(&mut world);
    assert_eq!(shown_vertices(&project, &world, second), Some(6));
}

/// A material resolved while its texture is decoded again is served the old image.
///
/// The texture is replaced once the new one lands: the arrival the
/// material residency makes the material again on.
#[test]
fn a_material_resolved_mid_reload_of_its_texture_is_remade_when_the_image_lands() {
    let tex_path = "textures/mid.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, png(2, 2))]);
    let mut world = World::new();
    let material = || {
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })))
    };
    world.spawn(material());
    project.tick(&mut world);

    project.write(tex_path, &png(5, 5));
    project.invalidate(&tex);
    project.frame(&mut world);
    let later = world.spawn(material());
    project.frame(&mut world);
    assert!(material_of(&project, &world, later).is_some());
    project.watch::<CpuTexture>();

    project.decode();
    project.frame(&mut world);
    let image = project.texture(&tex).expect("the image");
    assert_eq!((image.size.width, image.size.height), (5, 5));
    assert_eq!(
        project.arrived::<CpuTexture>(&tex),
        vec![ArrivalKind::Replaced],
        "the image the material names is replaced: the material is made again"
    );
}

/// The file changes again while a sibling is served the last good copy:
/// no frame shows the value in between, and both end on the newest.
#[test]
fn a_second_change_while_the_last_good_copy_is_served_lands_only_the_newest() {
    let obj_path = "meshes/twice_served.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);
    let second = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.frame(&mut world);
    let five = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 2 0\nf 1 2 3\nf 1 3 4\nf 1 4 5\n";
    project.decode_while(|| {
        project.write(obj_path, five.as_bytes());
        project.invalidate(&mesh);
    });
    for frame in 0..8 {
        project.frame(&mut world);
        for entity in [first, second] {
            assert_ne!(
                shown_vertices(&project, &world, entity),
                Some(6),
                "frame {frame}: the value in between"
            );
        }
        if shown_vertices(&project, &world, first) == Some(9) {
            assert_eq!(shown_vertices(&project, &world, second), Some(9));
            return;
        }
        project.decode();
    }
    panic!("the newest value never landed");
}

/// A file changed after the last reference to its asset left is not decoded again.
///
/// Its new value would replace a copy nothing draws, and the residency would
/// make that copy resident again.
#[test]
#[ignore = "a known defect: the reload decodes again an asset nothing names; the generic reload (phase G3) fixes it"]
fn a_changed_asset_no_reference_names_is_not_decoded_again() {
    let obj_path = "meshes/left.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    world.despawn(entity);
    project.tick(&mut world);
    project.watch::<Mesh>();

    project.write(obj_path, QUAD_OBJ.as_bytes());
    project.invalidate(&mesh);
    project.frame(&mut world);

    assert_eq!(project.decode(), 0, "no reference names the mesh");
    project.frame(&mut world);
    assert_eq!(
        project.arrived::<Mesh>(&mesh),
        Vec::<ArrivalKind>::new(),
        "no copy of it lands for the residency to make resident"
    );
}
