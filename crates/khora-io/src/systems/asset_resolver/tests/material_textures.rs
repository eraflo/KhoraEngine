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

//! Material textures: the resolver loads every texture a resolved material
//! lists into the shared `Assets<CpuTexture>`.

use std::fs;
use std::path::Path;

use khora_core::asset::InlineMaterial;
use khora_core::renderer::api::resource::CpuTexture;
use khora_telemetry::MetricsRegistry;
use tempfile::TempDir;

use super::*;
use crate::asset::{FileLoader, IndexBuilder};

/// A project assets dir on disk, indexed and served by a real `AssetService`.
///
/// The inventory decoders (texture + material), wired into a runtime
/// with a shared `AssetStore`.
struct Project {
    dir: TempDir,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    runtime: Runtime,
}

impl Project {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (rel, bytes) in files {
            write_file(dir.path(), rel, bytes);
        }
        let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            dir.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(dir.path())),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .unwrap();
        service.register_inventory_decoders();
        let service = Arc::new(Mutex::new(service));

        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(khora_core::lane::Reserve::new());
        runtime.services.insert(service.clone());
        Self {
            dir,
            service,
            store,
            runtime,
        }
    }

    /// One engine tick of the asset systems.
    ///
    /// The copies of changed assets are refreshed, then the authored
    /// refs are resolved — and the frames after it until what they
    /// asked for is decoded off the frame and taken in at a boundary.
    fn tick(&self, world: &mut World) {
        crate::systems::asset_intake::tests::settle(&self.runtime, world, |world| {
            run_system(world, &self.runtime, "asset_reload");
            run_system(world, &self.runtime, ASSET_RESOLVER);
        });
    }

    fn texture(&self, uuid: &AssetUUID) -> Option<AssetHandle<CpuTexture>> {
        self.store
            .store::<CpuTexture>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    fn load_count(&self) -> usize {
        self.service.lock().unwrap().load_count()
    }
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A real PNG of `width` x `height`, encoded in memory.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 100, 50, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

/// The `.kmat` RON form of a `StandardMaterial` whose base-colour slot
/// names `base_color_texture`.
fn kmat_with_base_color_texture(base_color_texture: AssetUUID) -> Vec<u8> {
    let tex = serde_json::to_value(base_color_texture)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("AssetUUID serializes to a string");
    format!(
        r#"{{
                    "type_name": "StandardMaterial",
                    "material": {{
                        "base_color": {{ "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 }},
                        "base_color_texture": "{tex}",
                        "metallic": 0.0,
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

#[test]
fn resolving_a_material_asset_loads_its_base_color_texture_into_the_store() {
    let tex_path = "textures/albedo.png";
    let tex = AssetUUID::new_v5(tex_path);
    let kmat_path = "materials/textured.kmat";
    let project = Project::new(&[
        (tex_path, png(3, 2)),
        (kmat_path, kmat_with_base_color_texture(tex)),
    ]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(AssetUUID::new_v5(
        kmat_path,
    )));

    project.tick(&mut world);

    assert!(
        stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity)).is_some(),
        "the material itself is in the store"
    );
    let texture = project
        .texture(&tex)
        .expect("the base-colour texture is loaded into Assets<CpuTexture>");
    assert_eq!(
        (texture.size.width, texture.size.height),
        (3, 2),
        "the stored texture is the decoded image the material names"
    );
}

#[test]
fn resolving_an_inline_material_loads_its_texture_into_the_store() {
    let tex_path = "textures/inline.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, png(4, 4))]);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);

    let texture = project
        .texture(&tex)
        .expect("an inline material's texture is loaded too");
    assert_eq!((texture.size.width, texture.size.height), (4, 4));
}

#[test]
fn resolving_a_material_loads_every_texture_slot_it_sets() {
    let paths = [
        "textures/base.png",
        "textures/mr.png",
        "textures/normal.png",
        "textures/emissive.png",
        "textures/ao.png",
    ];
    let files: Vec<(&str, Vec<u8>)> = paths.iter().map(|p| (*p, png(1, 1))).collect();
    let project = Project::new(&files);
    let [base, mr, normal, emissive, ao] = paths.map(AssetUUID::new_v5);
    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(base),
            metallic_roughness_texture: Some(mr),
            normal_map: Some(normal),
            emissive_texture: Some(emissive),
            occlusion_map: Some(ao),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);

    for (path, uuid) in paths.iter().zip([base, mr, normal, emissive, ao]) {
        assert!(
            project.texture(&uuid).is_some(),
            "{path} should be loaded into Assets<CpuTexture>"
        );
    }
}

#[test]
fn a_texture_already_in_the_store_is_not_reloaded() {
    let kept_path = "textures/kept.png";
    let fresh_path = "textures/fresh.png";
    let kept = AssetUUID::new_v5(kept_path);
    let fresh = AssetUUID::new_v5(fresh_path);
    let project = Project::new(&[(kept_path, png(8, 8)), (fresh_path, png(2, 2))]);

    // Something else already published `kept` (a 1x1 stand-in, unlike
    // the 8x8 file on disk).
    let already = AssetHandle::new(CpuTexture {
        pixels: vec![255; 4],
        size: khora_core::math::Extent3D {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        format: khora_core::renderer::api::resource::TextureFormat::Rgba8Unorm,
        mip_level_count: 1,
        sample_count: khora_core::renderer::api::resource::SampleCount::X1,
        dimension: khora_core::renderer::api::resource::TextureDimension::D2,
        usage: khora_core::renderer::api::resource::TextureUsage::COPY_DST
            | khora_core::renderer::api::resource::TextureUsage::TEXTURE_BINDING,
    });
    project
        .store
        .store::<CpuTexture>()
        .write()
        .unwrap()
        .insert(kept, already.clone());

    let mut world = World::new();
    world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(kept),
            normal_map: Some(fresh),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);

    assert!(
        project.texture(&fresh).is_some(),
        "the texture missing from the store is loaded"
    );
    assert!(
        project.texture(&kept) == Some(already),
        "the texture already in the store keeps its handle"
    );
    assert_eq!(
        project.load_count(),
        1,
        "only the missing texture goes through the AssetService"
    );
}

#[test]
fn a_texture_that_fails_to_load_is_skipped_and_the_other_slots_still_load() {
    let good_path = "textures/good.png";
    let corrupt_path = "textures/corrupt.png";
    let good = AssetUUID::new_v5(good_path);
    let corrupt = AssetUUID::new_v5(corrupt_path);
    // Named by the material but absent from the project index.
    let missing = AssetUUID::new_v5("textures/not_in_the_index.png");
    let project = Project::new(&[
        (good_path, png(2, 2)),
        (corrupt_path, b"definitely not an image".to_vec()),
    ]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(missing),
            metallic_roughness_texture: Some(corrupt),
            normal_map: Some(good),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);

    assert!(
        project.texture(&good).is_some(),
        "the slot that loads is in Assets<CpuTexture> despite its siblings failing"
    );
    assert!(project.texture(&missing).is_none());
    assert!(project.texture(&corrupt).is_none());
    assert!(
        stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity)).is_some(),
        "a texture failure does not stop the material from being stored"
    );
}

/// A texture whose decode failed is decoded again only once an asset changes.
///
/// Not on the next frames, nor when the material naming it is edited:
/// only a change to a file can fix it. Once its file changes, it loads.
#[test]
fn a_failed_texture_is_retried_once_an_asset_changes_not_when_its_material_ref_does() {
    let tex_path = "textures/late.png";
    let tex = AssetUUID::new_v5(tex_path);
    let project = Project::new(&[(tex_path, b"not yet an image".to_vec())]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);
    assert!(project.texture(&tex).is_none(), "undecodable bytes fail");

    // The file becomes a valid image, unannounced: a decode tried
    // again would find it.
    write_file(project.dir.path(), tex_path, &png(2, 2));
    project.tick(&mut world);
    assert!(
        project.texture(&tex).is_none(),
        "an unchanged material ref does not retry its failed texture"
    );

    // Editing the material (a new ref) resolves it again, but its
    // failed texture still waits for an asset to change.
    assert!(world.set_component(
        entity,
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            roughness: 0.9,
            ..Default::default()
        }))),
    ));
    project.tick(&mut world);
    assert!(
        project.texture(&tex).is_none(),
        "a changed material ref does not retry its failed texture"
    );

    // The file is announced changed, as the editor's hot reload does.
    project.service.lock().unwrap().invalidate(&tex);
    project.tick(&mut world);
    let texture = project
        .texture(&tex)
        .expect("the texture loads once its file is announced changed");
    assert_eq!((texture.size.width, texture.size.height), (2, 2));
}

/// End to end: once the resolver ran, every texture the resolved
/// material lists is in `Assets<CpuTexture>`.
///
/// The exact condition the GPU projection's `material_textures_ready`
/// waits on before it builds the material.
#[test]
fn a_resolved_textured_material_is_ready_to_project() {
    let base_path = "textures/base.png";
    let normal_path = "textures/normal.png";
    let base = AssetUUID::new_v5(base_path);
    let normal = AssetUUID::new_v5(normal_path);
    let project = Project::new(&[(base_path, png(2, 2)), (normal_path, png(2, 2))]);
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(
        Box::new(StandardMaterial {
            base_color_texture: Some(base),
            normal_map: Some(normal),
            ..Default::default()
        }),
    )));

    project.tick(&mut world);

    let resolved = stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity))
        .expect("the material is in the store");
    let material: &dyn Material = &**resolved;
    let listed = material.textures();
    assert_eq!(listed, vec![base, normal]);
    let cpu_textures = project.store.store::<CpuTexture>();
    let cpu_textures = cpu_textures.read().unwrap();
    assert!(
        listed.iter().all(|uuid| cpu_textures.contains(uuid)),
        "every texture the material lists is decoded, so it projects"
    );
}

/// A project opened like the editor and the runtime open one.
///
/// Loose files under `<root>/assets/`, every default decoder and the
/// glTF container registered, so a glTF's materials and textures are
/// sub-assets the resolver can load.
fn project_with_default_decoders(files: &[(&str, Vec<u8>)]) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    for (rel, bytes) in files {
        write_file(&root.join("assets"), rel, bytes);
    }
    let service = AssetService::open_loose_files(
        &root,
        &crate::asset::AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            &root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        )),
        Arc::new(MetricsRegistry::new()),
    )
    .expect("the project opens");
    let service = Arc::new(Mutex::new(service));
    let store = AssetStore::new();
    let mut runtime = Runtime::default();
    runtime.resources.insert(store.clone());
    runtime.resources.insert(khora_core::lane::Reserve::new());
    runtime.services.insert(service.clone());
    Project {
        dir,
        service,
        store,
        runtime,
    }
}

/// A `.gltf` with one material whose base colour is texture 0, an
/// image file `image` beside the glTF.
fn gltf_with_one_textured_material(image: &str) -> Vec<u8> {
    format!(
        r#"{{
                    "asset": {{ "version": "2.0" }},
                    "images": [ {{ "uri": "{image}" }} ],
                    "textures": [ {{ "source": 0 }} ],
                    "materials": [ {{
                        "name": "Painted",
                        "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }}
                    }} ]
                }}"#
    )
    .into_bytes()
}

#[test]
fn a_gltf_material_loads_its_texture_sub_asset_through_the_container() {
    let rel = "models/painted.gltf";
    let project = project_with_default_decoders(&[
        (rel, gltf_with_one_textured_material("painted.png")),
        ("models/painted.png", png(3, 2)),
    ]);
    let file = AssetUUID::new_v5(rel);
    let material = AssetUUID::sub_asset(file, "material0");
    let texture0 = AssetUUID::sub_asset(file, "texture0");
    let mut world = World::new();
    let entity = world.spawn(AssetRef::<Box<dyn Material>>::Asset(material));

    project.tick(&mut world);

    assert_eq!(material_uuid(&world, entity), material);
    let resolved = stored::<Box<dyn Material>>(&project.store, &material)
        .expect("the glTF material sub-asset is in the store");
    assert_eq!(
        resolved.textures(),
        vec![texture0],
        "the glTF material names its texture sub-asset"
    );
    let texture = project
        .texture(&texture0)
        .expect("the texture sub-asset is loaded into Assets<CpuTexture>");
    assert_eq!((texture.size.width, texture.size.height), (3, 2));
}

/// The editor's hot reload drops a modified texture from the
/// `AssetService` cache.
///
/// A material resolved after that must not be projected with the image
/// decoded from the file's former bytes.
#[test]
fn a_texture_modified_on_disk_is_served_fresh_once_the_material_ref_changes() {
    let tex_path = "textures/repainted.png";
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
    let before = project.texture(&tex).expect("the first image loads");
    assert_eq!((before.size.width, before.size.height), (2, 2));

    // Hot reload: the file changes and its cache entry is dropped.
    write_file(project.dir.path(), tex_path, &png(5, 5));
    assert!(project.service.lock().unwrap().invalidate(&tex));

    // The material is edited: a new ref, resolved again with its texture.
    assert!(world.set_component(
        entity,
        AssetRef::<Box<dyn Material>>::Local(InlineMaterial::new(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            roughness: 0.9,
            ..Default::default()
        }))),
    ));
    project.tick(&mut world);

    let after = project.texture(&tex).expect("the texture is in the store");
    assert_eq!(
        (after.size.width, after.size.height),
        (5, 5),
        "the store serves the image decoded from the file's current bytes"
    );
}
