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

//! After a failure or a hand-over: a reload that failed, a texture fixed,
//! another service moving in, a change buried under a backlog.

use super::*;

/// A `.kmat` saved broken keeps its last good value on the entities that
/// hold it.
///
/// An entity that names the same asset afterwards (spawned, or restored
/// with its scene) must see that same value, not nothing.
#[test]
fn an_entity_naming_a_material_kept_after_a_failed_reload_sees_the_last_good_value() {
    let kmat_path = "materials/kept.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let first = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);

    project.write(kmat_path, b"definitely not a material");
    project.invalidate(&mat);
    project.tick(&mut world);
    assert_eq!(
        standard_of(&project, &world, first).roughness,
        0.5,
        "the premise: the entity holding it keeps the last good value"
    );

    let later = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);

    assert!(
        material_of(&project, &world, later).is_some(),
        "the later entity resolves the material the first one shows"
    );
    assert_eq!(standard_of(&project, &world, later).roughness, 0.5);
}

/// A texture that did not decode when its material resolved comes back
/// once its file is fixed and announced, without the material changing.
#[test]
fn a_texture_that_failed_to_load_comes_back_once_its_file_is_fixed() {
    let tex_path = "textures/late.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, b"not yet an image".to_vec())]);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));
    project.tick(&mut world);
    assert!(project.texture(&tex).is_none(), "undecodable bytes fail");

    project.write(tex_path, &png(2, 2));
    project.invalidate(&tex);
    project.tick(&mut world);

    let texture = project
        .texture(&tex)
        .expect("the fixed texture is loaded for the material naming it");
    assert_eq!((texture.size.width, texture.size.height), (2, 2));
}

/// Another project's service moves in under the same uuids.
///
/// A mesh both projects hold at one path reaches the entity with the
/// incoming project's contents.
#[test]
fn a_service_moving_in_refreshes_the_copies_the_outgoing_one_served() {
    let obj_path = "meshes/shared.obj";
    let mesh = AssetUUID::new_v5(obj_path);
    let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);
    project.watch::<Mesh>();

    let other = tempfile::tempdir().unwrap();
    write_file(&other.path().join("assets"), obj_path, QUAD_OBJ.as_bytes());
    let mut incoming = AssetService::open_loose_files(
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
        let mut shared = project.service.lock().unwrap();
        std::mem::swap(&mut *shared, &mut incoming);
        shared.announce_replacing(&incoming);
    }
    project.tick(&mut world);

    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh stays resolved")
            .positions
            .len(),
        6,
        "the incoming project's mesh"
    );
    assert_eq!(project.arrived::<Mesh>(&mesh), vec![ArrivalKind::Replaced]);
}

/// Many files change between two ticks (a branch checkout, a bulk
/// re-export).
///
/// A change announced early, then buried under a full backlog of later
/// ones, must still reach the copy the engine holds.
#[test]
fn a_change_buried_under_a_full_backlog_still_reaches_its_copy() {
    use crate::asset::CHANGE_BACKLOG;

    let tex_path = "textures/early.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, png(2, 2))]);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));
    project.tick(&mut world);

    project.write(tex_path, &png(5, 5));
    project.invalidate(&tex);
    for i in 0..CHANGE_BACKLOG {
        project.invalidate(&AssetUUID::new_v5(&format!("other/{i}.png")));
    }
    project.tick(&mut world);

    let after = project.texture(&tex).expect("the texture stays stored");
    assert_eq!(
        (after.size.width, after.size.height),
        (5, 5),
        "the early change was not lost"
    );
}
