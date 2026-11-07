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

//! **The resolvers ask for assets; they never decode one on the frame.** An
//! entity whose mesh, material, font or image is not decoded yet stays
//! unresolved — its request filed once, not once a frame — and resolves once
//! the decode family's result was taken in at a boundary, to what a
//! synchronous load gave.
//!
//! The decode family's work is played by hand between frames (see
//! [`Project::background`](super::assets_requested_off_the_frame::Project)).

use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::asset::{AssetUUID, Material};
use khora_core::math::Vec2;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_data::ecs::{HandleComponent, MaterialRef, MeshRef, World};
use khora_data::ui::components::{UiImage, UiText, UiTransform};
use khora_io::asset::{AssetService, FileLoader, MeshDispatcher};
use khora_telemetry::MetricsRegistry;

use super::assets_requested_off_the_frame::Project;

const TRIANGLE_OBJ: &[u8] = b"v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";
const MESH: &str = "meshes/tri.obj";

/// The debug form of `uuid` loaded synchronously as an `A` by a service of
/// its own over `project`'s files: what the resolver gave before assets were
/// decoded off the frame.
fn loaded_synchronously<A: khora_core::asset::Asset + std::fmt::Debug>(
    project: &Project,
    uuid: AssetUUID,
) -> String {
    let mut service = AssetService::new(
        &project.index_bytes(),
        Box::new(FileLoader::new(project.root())),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .expect("a second service");
    service.register_inventory_decoders();
    service.register_decoder::<Mesh>("mesh", MeshDispatcher::default());
    let handle = service.load::<A>(&uuid).expect("the asset loads");
    format!("{:?}", *handle)
}

/// A real PNG of `width` x `height`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([10, 20, 30, 255]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("encodes a png");
    bytes
}

/// The `.kmat` form of a `StandardMaterial` whose base colour is the
/// texture `texture`.
fn kmat_with_base_color_texture(texture: AssetUUID) -> Vec<u8> {
    let texture = serde_json::to_value(texture)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .expect("an asset uuid serialises to a string");
    format!(
        r#"{{
            "type_name": "StandardMaterial",
            "material": {{
                "base_color": {{ "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 }},
                "base_color_texture": "{texture}",
                "metallic": 0.25,
                "roughness": 0.5,
                "metallic_roughness_texture": (),
                "normal_map": (),
                "occlusion_map": (),
                "emissive": {{ "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 }},
                "emissive_texture": (),
                "alpha_mode": "Opaque",
                "alpha_cutoff": 0.5,
                "double_sided": false,
            }},
        }}"#
    )
    .into_bytes()
}

fn ui_transform() -> UiTransform {
    UiTransform {
        pos: Vec2::new(0.0, 0.0),
        size: Vec2::new(100.0, 20.0),
        z_index: 0,
    }
}

/// Rounds of the decode family's run then a frame, until `done` holds — at
/// most `rounds`; whether it held.
fn settle(
    project: &Project,
    world: &mut World,
    rounds: usize,
    mut done: impl FnMut(&World) -> bool,
) -> bool {
    for _ in 0..rounds {
        project.background();
        project.frame(world);
        if done(world) {
            return true;
        }
    }
    false
}

// ─── Meshes ─────────────────────────────────────────────────────────────────

/// **A mesh asset stays unresolved while it decodes, and resolves once it
/// landed, to what a synchronous load gave.** The frame that asks leaves the
/// entity without a mesh handle and files one request; frames that ask
/// again while it is pending file nothing more. Once decoded and taken in,
/// the entity holds the mesh under the asset's uuid, and the store holds it.
#[test]
fn a_mesh_asset_resolves_once_its_decode_landed_and_not_before() {
    let project = Project::new(&[(MESH, TRIANGLE_OBJ)]);
    let uuid = AssetUUID::new_v5(MESH);
    let mut world = World::new();
    let entity = world.spawn(MeshRef::Asset(uuid));

    for _ in 0..4 {
        project.frame(&mut world);
        assert!(
            world.get::<HandleComponent<Mesh>>(entity).is_none(),
            "no mesh before its decode landed"
        );
    }
    let filed = project.filed();
    assert_eq!(filed.len(), 1, "one request for four frames: {filed:?}");
    // The request is decoded off the frame, and taken in at a boundary.
    let staging = project.decode(&filed);
    project.land(staging);
    project.frame(&mut world);

    let resolved = world
        .get::<HandleComponent<Mesh>>(entity)
        .expect("the mesh resolves once it landed");
    assert_eq!(resolved.uuid, uuid);
    assert_eq!(
        format!("{:?}", *resolved.handle),
        loaded_synchronously::<Mesh>(&project, uuid),
        "the mesh a synchronous load gave"
    );
    assert!(project.stored::<Mesh>(&uuid), "the store holds the mesh");
}

/// **A pending mesh is asked for once, not once a frame**: ten frames
/// before the decode family runs file one request.
#[test]
fn a_pending_mesh_is_requested_once_not_every_frame() {
    let project = Project::new(&[(MESH, TRIANGLE_OBJ)]);
    let mut world = World::new();
    world.spawn(MeshRef::Asset(AssetUUID::new_v5(MESH)));
    world.spawn(MeshRef::Asset(AssetUUID::new_v5(MESH)));

    for _ in 0..10 {
        project.frame(&mut world);
    }

    assert_eq!(
        project.filed().len(),
        1,
        "two entities, ten frames, one request"
    );
}

// ─── Materials and their textures ───────────────────────────────────────────

/// **A material asset and the texture it names resolve once decoded off the
/// frame**: the material handle reaches the entity, its texture reaches the
/// store — what the GPU projection waits on — within a few rounds of the
/// decode family, and neither on the frame that asked.
#[test]
fn a_material_asset_and_its_texture_resolve_once_decoded_off_the_frame() {
    let texture = AssetUUID::new_v5("textures/albedo.png");
    let material = AssetUUID::new_v5("materials/painted.kmat");
    let project = Project::new(&[
        ("textures/albedo.png", &png(3, 2)),
        (
            "materials/painted.kmat",
            &kmat_with_base_color_texture(texture),
        ),
    ]);
    let mut world = World::new();
    let entity = world.spawn(MaterialRef::Asset(material));

    project.frame(&mut world);
    assert!(
        world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .is_none(),
        "the frame that asks resolves nothing"
    );

    let resolved = settle(&project, &mut world, 5, |world| {
        world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .is_some()
            && project.stored::<CpuTexture>(&texture)
    });

    assert!(resolved, "the material and its texture resolve");
    let held = world
        .get::<HandleComponent<Box<dyn Material>>>(entity)
        .expect("the material");
    assert_eq!(held.uuid, material);
    assert_eq!(held.handle.textures(), vec![texture]);
    let stored = project
        .store
        .store::<CpuTexture>()
        .read()
        .expect("the store's lock")
        .get(&texture)
        .cloned()
        .expect("the texture");
    assert_eq!((stored.size.width, stored.size.height), (3, 2));
}

// ─── UI ─────────────────────────────────────────────────────────────────────

/// **A text's font resolves once decoded off the frame**: no handle on the
/// frame that asks, the font's handle and bytes once it landed.
#[test]
fn a_text_font_resolves_once_its_decode_landed() {
    let path = "fonts/body.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, b"font file bytes")]);
    let mut world = World::new();
    let entity = world.spawn((
        ui_transform(),
        UiText {
            content: "Hello".to_owned(),
            font,
            ..Default::default()
        },
    ));

    project.frame(&mut world);
    assert!(
        world.get::<HandleComponent<Font>>(entity).is_none(),
        "the frame that asks resolves nothing"
    );

    let resolved = settle(&project, &mut world, 3, |world| {
        world.get::<HandleComponent<Font>>(entity).is_some()
    });

    assert!(resolved, "the font resolves");
    let held = world
        .get::<HandleComponent<Font>>(entity)
        .expect("the font");
    assert_eq!(held.uuid, font);
    assert_eq!(held.handle.data, b"font file bytes");
    assert!(project.stored::<Font>(&font));
}

/// **A UI image lands in the store once decoded off the frame**, not on the
/// frame that asks, and is asked for once while pending.
#[test]
fn a_ui_image_lands_in_the_store_once_decoded() {
    let path = "ui/icon.png";
    let texture = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, &png(3, 2))]);
    let mut world = World::new();
    world.spawn((ui_transform(), UiImage { texture }));

    for _ in 0..3 {
        project.frame(&mut world);
    }
    assert!(
        !project.stored::<CpuTexture>(&texture),
        "no image before its decode landed"
    );
    let filed = project.filed();
    assert_eq!(filed.len(), 1, "one request for three frames: {filed:?}");
    let staging = project.decode(&filed);
    project.land(staging);
    project.frame(&mut world);

    assert!(
        project.stored::<CpuTexture>(&texture),
        "the image reaches the store once its decode landed"
    );
}

/// **A font whose decode failed is not asked for again on every frame.**
#[test]
fn a_text_font_whose_decode_failed_is_not_requested_every_frame() {
    let path = "fonts/gone.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, b"font bytes")]);
    project.remove(path);
    let mut world = World::new();
    let entity = world.spawn((
        ui_transform(),
        UiText {
            content: "Hello".to_owned(),
            font,
            ..Default::default()
        },
    ));

    let mut decoded = 0;
    for _ in 0..10 {
        project.frame(&mut world);
        decoded += project.background();
    }

    assert_eq!(
        decoded, 1,
        "the failed font is decoded once, not once a frame"
    );
    assert!(project.svc().has_failed(&font));
    assert!(world.get::<HandleComponent<Font>>(entity).is_none());
}
