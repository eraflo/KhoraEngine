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

//! Changed asset → refreshed copies.
//!
//! A `PreExtract` data system that drains the
//! [`AssetService`](crate::asset::AssetService)'s
//! [`changes`](crate::asset::AssetService::changes) each tick and replaces
//! every copy the engine took of a changed asset — the `AssetStore` CPU
//! sub-stores, the entities' resolved handles — under the same uuid, then
//! retires the stale GPU projection so it is built again from the new value.
//! Runs before the [`asset_resolver`](super::asset_resolver).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::asset::{Asset, AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::Runtime;
use khora_data::ecs::{Component, DataSystemRegistration, HandleComponent, TickPhase, World};
use khora_data::{AssetEviction, AssetStore};

use super::asset_resolver::load_textures;
use crate::asset::AssetService;

/// What decoding a changed asset again gave.
enum Reloaded<A: Asset> {
    /// The asset's current value.
    Fresh(AssetHandle<A>),
    /// The asset left the index (its file was removed, or its container no
    /// longer holds it): every copy goes, as it would after a restart.
    Gone,
    /// The asset is still part of the project but did not decode (a file
    /// caught mid-write): the last good copy stays, in the service too,
    /// until the next change.
    Kept,
}

// Written out rather than derived: a derive would demand `A: Clone`, which
// an `AssetHandle` never needs.
impl<A: Asset> Clone for Reloaded<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Fresh(handle) => Self::Fresh(handle.clone()),
            Self::Gone => Self::Gone,
            Self::Kept => Self::Kept,
        }
    }
}

/// The services one reload pass works through.
struct Reload<'a> {
    service: &'a Mutex<AssetService>,
    store: Option<&'a AssetStore>,
    eviction: Option<&'a Mutex<AssetEviction>>,
    /// The uuids whose GPU projection was already taken out this tick.
    retired: HashSet<AssetUUID>,
}

fn asset_reload_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(service) = runtime.services.get::<Arc<Mutex<AssetService>>>() else {
        return;
    };
    // Steady state: one lock and an empty stream.
    let changed = match service.lock() {
        Ok(svc) => svc.take_changes(),
        Err(_) => {
            log::error!("asset_reload: AssetService mutex poisoned.");
            return;
        }
    };
    if changed.is_empty() {
        return;
    }

    let mut reload = Reload {
        service,
        store: runtime.resources.get::<AssetStore>(),
        eviction: runtime
            .resources
            .get::<Arc<Mutex<AssetEviction>>>()
            .map(|eviction| &**eviction),
        retired: HashSet::new(),
    };
    let refreshed = reload.textures(&changed);
    let held_materials = reload.materials(world, &changed, &refreshed);
    let held_meshes = reload.meshes(world, &changed);

    // A changed asset no entity holds keeps no copy: the resolver decodes it
    // afresh when a reference names it again.
    for uuid in &changed {
        if held_materials.contains(uuid) || held_meshes.contains(uuid) {
            continue;
        }
        reload.forget::<Box<dyn Material>>(uuid);
        reload.forget::<Mesh>(uuid);
        reload.retire(uuid);
    }
}

impl Reload<'_> {
    /// Decodes `uuid` again through the service, whose copy was dropped when
    /// the change was announced. `last_good` is the copy the engine shows.
    fn reload<A: Asset>(&self, uuid: &AssetUUID, last_good: &AssetHandle<A>) -> Reloaded<A> {
        let Ok(mut svc) = self.service.lock() else {
            log::error!("asset_reload: AssetService mutex poisoned.");
            return Reloaded::Kept;
        };
        match svc.load::<A>(uuid) {
            Ok(handle) => Reloaded::Fresh(handle),
            Err(e) if svc.is_gone(uuid) => {
                log::error!("asset_reload: asset {uuid:?} is gone, its copies are dropped: {e:#}");
                Reloaded::Gone
            }
            Err(e) => {
                if svc.keep(uuid, last_good.clone()) {
                    log::warn!(
                        "asset_reload: asset {uuid:?} changed but did not decode, the last good \
                         copy stays until it changes again: {e:#}"
                    );
                    Reloaded::Kept
                } else {
                    // The copy came from a service this one replaced: no
                    // last good copy of this project's asset.
                    log::error!(
                        "asset_reload: asset {uuid:?} did not decode and the copy held came from \
                         another project, so it is dropped: {e:#}"
                    );
                    Reloaded::Gone
                }
            }
        }
    }

    /// Replaces, in `Assets<CpuTexture>`, every changed texture it holds.
    /// Returns the textures whose copy changed (replaced or dropped).
    fn textures(&self, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
        let mut refreshed = HashSet::new();
        let Some(store) = self.store else {
            return refreshed;
        };
        let textures = store.store::<CpuTexture>();
        let held: Vec<(AssetUUID, AssetHandle<CpuTexture>)> = {
            let guard = textures.read().unwrap_or_else(|e| e.into_inner());
            changed
                .iter()
                .filter_map(|uuid| guard.get(uuid).map(|texture| (*uuid, texture.clone())))
                .collect()
        };
        for (uuid, last_good) in held {
            match self.reload::<CpuTexture>(&uuid, &last_good) {
                Reloaded::Fresh(texture) => {
                    textures
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(uuid, texture);
                    refreshed.insert(uuid);
                }
                Reloaded::Gone => {
                    textures
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&uuid);
                    refreshed.insert(uuid);
                }
                Reloaded::Kept => {}
            }
        }
        refreshed
    }

    /// Brings every entity's resolved material up to date: a changed material
    /// is replaced under its uuid, a material naming a changed texture is
    /// projected again. Returns the changed materials some entity holds.
    fn materials(
        &mut self,
        world: &mut World,
        changed: &HashSet<AssetUUID>,
        refreshed: &HashSet<AssetUUID>,
    ) -> HashSet<AssetUUID> {
        let mut affected = Vec::new();
        for (entity, resolved) in world.query::<(EntityId, &HandleComponent<Box<dyn Material>>)>() {
            let names_changed = resolved
                .handle
                .textures()
                .iter()
                .any(|texture| changed.contains(texture));
            if changed.contains(&resolved.uuid) || names_changed {
                affected.push((entity, resolved.uuid, resolved.handle.clone()));
            }
        }

        let mut held = HashSet::new();
        let mut reloaded: HashMap<AssetUUID, Reloaded<Box<dyn Material>>> = HashMap::new();
        for (entity, uuid, current) in affected {
            if changed.contains(&uuid) {
                held.insert(uuid);
                let outcome = reloaded
                    .entry(uuid)
                    .or_insert_with(|| self.reload::<Box<dyn Material>>(&uuid, &current))
                    .clone();
                match outcome {
                    Reloaded::Fresh(material) => {
                        self.publish(uuid, &material);
                        self.load_material_textures(&**material, entity);
                        self.retire(&uuid);
                        world.set_component(
                            entity,
                            HandleComponent {
                                handle: material,
                                uuid,
                            },
                        );
                        drop_component::<HandleComponent<GpuMaterial>>(world, entity);
                        continue;
                    }
                    Reloaded::Gone => {
                        self.forget::<Box<dyn Material>>(&uuid);
                        self.retire(&uuid);
                        drop_component::<HandleComponent<Box<dyn Material>>>(world, entity);
                        drop_component::<HandleComponent<GpuMaterial>>(world, entity);
                        continue;
                    }
                    Reloaded::Kept => {}
                }
            }
            // The material itself is unchanged: the textures it names are
            // loaded if missing, and its projection rebuilt if one changed.
            self.load_material_textures(&**current, entity);
            let names_refreshed = current
                .textures()
                .iter()
                .any(|texture| refreshed.contains(texture));
            if names_refreshed {
                self.retire(&uuid);
                drop_component::<HandleComponent<GpuMaterial>>(world, entity);
            }
        }
        held
    }

    /// Replaces every entity's resolved mesh that changed, under its uuid.
    /// Returns the changed meshes some entity holds.
    fn meshes(&mut self, world: &mut World, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
        let affected: Vec<(EntityId, AssetUUID, AssetHandle<Mesh>)> = world
            .query::<(EntityId, &HandleComponent<Mesh>)>()
            .filter(|(_, resolved)| changed.contains(&resolved.uuid))
            .map(|(entity, resolved)| (entity, resolved.uuid, resolved.handle.clone()))
            .collect();

        let mut held = HashSet::new();
        let mut reloaded: HashMap<AssetUUID, Reloaded<Mesh>> = HashMap::new();
        for (entity, uuid, current) in affected {
            held.insert(uuid);
            let outcome = reloaded
                .entry(uuid)
                .or_insert_with(|| self.reload::<Mesh>(&uuid, &current))
                .clone();
            match outcome {
                Reloaded::Fresh(mesh) => {
                    self.publish(uuid, &mesh);
                    self.retire(&uuid);
                    world.set_component(entity, HandleComponent { handle: mesh, uuid });
                    drop_component::<HandleComponent<GpuMesh>>(world, entity);
                }
                Reloaded::Gone => {
                    self.forget::<Mesh>(&uuid);
                    self.retire(&uuid);
                    drop_component::<HandleComponent<Mesh>>(world, entity);
                    drop_component::<HandleComponent<GpuMesh>>(world, entity);
                }
                Reloaded::Kept => {}
            }
        }
        held
    }

    /// Loads the textures `material` names that the store lacks.
    fn load_material_textures(&self, material: &dyn Material, entity: EntityId) {
        if let Some(store) = self.store {
            load_textures(material, entity, &store.store::<CpuTexture>(), self.service);
        }
    }

    /// Stores `handle` as the shared copy of `uuid`.
    fn publish<A: Asset>(&self, uuid: AssetUUID, handle: &AssetHandle<A>) {
        if let Some(store) = self.store {
            store
                .store::<A>()
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(uuid, handle.clone());
        }
    }

    /// Drops the shared copy of `uuid`.
    fn forget<A: Asset>(&self, uuid: &AssetUUID) {
        if let Some(store) = self.store {
            store
                .store::<A>()
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .remove(uuid);
        }
    }

    /// Takes the GPU projection of `uuid` out of the caches, once, so the
    /// projection builds it again; the eviction service frees the old one
    /// after this frame's render. Without one (no device ever projected
    /// anything), the cache entries are simply dropped.
    fn retire(&mut self, uuid: &AssetUUID) {
        let Some(store) = self.store else {
            return;
        };
        if !self.retired.insert(*uuid) {
            return;
        }
        match self.eviction.map(|eviction| eviction.lock()) {
            Some(Ok(mut eviction)) => eviction.retire(store, uuid),
            Some(Err(_)) => log::error!("asset_reload: AssetEviction mutex poisoned."),
            None => {
                self.forget::<GpuMesh>(uuid);
                self.forget::<GpuMaterial>(uuid);
            }
        }
    }
}

/// Removes `C` from `entity`; a missing component is nothing to remove.
fn drop_component<C: Component>(world: &mut World, entity: EntityId) {
    if let Err(e) = world.remove_component::<C>(entity) {
        log::trace!("asset_reload: removing a component from {entity:?} skipped: {e:?}");
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "asset_reload",
        phase: TickPhase::PreExtract,
        run: asset_reload_system,
        // Lower than `asset_resolver` (order_hint -5): the copies are refreshed
        // before the resolver compares the authored refs against them.
        order_hint: -6,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use khora_core::asset::{AssetHandle, AssetUUID, Material, StandardMaterial};
    use khora_core::ecs::entity::EntityId;
    use khora_core::lane::OutputDeck;
    use khora_core::renderer::api::command::BindGroupId;
    use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
    use khora_core::renderer::api::material::GpuMaterial;
    use khora_core::renderer::api::pipeline::PrimitiveTopology;
    use khora_core::renderer::api::resource::{BufferId, CpuTexture, IndexFormat, SamplerId};
    use khora_core::renderer::api::shader::ShaderVariantKey;
    use khora_core::Runtime;
    use khora_data::ecs::{DataSystemRegistration, HandleComponent, MaterialRef, MeshRef, World};
    use khora_data::AssetStore;
    use khora_telemetry::MetricsRegistry;
    use tempfile::TempDir;

    use crate::asset::{AssetIdRegistry, AssetService, IndexBuilder};

    // ── Fixtures ────────────────────────────────────────────────────────────

    /// The registered data system called `name`.
    fn data_system(name: &str) -> &'static DataSystemRegistration {
        inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == name)
            .unwrap_or_else(|| panic!("`{name}` is registered"))
    }

    /// A project opened as the editor and the runtime open one: loose files
    /// under `<root>/assets/`, every default decoder and the glTF container
    /// registered, served into a runtime with a shared `AssetStore`.
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
                &AssetIdRegistry::load(&root),
                Arc::new(MetricsRegistry::new()),
            )
            .expect("the project opens");
            let service = Arc::new(Mutex::new(service));
            let store = AssetStore::new();
            let mut runtime = Runtime::default();
            runtime.resources.insert(store.clone());
            runtime.services.insert(service.clone());
            Self {
                _dir: dir,
                root,
                service,
                store,
                runtime,
            }
        }

        /// One engine tick of the asset systems: the reload, then the resolver.
        fn tick(&self, world: &mut World) {
            let mut deck = OutputDeck::default();
            (data_system("asset_reload").run)(world, &self.runtime, &mut deck);
            (data_system("asset_resolver").run)(world, &self.runtime, &mut deck);
        }

        /// Rewrites the file at `rel` under the assets folder.
        fn write(&self, rel: &str, bytes: &[u8]) {
            write_file(&self.root.join("assets"), rel, bytes);
        }

        /// Deletes the file at `rel` and lists the assets folder again, as the
        /// editor's hot reload does when a file goes away.
        fn remove_and_reindex(&self, rel: &str) {
            fs::remove_file(self.root.join("assets").join(rel)).unwrap();
            let bytes = IndexBuilder::new(&self.root.join("assets"))
                .with_registry(&AssetIdRegistry::load(&self.root))
                .build_index_bytes()
                .unwrap();
            self.service.lock().unwrap().reindex(&bytes).unwrap();
        }

        /// What the editor's hot reload does when the file `uuid` names changed.
        fn invalidate(&self, uuid: &AssetUUID) {
            self.service.lock().unwrap().invalidate(uuid);
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

        fn gpu_material_cached(&self, uuid: &AssetUUID) -> bool {
            self.store
                .store::<GpuMaterial>()
                .read()
                .unwrap()
                .contains(uuid)
        }

        fn gpu_mesh_cached(&self, uuid: &AssetUUID) -> bool {
            self.store.store::<GpuMesh>().read().unwrap().contains(uuid)
        }

        /// Stands for the GPU projection having built `entity`'s material:
        /// the entity carries a `GpuMaterial` handle and the cache holds it
        /// under the material's uuid.
        fn project_material(&self, world: &mut World, entity: EntityId) -> AssetUUID {
            let uuid = material_uuid(world, entity);
            let stub = gpu_material_stub(uuid);
            self.store
                .store::<GpuMaterial>()
                .write()
                .unwrap()
                .insert(uuid, stub.handle.clone());
            world.add_component(entity, stub).unwrap();
            uuid
        }

        /// Stands for the GPU projection having built `entity`'s mesh.
        fn project_mesh(&self, world: &mut World, entity: EntityId) -> AssetUUID {
            let uuid = world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh resolved")
                .uuid;
            let stub = gpu_mesh_stub(uuid);
            self.store
                .store::<GpuMesh>()
                .write()
                .unwrap()
                .insert(uuid, stub.handle.clone());
            world.add_component(entity, stub).unwrap();
            uuid
        }
    }

    fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn material_uuid(world: &World, entity: EntityId) -> AssetUUID {
        world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("the material resolved")
            .uuid
    }

    /// The `StandardMaterial` `entity`'s resolved handle holds.
    fn standard_of(world: &World, entity: EntityId) -> StandardMaterial {
        let resolved = world
            .get::<HandleComponent<Box<dyn Material>>>(entity)
            .expect("the material resolved");
        let material: &dyn Material = &**resolved.handle;
        material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("a StandardMaterial")
            .clone()
    }

    fn gpu_material_stub(uuid: AssetUUID) -> HandleComponent<GpuMaterial> {
        HandleComponent {
            handle: AssetHandle::new(GpuMaterial {
                uniform_buffer: BufferId(0),
                base_color_view: None,
                metallic_roughness_view: None,
                normal_view: None,
                emissive_view: None,
                occlusion_view: None,
                base_color_texture: None,
                metallic_roughness_texture: None,
                normal_texture: None,
                emissive_texture: None,
                occlusion_texture: None,
                sampler: SamplerId(0),
                bind_group: BindGroupId(0),
                variant: ShaderVariantKey::empty(),
                double_sided: false,
                blend: false,
            }),
            uuid,
        }
    }

    fn gpu_mesh_stub(uuid: AssetUUID) -> HandleComponent<GpuMesh> {
        HandleComponent {
            handle: AssetHandle::new(GpuMesh {
                vertex_buffer: BufferId(0),
                index_buffer: BufferId(0),
                index_count: 0,
                index_format: IndexFormat::Uint32,
                primitive_topology: PrimitiveTopology::TriangleList,
            }),
            uuid,
        }
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
    /// its one material of base colour `color`; the vertex bytes are in the
    /// buffer file `bin` beside it.
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

    // ── Registration ────────────────────────────────────────────────────────

    /// The refreshed copies must be in place before the resolver compares the
    /// authored refs against them, whatever order hints other systems carry.
    #[test]
    fn the_resolver_runs_after_asset_reload() {
        let reload = data_system("asset_reload");
        let resolver = data_system("asset_resolver");
        assert_eq!(reload.phase, resolver.phase, "both run in one phase");
        assert!(
            resolver.runs_after.contains(&"asset_reload"),
            "the resolver declares it runs after asset_reload: {:?}",
            resolver.runs_after
        );
    }

    // ── Steady state ────────────────────────────────────────────────────────

    /// Once a change is applied, the following ticks find nothing to do: the
    /// change is not applied again and the projection rebuilt since stays.
    #[test]
    fn an_unchanged_project_does_no_work() {
        let kmat_path = "materials/plain.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);
        let resolved = project.load_count();

        project.write(kmat_path, &kmat(0.7, None));
        project.invalidate(&mat);
        project.tick(&mut world);
        assert_eq!(
            project.load_count(),
            resolved + 1,
            "the change is applied: the material is decoded again"
        );
        project.project_material(&mut world, entity);
        let applied = project.load_count();

        project.tick(&mut world);
        project.tick(&mut world);

        assert_eq!(project.load_count(), applied, "no change, no decode");
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_some(),
            "the projection built after the change stays"
        );
        assert!(project.gpu_material_cached(&mat));
    }

    // ── Textures ────────────────────────────────────────────────────────────

    #[test]
    fn a_repainted_texture_is_served_fresh_from_the_store() {
        let tex_path = "textures/repainted.png";
        let tex = AssetUUID::new_v5(tex_path);
        let project = Project::new(&[(tex_path, png(2, 2))]);
        let mut world = World::new();
        world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })));
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

    #[test]
    fn a_repainted_texture_reprojects_every_material_naming_it() {
        let tex_path = "textures/shared.png";
        let tex = AssetUUID::new_v5(tex_path);
        let kmat_path = "materials/textured.kmat";
        let project = Project::new(&[(tex_path, png(2, 2)), (kmat_path, kmat(0.5, Some(tex)))]);
        let mut world = World::new();
        let by_asset = world.spawn(MaterialRef::Asset(AssetUUID::new_v5(kmat_path)));
        let inline = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })));
        project.tick(&mut world);
        let asset_material = project.project_material(&mut world, by_asset);
        let inline_material = project.project_material(&mut world, inline);

        project.write(tex_path, &png(5, 5));
        project.invalidate(&tex);
        project.tick(&mut world);

        for (entity, material, what) in [
            (by_asset, asset_material, "the .kmat material"),
            (inline, inline_material, "the inline material"),
        ] {
            assert!(
                world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
                "{what} lost its GPU projection, built with the old image"
            );
            assert!(
                !project.gpu_material_cached(&material),
                "{what}'s stale GPU material left the cache"
            );
            assert_eq!(
                material_uuid(&world, entity),
                material,
                "{what} keeps its resolved handle under the same uuid"
            );
        }
    }

    /// One texture fails to decode, a sibling repaints fine: the broken one
    /// keeps its last good image and the material naming it keeps its
    /// projection, while the sibling's material re-projects.
    #[test]
    fn a_reload_that_fails_to_decode_keeps_the_last_good_value() {
        let broken_path = "textures/broken.png";
        let fine_path = "textures/fine.png";
        let broken = AssetUUID::new_v5(broken_path);
        let fine = AssetUUID::new_v5(fine_path);
        let project = Project::new(&[(broken_path, png(2, 2)), (fine_path, png(2, 2))]);
        let mut world = World::new();
        let names_broken = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(broken),
            ..Default::default()
        })));
        let names_fine = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(fine),
            ..Default::default()
        })));
        project.tick(&mut world);
        let last_good = project.texture(&broken).expect("the first image loads");
        project.project_material(&mut world, names_broken);
        project.project_material(&mut world, names_fine);

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
        assert!(world
            .get::<HandleComponent<GpuMaterial>>(names_fine)
            .is_none());
        assert!(
            project.texture(&broken) == Some(last_good),
            "the texture that fails to decode keeps its last good image"
        );
        assert!(
            world
                .get::<HandleComponent<GpuMaterial>>(names_broken)
                .is_some(),
            "the material naming it keeps its projection"
        );
    }

    // ── Materials ───────────────────────────────────────────────────────────

    #[test]
    fn an_edited_kmat_reaches_its_entities_under_the_same_uuid() {
        let kmat_path = "materials/edited.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);
        assert_eq!(standard_of(&world, entity).roughness, 0.5);
        project.project_material(&mut world, entity);

        project.write(kmat_path, &kmat(0.9, None));
        project.invalidate(&mat);
        project.tick(&mut world);

        assert_eq!(material_uuid(&world, entity), mat, "the same uuid");
        assert_eq!(
            standard_of(&world, entity).roughness,
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
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the stale projection is dropped"
        );
        assert!(
            !project.gpu_material_cached(&mat),
            "the GpuMaterial cache no longer holds the old one"
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
        world.spawn(MaterialRef::Asset(mat));
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
            .map(|_| world.spawn(MaterialRef::Asset(mat)))
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
            assert_eq!(standard_of(&world, entity).roughness, 0.9);
        }
    }

    // ── Meshes ──────────────────────────────────────────────────────────────

    #[test]
    fn a_modified_mesh_reaches_its_entities_and_its_gpu_mesh_is_retired() {
        let obj_path = "meshes/shape.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the triangle resolves")
                .handle
                .positions
                .len(),
            3
        );
        project.project_mesh(&mut world, entity);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.tick(&mut world);

        let resolved = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("the entity keeps a mesh");
        assert_eq!(resolved.uuid, mesh, "the same uuid");
        assert_eq!(resolved.handle.positions.len(), 6, "the quad's vertices");
        assert_eq!(
            project
                .stored_mesh(&mesh)
                .expect("the store holds it")
                .positions
                .len(),
            6,
            "the store serves the modified mesh"
        );
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_none(),
            "the stale GPU mesh handle is dropped"
        );
        assert!(
            !project.gpu_mesh_cached(&mesh),
            "the GpuMesh cache no longer holds the old one"
        );
    }

    #[test]
    fn an_asset_gone_from_the_index_is_dropped() {
        let obj_path = "meshes/gone.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        project.remove_and_reindex(obj_path);
        project.invalidate(&mesh);
        project.tick(&mut world);

        assert!(
            world.get::<HandleComponent<Mesh>>(entity).is_none(),
            "the entity no longer carries the deleted mesh"
        );
        assert!(world.get::<HandleComponent<GpuMesh>>(entity).is_none());
        assert!(project.stored_mesh(&mesh).is_none(), "the store drops it");
        assert!(!project.gpu_mesh_cached(&mesh));
        assert!(
            world.get::<MeshRef>(entity).is_some(),
            "the authored reference is the author's, and stays"
        );
    }

    // ── glTF ────────────────────────────────────────────────────────────────

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
        let entity = world.spawn((MeshRef::Asset(mesh), MaterialRef::Asset(material)));
        project.tick(&mut world);
        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part resolves")
                .handle
                .positions
                .len(),
            3
        );
        assert_eq!(standard_of(&world, entity).base_color.g, 0.0);

        let (gltf, buffer) = gltf_shape(&TWO_TRIANGLES, [0.0, 1.0, 0.0, 1.0], bin);
        project.write(rel, &gltf);
        project.write("models/shape.bin", &buffer);
        project.invalidate(&file);
        project.tick(&mut world);

        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays resolved")
                .handle
                .positions
                .len(),
            6,
            "the mesh part is the re-exported one"
        );
        assert_eq!(
            standard_of(&world, entity).base_color.g,
            1.0,
            "the material part is the re-exported one"
        );
    }

    /// Lists the assets folder again, as the editor's hot reload does after a
    /// file that can hold parts was modified.
    fn reindex(project: &Project) {
        let bytes = IndexBuilder::new(&project.root.join("assets"))
            .with_registry(&AssetIdRegistry::load(&project.root))
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

    /// A glTF saved again with a repainted image: its texture part and the
    /// material naming it are refreshed when the container is invalidated.
    #[test]
    fn a_reexported_gltf_refreshes_its_texture_part_and_reprojects_its_material() {
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
        let entity = world.spawn(MaterialRef::Asset(material));
        project.tick(&mut world);
        let before = project.texture(&texture0).expect("the texture part loads");
        assert_eq!((before.size.width, before.size.height), (3, 2));
        project.project_material(&mut world, entity);

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
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the material built with the old image is projected again"
        );
        assert!(!project.gpu_material_cached(&material));
    }

    /// A glTF's image is a file of its own: repainting it, and nothing else,
    /// changes what the glTF's texture part decodes to. The editor's hot
    /// reload invalidates only the image file it saw change; the part read
    /// from it, and the material naming that part, must follow.
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
        let entity = world.spawn(MaterialRef::Asset(material));
        project.tick(&mut world);
        let before = project.texture(&texture0).expect("the texture part loads");
        assert_eq!((before.size.width, before.size.height), (3, 2));
        project.project_material(&mut world, entity);

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
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the glTF material built with the old image is projected again"
        );
    }

    /// An exporter caught mid-write leaves a glTF the index cannot read, so
    /// it lists the file without its parts. That is a file that failed to
    /// decode, not parts the author removed: the entities keep the last good
    /// mesh and material until the file is whole again.
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
        let entity = world.spawn((MeshRef::Asset(mesh), MaterialRef::Asset(material)));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);
        project.project_material(&mut world, entity);

        // What the editor's hot reload does on a modified glTF: invalidate
        // it, then list its parts again since a glTF may hold some.
        project.write(rel, &gltf[..gltf.len() / 2]);
        project.invalidate(&file);
        reindex(&project);
        project.tick(&mut world);

        let resolved = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("the entity keeps the last good mesh part");
        assert_eq!(resolved.handle.positions.len(), 3);
        assert!(
            world
                .get::<HandleComponent<Box<dyn Material>>>(entity)
                .is_some(),
            "the entity keeps the last good material part"
        );
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_some(),
            "the mesh projection built from the last good part stays"
        );
    }

    /// A glTF placed before the image it names: its material resolves, its
    /// texture part does not. The image then arrives as a new file, which
    /// the editor's hot reload invalidates before it lists the assets folder
    /// again; the texture part read from that image must load.
    #[test]
    fn an_image_arriving_after_a_gltf_material_resolved_loads_its_texture_part() {
        let rel = "models/painted.gltf";
        let image = "models/painted.png";
        let project = Project::new(&[(rel, gltf_with_one_textured_material("painted.png"))]);
        let file = AssetUUID::new_v5(rel);
        let material = AssetUUID::sub_asset(file, "material0");
        let texture0 = AssetUUID::sub_asset(file, "texture0");
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(material));
        project.tick(&mut world);
        assert!(
            world
                .get::<HandleComponent<Box<dyn Material>>>(entity)
                .is_some(),
            "the premise: the material part resolves without its image"
        );
        assert!(
            project.texture(&texture0).is_none(),
            "the premise: the texture part has no image to read yet"
        );

        // What the editor's hot reload does on a created file: invalidate
        // it, then list the assets folder again.
        project.write(image, &png(3, 2));
        project.invalidate(&AssetUUID::new_v5(image));
        reindex(&project);
        project.tick(&mut world);

        let texture = project
            .texture(&texture0)
            .expect("the texture part loads once its image exists");
        assert_eq!((texture.size.width, texture.size.height), (3, 2));
    }

    /// A glTF named whole as one mesh reads its vertices from the buffer file
    /// beside it: rewriting that file, and nothing else, changes the mesh,
    /// and the entity must show the rewritten vertices.
    #[test]
    fn a_rewritten_buffer_refreshes_a_gltf_resolved_whole_as_a_mesh() {
        let rel = "models/shape.gltf";
        let bin = "models/shape.bin";
        let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "shape.bin");
        let project = Project::new(&[(rel, gltf), (bin, buffer)]);
        let file = AssetUUID::new_v5(rel);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(file));
        project.tick(&mut world);
        let before = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("the glTF resolves as one mesh")
            .handle
            .positions
            .clone();

        // Same vertex count and byte length, other positions: only the
        // buffer file changes.
        let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shape.bin");
        project.write(bin, &buffer);
        project.invalidate(&AssetUUID::new_v5(bin));
        project.tick(&mut world);

        let after = &world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("the mesh stays resolved")
            .handle
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
                world.spawn(MeshRef::Asset(mesh))
            })
            .collect();
        project.tick(&mut world);

        let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shared.bin");
        project.write(bin, &buffer);
        project.invalidate(&AssetUUID::new_v5(bin));
        project.tick(&mut world);

        for entity in entities {
            let positions = &world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays resolved")
                .handle
                .positions;
            assert_eq!(positions[1].x, 2.0, "the rewritten buffer's vertices");
        }
    }

    /// A buffer saved broken leaves its glTF unable to open: the parts keep
    /// their last good value. Once the buffer is saved whole, the glTF's
    /// parts must show it, though only the buffer file changed each time.
    #[test]
    fn a_buffer_repaired_after_a_broken_save_refreshes_the_gltf_parts() {
        let rel = "models/shape.gltf";
        let bin = "models/shape.bin";
        let (gltf, buffer) = gltf_shape(&TRIANGLE, [1.0, 0.0, 0.0, 1.0], "shape.bin");
        let project = Project::new(&[(rel, gltf), (bin, buffer.clone())]);
        let mesh = AssetUUID::sub_asset(AssetUUID::new_v5(rel), "mesh0/prim0");
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        project.write(bin, &buffer[..buffer.len() / 2]);
        project.invalidate(&AssetUUID::new_v5(bin));
        project.tick(&mut world);
        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the premise: the last good part stays")
                .handle
                .positions[1]
                .x,
            1.0
        );

        let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        let (_, repaired) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "shape.bin");
        project.write(bin, &repaired);
        project.invalidate(&AssetUUID::new_v5(bin));
        project.tick(&mut world);

        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays resolved")
                .handle
                .positions[1]
                .x,
            2.0,
            "the mesh part reads the repaired buffer"
        );
    }

    /// The engine's eviction service is present: the projection a change
    /// made stale is handed to it to free after the render, not dropped
    /// from the cache and leaked.
    #[test]
    fn a_stale_projection_is_handed_to_the_eviction_to_free() {
        use khora_data::AssetEviction;

        let kmat_path = "materials/evicted.kmat";
        let obj_path = "meshes/evicted.obj";
        let mat = AssetUUID::new_v5(kmat_path);
        let mesh = AssetUUID::new_v5(obj_path);
        let mut project = Project::new(&[
            (kmat_path, kmat(0.5, None)),
            (obj_path, TRIANGLE_OBJ.as_bytes().to_vec()),
        ]);
        let eviction = Arc::new(Mutex::new(AssetEviction::new()));
        project.runtime.resources.insert(eviction.clone());
        let mut world = World::new();
        let entity = world.spawn((MeshRef::Asset(mesh), MaterialRef::Asset(mat)));
        project.tick(&mut world);
        project.project_material(&mut world, entity);
        project.project_mesh(&mut world, entity);

        project.write(kmat_path, &kmat(0.9, None));
        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mat);
        project.invalidate(&mesh);
        project.tick(&mut world);

        assert!(!project.gpu_material_cached(&mat));
        assert!(!project.gpu_mesh_cached(&mesh));
        assert_eq!(
            eviction.lock().unwrap().retired_count(),
            2,
            "the stale GPU material and mesh wait for the eviction to free them"
        );
    }

    /// A `.kmat` saved broken keeps its last good value on the entities that
    /// hold it; an entity that names the same asset afterwards (spawned, or
    /// restored with its scene) must see that same value, not nothing.
    #[test]
    fn an_entity_naming_a_material_kept_after_a_failed_reload_sees_the_last_good_value() {
        let kmat_path = "materials/kept.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let first = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);

        project.write(kmat_path, b"definitely not a material");
        project.invalidate(&mat);
        project.tick(&mut world);
        assert_eq!(
            standard_of(&world, first).roughness,
            0.5,
            "the premise: the entity holding it keeps the last good value"
        );

        let later = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);

        assert!(
            world
                .get::<HandleComponent<Box<dyn Material>>>(later)
                .is_some(),
            "the later entity resolves the material the first one shows"
        );
        assert_eq!(standard_of(&world, later).roughness, 0.5);
    }

    /// A texture that did not decode when its material resolved comes back
    /// once its file is fixed and announced, without the material changing.
    #[test]
    fn a_texture_that_failed_to_load_comes_back_once_its_file_is_fixed() {
        let tex_path = "textures/late.png";
        let tex = AssetUUID::new_v5(tex_path);
        let project = Project::new(&[(tex_path, b"not yet an image".to_vec())]);
        let mut world = World::new();
        world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })));
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

    /// Another project's service moves in under the same uuids: a mesh both
    /// projects hold at one path reaches the entity with the incoming
    /// project's contents.
    #[test]
    fn a_service_moving_in_refreshes_the_copies_the_outgoing_one_served() {
        let obj_path = "meshes/shared.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        let other = tempfile::tempdir().unwrap();
        write_file(&other.path().join("assets"), obj_path, QUAD_OBJ.as_bytes());
        let mut incoming = AssetService::open_loose_files(
            other.path(),
            &AssetIdRegistry::load(other.path()),
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
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh stays resolved")
                .handle
                .positions
                .len(),
            6,
            "the incoming project's mesh"
        );
        assert!(world.get::<HandleComponent<GpuMesh>>(entity).is_none());
        assert!(!project.gpu_mesh_cached(&mesh));
    }

    /// Many files change between two ticks (a branch checkout, a bulk
    /// re-export): a change announced early, then buried under a full
    /// backlog of later ones, must still reach the copy the engine holds.
    #[test]
    fn a_change_buried_under_a_full_backlog_still_reaches_its_copy() {
        use crate::asset::CHANGE_BACKLOG;

        let tex_path = "textures/early.png";
        let tex = AssetUUID::new_v5(tex_path);
        let project = Project::new(&[(tex_path, png(2, 2))]);
        let mut world = World::new();
        world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })));
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
}
