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

//! Tests of the asset reload, by behaviour; the fixtures they share live
//! here: a project on disk served into a runtime, the copies its entities
//! name, and the files written into it.

mod gltf;
mod loading_family;
mod materials;
mod meshes;
mod recovery;
mod registration;
mod steady_state;
mod textures;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::asset::{
    AssetHandle, AssetUUID, InlineMaterial, Material, StandardMaterial, Stored,
};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{OutputDeck, Reserve};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::Runtime;
use khora_data::assets::{ArrivalKind, ArrivalsRead};
use khora_data::ecs::{AssetRef, DataSystemRegistration, Observer, SystemContext, World};
use khora_data::AssetStore;
use khora_telemetry::MetricsRegistry;
use tempfile::TempDir;

use crate::asset::{AssetIdRegistry, AssetService, IndexBuilder};
/// The arrivals reader these tests read the store as.
const WATCHER: &str = "asset_reload_tests";

/// The registered data system called `name`.
fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// A project opened as the editor and the runtime open one.
///
/// Loose files under `<root>/assets/`, every default decoder and the glTF
/// container registered, served into a runtime with a shared `AssetStore`.
struct Project {
    _dir: TempDir,
    root: PathBuf,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    runtime: Runtime,
}

impl Project {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        for (rel, bytes) in files {
            write_file(&root.join("assets"), rel, bytes);
        }
        let service = AssetService::open_loose_files(
            &root,
            &AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
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
        runtime.resources.insert(Reserve::new());
        runtime.services.insert(service.clone());
        Self {
            _dir: dir,
            root,
            service,
            store,
            runtime,
        }
    }

    /// One engine tick of the asset systems.
    ///
    /// The reload, then the resolver — and the frames after it until what
    /// they asked for is decoded off the frame and taken in at a boundary.
    fn tick(&self, world: &mut World) {
        crate::systems::asset_intake::tests::settle(&self.runtime, world, |world| {
            self.run_system(world, "asset_reload");
            self.run_system(world, "asset_resolver");
        });
    }

    /// Runs the registered data system `name` once, as the scheduler's runner does.
    ///
    /// It is handed the tick of its last run; its writes carry this one.
    fn run_system(&self, world: &mut World, name: &'static str) {
        let observer = Observer::DataSystem(name);
        let ctx = SystemContext {
            since: world.observer_last_seen(observer),
            resumed: world.observer_resumes(observer),
            scope: None,
        };
        let tick = world.begin_run();
        (data_system(name).run)(world, &self.runtime, &mut OutputDeck::default(), ctx);
        world.note_observer_run(observer, tick);
        world.end_run();
    }

    /// One frame of the asset systems and nothing more: the boundary's intake of what the
    /// `loading` family landed, then the `PreExtract` reload and resolver.
    ///
    /// Nothing is decoded during it: what it files waits for
    /// [`Self::decode`].
    fn frame(&self, world: &mut World) {
        crate::systems::asset_intake::take_in_decoded(&self.runtime);
        self.run_system(world, "asset_reload");
        self.run_system(world, "asset_resolver");
    }

    /// One run of the `loading` family between two frames: every decode
    /// filed, decoded and received under its lease.
    ///
    /// Returns how many requests it took.
    fn decode(&self) -> usize {
        crate::systems::asset_intake::tests::decode_filed(&self.runtime)
    }

    /// [`Self::decode`], with `during` done by the host once the decodes
    /// ran and before the frame start receives them.
    fn decode_while(&self, during: impl FnOnce()) -> usize {
        crate::systems::asset_intake::tests::decode_filed_while(&self.runtime, during)
    }

    /// Rewrites the file at `rel` under the assets folder.
    fn write(&self, rel: &str, bytes: &[u8]) {
        write_file(&self.root.join("assets"), rel, bytes);
    }

    /// Deletes the file at `rel` and lists the assets folder again, as the
    /// editor's hot reload does when a file goes away.
    fn remove_and_reindex(&self, rel: &str) {
        fs::remove_file(self.root.join("assets").join(rel)).unwrap();
        let bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            self.root.join("assets"),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .with_registry(&AssetIdRegistry::load_or_unread(
            &crate::trust::FileGuard::new(
                &self.root,
                crate::trust::Trust::Project,
                crate::trust::Refusals::detached(),
            ),
        ))
        .build_index_bytes()
        .unwrap();
        self.service.lock().unwrap().reindex(&bytes).unwrap();
    }

    /// Invalidates the asset `uuid`, a file the index lists.
    fn invalidate(&self, uuid: &AssetUUID) {
        self.service.lock().unwrap().invalidate(uuid);
    }

    /// What the editor's hot reload does when the file at `rel` changed:
    /// invalidates it through the project's identity registry.
    fn invalidate_file(&self, rel: &str) {
        let registry = AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            &self.root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ));
        (self.service.lock().unwrap()).invalidate_through(rel, &|path| registry.resolve(path));
    }

    fn load_count(&self) -> usize {
        self.service.lock().unwrap().load_count()
    }

    fn texture(&self, uuid: &AssetUUID) -> Option<AssetHandle<CpuTexture>> {
        self.store
            .store::<CpuTexture>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    fn stored_material(&self, uuid: &AssetUUID) -> Option<AssetHandle<Box<dyn Material>>> {
        self.store
            .store::<Box<dyn Material>>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    fn stored_mesh(&self, uuid: &AssetUUID) -> Option<AssetHandle<Mesh>> {
        self.store
            .store::<Mesh>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    /// Starts reading the arrivals of `Assets<A>`: [`Self::arrived`]
    /// returns what lands after this call.
    fn watch<A: Stored>(&self) {
        let _ = self.store.store::<A>().read().unwrap().arrivals(WATCHER);
    }

    /// What happened, since the last [`Self::watch`] or call, to the
    /// copy of `uuid` in `Assets<A>`: the arrivals the residencies act on.
    fn arrived<A: Stored>(&self, uuid: &AssetUUID) -> Vec<ArrivalKind> {
        match self.store.store::<A>().read().unwrap().arrivals(WATCHER) {
            ArrivalsRead::Since(arrivals) => arrivals
                .into_iter()
                .filter(|arrival| arrival.uuid == *uuid)
                .map(|arrival| arrival.kind)
                .collect(),
            ArrivalsRead::Resync => panic!("the store is watched first"),
        }
    }
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// The copy of the material `entity`'s `AssetRef<Box<dyn Material>>` names that the store holds, if any.
fn material_of(
    project: &Project,
    world: &World,
    entity: EntityId,
) -> Option<AssetHandle<Box<dyn Material>>> {
    let uuid = world.get::<AssetRef<Box<dyn Material>>>(entity)?.uuid();
    project.stored_material(&uuid)
}

/// The copy of the mesh `entity`'s `AssetRef<Mesh>` names that the store holds, if any.
fn mesh_of(project: &Project, world: &World, entity: EntityId) -> Option<AssetHandle<Mesh>> {
    let uuid = world.get::<AssetRef<Mesh>>(entity)?.uuid();
    project.stored_mesh(&uuid)
}

/// The uuid `entity`'s `AssetRef<Box<dyn Material>>` names, whose copy the store holds.
fn material_uuid(project: &Project, world: &World, entity: EntityId) -> AssetUUID {
    let uuid = world
        .get::<AssetRef<Box<dyn Material>>>(entity)
        .expect("the entity has an AssetRef<Box<dyn Material>>")
        .uuid();
    assert!(
        project.stored_material(&uuid).is_some(),
        "the material resolved into the store"
    );
    uuid
}

/// The `StandardMaterial` the store holds under `entity`'s `AssetRef<Box<dyn Material>>`.
fn standard_of(project: &Project, world: &World, entity: EntityId) -> StandardMaterial {
    let resolved = material_of(project, world, entity).expect("the material resolved");
    let material: &dyn Material = &**resolved;
    material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .expect("a StandardMaterial")
        .clone()
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

/// The `.kmat` RON form of a `StandardMaterial` of `roughness`, whose
/// base-colour slot names `base_color_texture` when it is `Some`.
fn kmat(roughness: f32, base_color_texture: Option<AssetUUID>) -> Vec<u8> {
    let texture = match base_color_texture {
        Some(uuid) => {
            let tex = serde_json::to_value(uuid)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .expect("AssetUUID serializes to a string");
            format!("\"{tex}\"")
        }
        None => "()".to_owned(),
    };
    format!(
        r#"{{
                "type_name": "StandardMaterial",
                "material": {{
                    "base_color": {{ "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 }},
                    "base_color_texture": {texture},
                    "metallic": 0.0,
                    "roughness": {roughness:?},
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

const TRIANGLE_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
/// A quad: the decoder gives each of its two triangles its own vertices.
const QUAD_OBJ: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";

/// A `.gltf` whose one mesh is a non-indexed list of `corners`, drawn in
/// its one material of base colour `color`.
///
/// The vertex bytes are in the buffer file `bin` beside it.
fn gltf_shape(corners: &[[f32; 3]], color: [f32; 4], bin: &str) -> (Vec<u8>, Vec<u8>) {
    let mut bytes = Vec::new();
    for corner in corners {
        for c in corner {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
    }
    let axis = |i: usize, pick: fn(f32, f32) -> f32| {
        corners.iter().map(|c| c[i]).fold(corners[0][i], pick)
    };
    let min = [axis(0, f32::min), axis(1, f32::min), axis(2, f32::min)];
    let max = [axis(0, f32::max), axis(1, f32::max), axis(2, f32::max)];
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [ { "uri": bin, "byteLength": bytes.len() } ],
        "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": bytes.len() } ],
        "accessors": [ {
            "bufferView": 0,
            "componentType": 5126,
            "count": corners.len(),
            "type": "VEC3",
            "min": min,
            "max": max,
        } ],
        "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 }, "material": 0 } ] } ],
        "materials": [ { "name": "Paint", "pbrMetallicRoughness": { "baseColorFactor": color } } ],
        "nodes": [ { "mesh": 0 } ],
        "scenes": [ { "nodes": [0] } ],
        "scene": 0,
    });
    (serde_json::to_vec(&json).unwrap(), bytes)
}

const TRIANGLE: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const TWO_TRIANGLES: [[f32; 3]; 6] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];

/// The uuid `entity`'s `AssetRef<Mesh>` names.
fn mesh_ref_uuid(world: &World, entity: EntityId) -> AssetUUID {
    world
        .get::<AssetRef<Mesh>>(entity)
        .expect("the entity has an AssetRef<Mesh>")
        .uuid()
}
