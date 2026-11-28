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

//! Textures: a repainted texture replaced for every material naming it,
//! one that fails to decode keeping its last good image.

use super::*;

#[test]
fn a_repainted_texture_is_served_fresh_from_the_store() {
    let tex_path = "textures/repainted.png";
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
    let before = project.texture(&tex).expect("the first image loads");
    assert_eq!((before.size.width, before.size.height), (2, 2));

    project.write(tex_path, &png(5, 5));
    project.invalidate(&tex);
    project.tick(&mut world);

    let after = project
        .texture(&tex)
        .expect("the texture stays in the store");
    assert_eq!(
        (after.size.width, after.size.height),
        (5, 5),
        "the store serves the image decoded from the file's current bytes"
    );
}

/// A repainted texture is replaced once, for every material naming it.
///
/// That arrival is what the material residency makes each of them again
/// on; the materials themselves keep their copies and uuids.
#[test]
fn a_repainted_texture_is_replaced_for_every_material_naming_it() {
    let tex_path = "textures/shared.png";
    let tex = AssetUUID::new_v5(tex_path);
    let kmat_path = "materials/textured.kmat";
    let project = Project::new(&[(tex_path, png(2, 2)), (kmat_path, kmat(0.5, Some(tex)))]);
    let mut world = World::new();
    let by_asset = world.spawn(AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new_v5(
        kmat_path,
    )));
    let inline = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));
    project.tick(&mut world);
    let asset_material = material_uuid(&project, &world, by_asset);
    let inline_material = material_uuid(&project, &world, inline);
    project.watch::<CpuTexture>();
    project.watch::<Box<dyn Material>>();

    project.write(tex_path, &png(5, 5));
    project.invalidate(&tex);
    project.tick(&mut world);

    assert_eq!(
        project.arrived::<CpuTexture>(&tex),
        vec![ArrivalKind::Replaced],
        "the texture is replaced once, for every material naming it"
    );
    for (entity, material, what) in [
        (by_asset, asset_material, "the .kmat material"),
        (inline, inline_material, "the inline material"),
    ] {
        assert!(
            project.arrived::<Box<dyn Material>>(&material).is_empty(),
            "{what} itself did not change"
        );
        assert_eq!(
            material_uuid(&project, &world, entity),
            material,
            "{what} keeps its copy under the same uuid"
        );
    }
}

/// One texture fails to decode, a sibling repaints fine.
///
/// The broken one keeps its last good image, not replaced, so the material
/// naming it keeps its GPU copy, while the sibling is replaced.
#[test]
fn a_reload_that_fails_to_decode_keeps_the_last_good_value() {
    let broken_path = "textures/broken.png";
    let fine_path = "textures/fine.png";
    let broken = AssetUUID::new_v5(broken_path);
    let fine = AssetUUID::new_v5(fine_path);
    let project = Project::new(&[(broken_path, png(2, 2)), (fine_path, png(2, 2))]);
    let mut world = World::new();
    let names_broken = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(broken),
            ..Default::default()
        }),
    )));
    let names_fine = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(fine),
            ..Default::default()
        }),
    )));
    project.tick(&mut world);
    let last_good = project.texture(&broken).expect("the first image loads");
    project.watch::<CpuTexture>();

    project.write(broken_path, b"definitely not an image");
    project.write(fine_path, &png(5, 5));
    project.invalidate(&broken);
    project.invalidate(&fine);
    project.tick(&mut world);

    let repainted = project.texture(&fine).expect("the sibling stays stored");
    assert_eq!(
        (repainted.size.width, repainted.size.height),
        (5, 5),
        "the sibling that decodes is refreshed"
    );
    assert_eq!(
        project.arrived::<CpuTexture>(&fine),
        vec![ArrivalKind::Replaced]
    );
    assert!(
        project.texture(&broken) == Some(last_good),
        "the texture that fails to decode keeps its last good image"
    );
    assert!(
        project.arrived::<CpuTexture>(&broken).is_empty(),
        "it is not replaced: the material naming it keeps its GPU copy"
    );
    for entity in [names_broken, names_fine] {
        assert!(material_of(&project, &world, entity).is_some());
    }
}
