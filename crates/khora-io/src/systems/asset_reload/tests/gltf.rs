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

//! glTF: a re-exported file, a repainted image or a rewritten buffer
//! refreshing the parts read from it.

use super::*;

#[test]
fn a_reexported_gltf_refreshes_its_parts() {
    let rel = "models/shape.gltf";
    let bin = "shape.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], bin);
    let project = Project::new(&[(rel, gltf), ("models/shape.bin", buffer)]);
    let file = AssetUUID::new_v5(rel);
    let mesh = AssetUUID::sub_asset(file, "mesh0/prim0");
    let material = AssetUUID::sub_asset(file, "material0");
    let mut world = World::new();
    let entity = world.spawn((
        AssetRef::<Mesh>::Asset(mesh),
        AssetRef::<Box<dyn Material>>::Asset(material),
    ));
    project.tick(&mut world);
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh part resolves")
            .positions
            .len(),
        3
    );
    assert_eq!(standard_of(&project, &world, entity).base_color.g, 0.0);

    let (gltf, buffer) = gltf_shape(&TWO_TRIANGLES, [0.0, 1.0, 0.0, 1.0], bin);
    project.write(rel, &gltf);
    project.write("models/shape.bin", &buffer);
    project.invalidate(&file);
    project.tick(&mut world);

    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh part stays resolved")
            .positions
            .len(),
        6,
        "the mesh part is the re-exported one"
    );
    assert_eq!(
        standard_of(&project, &world, entity).base_color.g,
        1.0,
        "the material part is the re-exported one"
    );
}

/// Lists the assets folder again, as the editor's hot reload does after a
/// file that can hold parts was modified.
fn reindex(project: &Project) {
    let bytes = IndexBuilder::new(crate::trust::FileGuard::new(
        project.root.join("assets"),
        crate::trust::Trust::Project,
        crate::trust::Refusals::detached(),
    ))
    .with_registry(&AssetIdRegistry::load_or_unread(
        &crate::trust::FileGuard::new(
            &project.root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ),
    ))
    .build_index_bytes()
    .unwrap();
    project.service.lock().unwrap().reindex(&bytes).unwrap();
}

/// A `.gltf` with one material whose base colour is texture 0, read from
/// the image file `image` beside the glTF.
fn gltf_with_one_textured_material(image: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "asset": { "version": "2.0" },
        "images": [ { "uri": image } ],
        "textures": [ { "source": 0 } ],
        "materials": [ {
            "name": "Painted",
            "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } }
        } ],
    }))
    .unwrap()
}

/// A glTF saved again with a repainted image replaces its texture part.
///
/// Replaced in the store when the container is invalidated: the arrival
/// the material residency makes the material naming it again on.
#[test]
fn a_reexported_gltf_refreshes_its_texture_part_for_its_material() {
    let rel = "models/painted.gltf";
    let image = "models/painted.png";
    let project = Project::new(&[
        (rel, gltf_with_one_textured_material("painted.png")),
        (image, png(3, 2)),
    ]);
    let file = AssetUUID::new_v5(rel);
    let material = AssetUUID::sub_asset(file, "material0");
    let texture0 = AssetUUID::sub_asset(file, "texture0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(material));
    project.tick(&mut world);
    let before = project.texture(&texture0).expect("the texture part loads");
    assert_eq!((before.size.width, before.size.height), (3, 2));
    project.watch::<CpuTexture>();

    project.write(image, &png(5, 5));
    project.write(rel, &gltf_with_one_textured_material("painted.png"));
    project.invalidate(&file);
    reindex(&project);
    project.tick(&mut world);

    let after = project
        .texture(&texture0)
        .expect("the texture part stays in the store");
    assert_eq!(
        (after.size.width, after.size.height),
        (5, 5),
        "the texture part is the re-exported image"
    );
    assert_eq!(
        project.arrived::<CpuTexture>(&texture0),
        vec![ArrivalKind::Replaced],
        "the texture part the material names is replaced"
    );
    assert_eq!(material_uuid(&project, &world, entity), material);
}

/// A glTF's image is a file of its own: repainting it, and nothing else,
/// changes what the glTF's texture part decodes to.
///
/// The editor's hot reload invalidates only the image file it saw change;
/// the part read from it, and the material naming that part, must follow.
#[test]
fn a_repainted_image_a_gltf_points_at_refreshes_the_gltf_texture_part() {
    let rel = "models/painted.gltf";
    let image = "models/painted.png";
    let project = Project::new(&[
        (rel, gltf_with_one_textured_material("painted.png")),
        (image, png(3, 2)),
    ]);
    let file = AssetUUID::new_v5(rel);
    let material = AssetUUID::sub_asset(file, "material0");
    let texture0 = AssetUUID::sub_asset(file, "texture0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(material));
    project.tick(&mut world);
    let before = project.texture(&texture0).expect("the texture part loads");
    assert_eq!((before.size.width, before.size.height), (3, 2));
    project.watch::<CpuTexture>();

    project.write(image, &png(5, 5));
    project.invalidate(&AssetUUID::new_v5(image));
    project.tick(&mut world);

    let after = project
        .texture(&texture0)
        .expect("the texture part stays in the store");
    assert_eq!(
        (after.size.width, after.size.height),
        (5, 5),
        "the glTF's texture part shows the repainted image"
    );
    assert_eq!(
        project.arrived::<CpuTexture>(&texture0),
        vec![ArrivalKind::Replaced],
        "the texture part is replaced: the glTF material naming it is made again"
    );
    assert_eq!(material_uuid(&project, &world, entity), material);
}

/// An exporter caught mid-write leaves a glTF the index cannot read, so it
/// lists the file without its parts.
///
/// That is a file that failed to decode, not parts the author removed: the
/// entities keep the last good mesh and material until the file is whole
/// again.
#[test]
fn a_gltf_caught_mid_write_keeps_its_parts_on_the_entities() {
    let rel = "models/shape.gltf";
    let bin = "shape.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], bin);
    let project = Project::new(&[(rel, gltf.clone()), ("models/shape.bin", buffer)]);
    let file = AssetUUID::new_v5(rel);
    let mesh = AssetUUID::sub_asset(file, "mesh0/prim0");
    let material = AssetUUID::sub_asset(file, "material0");
    let mut world = World::new();
    let entity = world.spawn((
        AssetRef::<Mesh>::Asset(mesh),
        AssetRef::<Box<dyn Material>>::Asset(material),
    ));
    project.tick(&mut world);
    project.watch::<Mesh>();

    // What the editor's hot reload does on a modified glTF: invalidate
    // it, then list its parts again since a glTF may hold some.
    project.write(rel, &gltf[..gltf.len() / 2]);
    project.invalidate(&file);
    reindex(&project);
    project.tick(&mut world);

    let resolved =
        mesh_of(&project, &world, entity).expect("the entity keeps the last good mesh part");
    assert_eq!(resolved.positions.len(), 3);
    assert!(
        material_of(&project, &world, entity).is_some(),
        "the entity keeps the last good material part"
    );
    assert!(
        project.arrived::<Mesh>(&mesh).is_empty(),
        "the last good mesh part is not replaced: its GPU copy stays"
    );
}

/// A glTF placed before the image it names: its material resolves, its
/// texture part does not.
///
/// The image then arrives as a new file, which the editor's hot reload
/// invalidates before it lists the assets folder again; the texture part
/// read from that image must load.
#[test]
fn an_image_arriving_after_a_gltf_material_resolved_loads_its_texture_part() {
    let rel = "models/painted.gltf";
    let image = "models/painted.png";
    let project = Project::new(&[(rel, gltf_with_one_textured_material("painted.png"))]);
    let file = AssetUUID::new_v5(rel);
    let material = AssetUUID::sub_asset(file, "material0");
    let texture0 = AssetUUID::sub_asset(file, "texture0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(material));
    project.tick(&mut world);
    assert!(
        material_of(&project, &world, entity).is_some(),
        "the premise: the material part resolves without its image"
    );
    assert!(
        project.texture(&texture0).is_none(),
        "the premise: the texture part has no image to read yet"
    );

    // What the editor's hot reload does on a created file: invalidate
    // it, then list the assets folder again.
    project.write(image, &png(3, 2));
    project.invalidate_file(image);
    reindex(&project);
    project.tick(&mut world);

    let texture = project
        .texture(&texture0)
        .expect("the texture part loads once its image exists");
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

/// A glTF named whole as one mesh reads its vertices from the buffer file
/// beside it.
///
/// Rewriting that file, and nothing else, changes the mesh, and the entity
/// must show the rewritten vertices.
#[test]
fn a_rewritten_buffer_refreshes_a_gltf_resolved_whole_as_a_mesh() {
    let rel = "models/shape.gltf";
    let bin = "models/shape.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "shape.bin");
    let project = Project::new(&[(rel, gltf), (bin, buffer)]);
    let file = AssetUUID::new_v5(rel);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(file));
    project.tick(&mut world);
    let before = mesh_of(&project, &world, entity)
        .expect("the glTF resolves as one mesh")
        .positions
        .clone();

    // Same vertex count and byte length, other positions: only the
    // buffer file changes.
    let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shape.bin");
    project.write(bin, &buffer);
    project.invalidate_file(bin);
    project.tick(&mut world);

    let after = &mesh_of(&project, &world, entity)
        .expect("the mesh stays resolved")
        .positions;
    assert_ne!(
        &before, after,
        "the mesh shows the vertices of the rewritten buffer"
    );
}

/// Two glTFs read their vertices from one buffer file: rewriting it
/// changes the mesh part of each, and both entities must follow.
#[test]
fn a_buffer_two_gltfs_share_refreshes_the_mesh_part_of_each() {
    let bin = "models/shared.bin";
    let (first, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "shared.bin");
    let (second, _) = gltf_shape(&TRIANGLE, [0.0, 1.0, 0.0, 1.0], "shared.bin");
    let project = Project::new(&[
        ("models/first.gltf", first),
        ("models/second.gltf", second),
        (bin, buffer),
    ]);
    let mut world = World::new();
    let entities: Vec<EntityId> = ["models/first.gltf", "models/second.gltf"]
        .iter()
        .map(|rel| {
            let mesh = AssetUUID::sub_asset(AssetUUID::new_v5(rel), "mesh0/prim0");
            world.spawn(AssetRef::<Mesh>::Asset(mesh))
        })
        .collect();
    project.tick(&mut world);

    let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shared.bin");
    project.write(bin, &buffer);
    project.invalidate_file(bin);
    project.tick(&mut world);

    for entity in entities {
        let positions = &mesh_of(&project, &world, entity)
            .expect("the mesh part stays resolved")
            .positions;
        assert_eq!(positions[1].x, 2.0, "the rewritten buffer's vertices");
    }
}

/// A buffer saved broken leaves its glTF unable to open: the parts keep
/// their last good value.
///
/// Once the buffer is saved whole, the glTF's parts must show it, though
/// only the buffer file changed each time.
#[test]
fn a_buffer_repaired_after_a_broken_save_refreshes_the_gltf_parts() {
    let rel = "models/shape.gltf";
    let bin = "models/shape.bin";
    let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "shape.bin");
    let project = Project::new(&[(rel, gltf), (bin, buffer.clone())]);
    let mesh = AssetUUID::sub_asset(AssetUUID::new_v5(rel), "mesh0/prim0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Mesh>::Asset(mesh));
    project.tick(&mut world);

    project.write(bin, &buffer[..buffer.len() / 2]);
    project.invalidate_file(bin);
    project.tick(&mut world);
    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the premise: the last good part stays")
            .positions[1]
            .x,
        1.0
    );

    let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    let (_, repaired) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shape.bin");
    project.write(bin, &repaired);
    project.invalidate_file(bin);
    project.tick(&mut world);

    assert_eq!(
        mesh_of(&project, &world, entity)
            .expect("the mesh part stays resolved")
            .positions[1]
            .x,
        2.0,
        "the mesh part reads the repaired buffer"
    );
}
