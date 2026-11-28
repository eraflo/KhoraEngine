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

//! Materials: an edited `.kmat` reaching its entities under the same uuid,
//! with the textures it names.

use super::*;

#[test]
fn an_edited_kmat_reaches_its_entities_under_the_same_uuid() {
    let kmat_path = "materials/edited.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    assert_eq!(standard_of(&project, &world, entity).roughness, 0.5);
    project.watch::<Box<dyn Material>>();

    project.write(kmat_path, &kmat(0.9, None));
    project.invalidate(&mat);
    project.tick(&mut world);

    assert_eq!(
        material_uuid(&project, &world, entity),
        mat,
        "the same uuid"
    );
    assert_eq!(
        standard_of(&project, &world, entity).roughness,
        0.9,
        "the entity's material is the edited one"
    );
    let stored = project.stored_material(&mat).expect("the store holds it");
    let stored: &dyn Material = &**stored;
    assert_eq!(
        stored
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("a StandardMaterial")
            .roughness,
        0.9,
        "the store serves the edited material"
    );
    assert_eq!(
        project.arrived::<Box<dyn Material>>(&mat),
        vec![ArrivalKind::Replaced],
        "replaced in the store: the residency makes its GPU copy again"
    );
}

#[test]
fn an_edited_kmat_loads_the_textures_it_now_names() {
    let kmat_path = "materials/gains_a_texture.kmat";
    let tex_path = "textures/new.png";
    let mat = AssetUUID::new_v5(kmat_path);
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None)), (tex_path, png(3, 2))]);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat));
    project.tick(&mut world);
    assert!(project.texture(&tex).is_none(), "nothing names it yet");

    project.write(kmat_path, &kmat(0.5, Some(tex)));
    project.invalidate(&mat);
    project.tick(&mut world);

    let texture = project
        .texture(&tex)
        .expect("the texture the edited material names is loaded");
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

#[test]
fn an_asset_shared_by_many_entities_is_decoded_once() {
    let kmat_path = "materials/shared.kmat";
    let mat = AssetUUID::new_v5(kmat_path);
    let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
    let mut world = World::new();
    let entities: Vec<EntityId> = (0..3)
        .map(|_| world.spawn(AssetRef::<Box<dyn Material>>::Asset(mat)))
        .collect();
    project.tick(&mut world);
    let before = project.load_count();

    project.write(kmat_path, &kmat(0.9, None));
    project.invalidate(&mat);
    project.tick(&mut world);

    assert_eq!(
        project.load_count(),
        before + 1,
        "one decode however many entities name it"
    );
    for entity in entities {
        assert_eq!(standard_of(&project, &world, entity).roughness, 0.9);
    }
}
