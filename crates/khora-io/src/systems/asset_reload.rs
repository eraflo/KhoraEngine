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
//!
//! It decodes nothing itself: a changed asset some copy holds is asked for
//! again ([`request`](crate::asset::AssetService::request)), and decoded off
//! the frame by the `loading` family. Until it lands the last good copy keeps
//! serving; the copies are refreshed on the frame it lands — taken in by
//! `asset_intake` at that frame's boundary, before this runs. An asset the
//! index alone says is gone is dropped on the frame its change is seen.

use khora_data::ecs::SystemContext;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::{Asset, AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::{GpuMesh, Mesh};
use khora_core::renderer::api::material::GpuMaterial;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::Runtime;
use khora_data::ecs::{Component, DataSystemRegistration, HandleComponent, TickPhase, World};
use khora_data::{AssetEviction, AssetStore};

use std::cell::RefCell;

use super::asset_resolver::load_textures;
use crate::asset::{AssetService, Requested};

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
    /// Its new value is still being decoded off the frame: the last good
    /// copy keeps serving until it lands.
    Pending,
}

// Written out rather than derived: a derive would demand `A: Clone`, which
// an `AssetHandle` never needs.
impl<A: Asset> Clone for Reloaded<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Fresh(handle) => Self::Fresh(handle.clone()),
            Self::Gone => Self::Gone,
            Self::Kept => Self::Kept,
            Self::Pending => Self::Pending,
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
    /// The changed assets whose new value is still being decoded.
    pending: RefCell<HashSet<AssetUUID>>,
}

fn asset_reload_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(service) = runtime.services.get::<Arc<Mutex<AssetService>>>() else {
        return;
    };
    // Steady state: one lock, an empty stream and nothing being decoded.
    // The assets changed before whose new value had not landed are looked
    // at again with the new changes.
    let changed = match service.lock() {
        Ok(mut svc) => {
            let mut changed = svc.take_changes();
            changed.extend(svc.take_reloading());
            changed
        }
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
        pending: RefCell::new(HashSet::new()),
    };
    let refreshed = reload.textures(&changed);
    let held_materials = reload.materials(world, &changed, &refreshed);
    let held_meshes = reload.meshes(world, &changed);
    let held_fonts = reload.fonts(world, &changed);

    // A changed asset no entity holds keeps no copy: the resolver decodes it
    // afresh when a reference names it again.
    for uuid in &changed {
        if held_materials.contains(uuid) || held_meshes.contains(uuid) || held_fonts.contains(uuid)
        {
            continue;
        }
        if reload.pending.borrow().contains(uuid) {
            continue;
        }
        // Nothing holds it any more: no last good copy to serve either.
        if let Ok(mut svc) = service.lock() {
            svc.settle_meanwhile::<Box<dyn Material>>(uuid);
            svc.settle_meanwhile::<Mesh>(uuid);
            svc.settle_meanwhile::<Font>(uuid);
            svc.settle_meanwhile::<CpuTexture>(uuid);
        }
        reload.forget::<Box<dyn Material>>(uuid);
        reload.forget::<Mesh>(uuid);
        reload.forget::<Font>(uuid);
        reload.retire(uuid);
    }

    // What is still being decoded is looked at again on the next pass.
    let pending = reload.pending.into_inner();
    if !pending.is_empty() {
        match service.lock() {
            Ok(mut svc) => svc.keep_reloading(pending),
            Err(_) => log::error!("asset_reload: AssetService mutex poisoned."),
        }
    }
}

impl Reload<'_> {
    /// Asks the service for `uuid` again — its copy was dropped when the
    /// change was announced, so the `loading` family decodes it off the frame.
    /// `last_good` is the copy the engine shows meanwhile.
    fn reload<A: Asset>(&self, uuid: &AssetUUID, last_good: &AssetHandle<A>) -> Reloaded<A> {
        let Ok(mut svc) = self.service.lock() else {
            log::error!("asset_reload: AssetService mutex poisoned.");
            return Reloaded::Kept;
        };
        // Removed from the project: the index says so, nothing to decode.
        if svc.gone_from_index(uuid) {
            log::info!("asset_reload: asset {uuid:?} is gone, its copies are dropped");
            svc.settle_meanwhile::<A>(uuid);
            return Reloaded::Gone;
        }
        let outcome = match svc.request_decoded::<A>(uuid) {
            Requested::Ready(handle) => Reloaded::Fresh(handle),
            Requested::Pending => {
                self.pending.borrow_mut().insert(*uuid);
                // A reference resolved meanwhile is served what the engine
                // already shows.
                svc.serve_meanwhile(uuid, last_good.clone());
                return Reloaded::Pending;
            }
            // The decode failed: a part its container no longer holds is
            // gone; anything else was caught mid-write.
            Requested::Failed if svc.is_gone(uuid) => {
                log::error!("asset_reload: asset {uuid:?} is gone, its copies are dropped");
                Reloaded::Gone
            }
            Requested::Failed => {
                if svc.keep(uuid, last_good.clone()) {
                    log::warn!(
                        "asset_reload: asset {uuid:?} changed but did not decode, the last good \
                         copy stays until it changes again"
                    );
                    Reloaded::Kept
                } else {
                    // The copy came from a service this one replaced: no
                    // last good copy of this project's asset.
                    log::error!(
                        "asset_reload: asset {uuid:?} did not decode and the copy held came from \
                         another project, so it is dropped"
                    );
                    Reloaded::Gone
                }
            }
        };
        svc.settle_meanwhile::<A>(uuid);
        outcome
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
                Reloaded::Kept | Reloaded::Pending => {}
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
                    Reloaded::Kept | Reloaded::Pending => {}
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
                Reloaded::Kept | Reloaded::Pending => {}
            }
        }
        held
    }

    /// Replaces every entity's resolved font that changed, under its uuid:
    /// the handle written again is what tells the text systems to register
    /// the new file and lay their texts out with it. Returns the changed fonts
    /// some entity holds.
    fn fonts(&mut self, world: &mut World, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
        let affected: Vec<(EntityId, AssetUUID, AssetHandle<Font>)> = world
            .query::<(EntityId, &HandleComponent<Font>)>()
            .filter(|(_, resolved)| changed.contains(&resolved.uuid))
            .map(|(entity, resolved)| (entity, resolved.uuid, resolved.handle.clone()))
            .collect();

        let mut held = HashSet::new();
        let mut reloaded: HashMap<AssetUUID, Reloaded<Font>> = HashMap::new();
        for (entity, uuid, current) in affected {
            held.insert(uuid);
            let outcome = reloaded
                .entry(uuid)
                .or_insert_with(|| self.reload::<Font>(&uuid, &current))
                .clone();
            match outcome {
                Reloaded::Fresh(font) => {
                    self.publish(uuid, &font);
                    world.set_component(entity, HandleComponent { handle: font, uuid });
                }
                Reloaded::Gone => {
                    self.forget::<Font>(&uuid);
                    drop_component::<HandleComponent<Font>>(world, entity);
                }
                Reloaded::Kept | Reloaded::Pending => {}
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
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use khora_core::asset::{AssetHandle, AssetUUID, Material, StandardMaterial};
    use khora_core::ecs::entity::EntityId;
    use khora_core::lane::{OutputDeck, Reserve};
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

        /// One engine tick of the asset systems — the reload, then the
        /// resolver — and the frames after it until what they asked for is
        /// decoded off the frame and taken in at a boundary.
        fn tick(&self, world: &mut World) {
            crate::systems::asset_intake::test_support::settle(&self.runtime, world, |world| {
                let mut deck = OutputDeck::default();
                for name in ["asset_reload", "asset_resolver"] {
                    (data_system(name).run)(
                        world,
                        &self.runtime,
                        &mut deck,
                        khora_data::ecs::SystemContext {
                            since: khora_data::ecs::Tick(0),
                            resumed: false,
                            scope: None,
                        },
                    );
                }
            });
        }

        /// One frame of the asset systems and nothing more: the boundary's
        /// intake of what the `loading` family landed, then the PreExtract
        /// reload and resolver. Nothing is decoded during it: what it files
        /// waits for [`Self::decode`].
        fn frame(&self, world: &mut World) {
            crate::systems::asset_intake::take_in_decoded(&self.runtime);
            let mut deck = OutputDeck::default();
            for name in ["asset_reload", "asset_resolver"] {
                (data_system(name).run)(
                    world,
                    &self.runtime,
                    &mut deck,
                    khora_data::ecs::SystemContext {
                        since: khora_data::ecs::Tick(0),
                        resumed: false,
                        scope: None,
                    },
                );
            }
        }

        /// One run of the `loading` family between two frames: every decode
        /// filed, decoded and received under its lease. Returns how many
        /// requests it took.
        fn decode(&self) -> usize {
            crate::systems::asset_intake::test_support::decode_filed(&self.runtime)
        }

        /// [`Self::decode`], with `during` done by the host once the decodes
        /// ran and before the frame start receives them.
        fn decode_while(&self, during: impl FnOnce()) -> usize {
            crate::systems::asset_intake::test_support::decode_filed_while(&self.runtime, during)
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
                specular_view: None,
                specular_color_view: None,
                specular_texture: None,
                specular_color_texture: None,
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

    // ── Through the `loading` family ───────────────────────────────────────────
    //
    // A changed asset is decoded again off the frame, by the `loading` family,
    // as a first load is. The frame that sees the change decodes nothing; the
    // engine keeps showing the last good copy; the refresh — re-tagging, the
    // stale projection retired — runs on the frame the new value lands, its
    // boundary's intake followed by the reload's pass.

    /// The frame that sees a change files the decode and decodes nothing
    /// itself: the service counts no new load, the entity still shows the
    /// value it had, and the `loading` family has one request to take.
    #[test]
    fn a_change_is_not_decoded_on_the_frame_it_is_seen() {
        let kmat_path = "materials/seen.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(mat));
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
            standard_of(&world, entity).roughness,
            0.5,
            "the entity shows the value it had"
        );
        assert_eq!(
            project.decode(),
            1,
            "the changed material is filed once for the `loading` family"
        );
    }

    /// Until the new value lands, the engine shows the last good one: the
    /// entity keeps its handle and its projection, the store its copy, frame
    /// after frame; the change is filed once, however many frames it waits.
    #[test]
    fn the_last_good_value_serves_until_the_new_one_lands() {
        let obj_path = "meshes/waiting.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        for frame in 0..3 {
            project.frame(&mut world);
            let resolved = world
                .get::<HandleComponent<Mesh>>(entity)
                .unwrap_or_else(|| panic!("frame {frame}: the entity keeps its mesh"));
            assert_eq!(
                (resolved.uuid, resolved.handle.positions.len()),
                (mesh, 3),
                "frame {frame}: the last good triangle"
            );
            assert!(
                world.get::<HandleComponent<GpuMesh>>(entity).is_some(),
                "frame {frame}: its projection stays"
            );
            assert!(project.gpu_mesh_cached(&mesh), "frame {frame}");
            assert_eq!(
                project.stored_mesh(&mesh).map(|m| m.positions.len()),
                Some(3),
                "frame {frame}: the store keeps the last good copy"
            );
        }
        assert_eq!(project.decode(), 1, "filed once over the frames it waited");

        project.frame(&mut world);
        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the entity keeps a mesh")
                .handle
                .positions
                .len(),
            6,
            "the quad, on the frame it lands"
        );
    }

    /// On the frame the new values land, the entities are re-tagged under the
    /// same uuids, their stale GPU handles dropped, and the stale projections
    /// handed to the eviction — not before.
    #[test]
    fn on_landing_entities_are_retagged_and_stale_projections_evicted_the_same_frame() {
        use khora_data::AssetEviction;

        let kmat_path = "materials/landing.kmat";
        let obj_path = "meshes/landing.obj";
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
        project.frame(&mut world);

        assert_eq!(
            eviction.lock().unwrap().retired_count(),
            0,
            "nothing is retired before the new values land"
        );
        assert!(world.get::<HandleComponent<GpuMaterial>>(entity).is_some());
        assert!(world.get::<HandleComponent<GpuMesh>>(entity).is_some());
        assert!(project.gpu_material_cached(&mat));
        assert!(project.gpu_mesh_cached(&mesh));

        assert_eq!(project.decode(), 2, "the material and the mesh are filed");
        project.frame(&mut world);

        assert_eq!(material_uuid(&world, entity), mat, "the same uuid");
        assert_eq!(standard_of(&world, entity).roughness, 0.9);
        let resolved = world
            .get::<HandleComponent<Mesh>>(entity)
            .expect("the entity keeps a mesh");
        assert_eq!((resolved.uuid, resolved.handle.positions.len()), (mesh, 6));
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the stale GPU material handle is dropped on the frame it lands"
        );
        assert!(
            world.get::<HandleComponent<GpuMesh>>(entity).is_none(),
            "the stale GPU mesh handle is dropped on the frame it lands"
        );
        assert!(!project.gpu_material_cached(&mat));
        assert!(!project.gpu_mesh_cached(&mesh));
        assert_eq!(
            eviction.lock().unwrap().retired_count(),
            2,
            "the stale GPU material and mesh wait for the eviction"
        );
    }

    /// A repainted texture: the store and the materials naming it keep the
    /// old image and their projections until the new image lands; on that
    /// frame the store serves it and every material naming it re-projects.
    #[test]
    fn a_repainted_texture_reprojects_its_materials_the_frame_it_lands() {
        let tex_path = "textures/landing.png";
        let tex = AssetUUID::new_v5(tex_path);
        let project = Project::new(&[(tex_path, png(2, 2))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
            base_color_texture: Some(tex),
            ..Default::default()
        })));
        project.tick(&mut world);
        let material = project.project_material(&mut world, entity);

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
            world.get::<HandleComponent<GpuMaterial>>(entity).is_some(),
            "the material keeps its projection meanwhile"
        );

        assert_eq!(project.decode(), 1, "the texture is filed");
        project.frame(&mut world);

        let after = project.texture(&tex).expect("the store keeps the image");
        assert_eq!((after.size.width, after.size.height), (5, 5));
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "the material built with the old image re-projects on the frame it lands"
        );
        assert!(!project.gpu_material_cached(&material));
    }

    /// A reload whose decode fails is learned when the failure lands from
    /// the `loading` family: the entity keeps the last good value and its
    /// projection, and an entity naming the asset afterwards sees that value.
    #[test]
    fn a_reload_that_fails_off_the_frame_keeps_the_last_good_value() {
        let kmat_path = "materials/broken_later.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let first = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);
        project.project_material(&mut world, first);

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
            standard_of(&world, first).roughness,
            0.5,
            "the entity keeps the last good value once the failure landed"
        );
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(first).is_some(),
            "and its projection"
        );
        assert!(project.gpu_material_cached(&mat));

        let later = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);
        assert_eq!(
            standard_of(&world, later).roughness,
            0.5,
            "an entity naming it afterwards sees the last good value"
        );
    }

    /// The file changes again while the decode of its first change is out:
    /// that decode read bytes no longer on disk, so it is never installed.
    /// The entity goes from the last good value straight to the newest one,
    /// and no frame shows the one in between.
    #[test]
    fn a_change_made_while_a_reload_is_in_flight_installs_only_the_newest() {
        let kmat_path = "materials/twice.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);

        project.write(kmat_path, &kmat(0.9, None));
        project.invalidate(&mat);
        project.frame(&mut world);
        assert_ne!(
            standard_of(&world, entity).roughness,
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
            let roughness = standard_of(&world, entity).roughness;
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

    /// A re-exported glTF: its parts are decoded again by the family, the
    /// entity shows the old parts on the frame the change is seen and the
    /// re-exported ones on the frame they land.
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
        let entity = world.spawn((MeshRef::Asset(mesh), MaterialRef::Asset(material)));
        project.tick(&mut world);
        let loaded = project.load_count();

        let (gltf, buffer) = gltf_shape(&TWO_TRIANGLES, [0.0, 1.0, 0.0, 1.0], bin);
        project.write(rel, &gltf);
        project.write("models/through.bin", &buffer);
        project.invalidate(&file);
        project.frame(&mut world);

        assert_eq!(project.load_count(), loaded, "no part decoded on the frame");
        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays")
                .handle
                .positions
                .len(),
            3,
            "the old mesh part until the new one lands"
        );
        assert_eq!(standard_of(&world, entity).base_color.g, 0.0);

        assert!(project.decode() > 0, "the parts are filed for the family");
        project.frame(&mut world);

        assert_eq!(
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays resolved")
                .handle
                .positions
                .len(),
            6,
            "the re-exported mesh part"
        );
        assert_eq!(
            standard_of(&world, entity).base_color.g,
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
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        let loaded = project.load_count();

        let moved = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        let (_, buffer) = gltf_shape(&moved, [1.0, 0.0, 0.0, 1.0], "buffered.bin");
        project.write(bin, &buffer);
        project.invalidate(&AssetUUID::new_v5(bin));
        project.frame(&mut world);

        let x = |world: &World| {
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("the mesh part stays resolved")
                .handle
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
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        project.remove_and_reindex(obj_path);
        project.invalidate(&mesh);
        project.frame(&mut world);

        assert!(
            world.get::<HandleComponent<Mesh>>(entity).is_none(),
            "the entity no longer carries the deleted mesh"
        );
        assert!(world.get::<HandleComponent<GpuMesh>>(entity).is_none());
        assert!(project.stored_mesh(&mesh).is_none(), "the store drops it");
        assert!(!project.gpu_mesh_cached(&mesh));
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
    /// changed too and landed first: the material re-projects with the new
    /// image, and again once its own new value lands.
    #[test]
    fn a_material_still_decoding_reprojects_when_a_texture_it_names_lands_first() {
        let tex_path = "textures/first.png";
        let tex = AssetUUID::new_v5(tex_path);
        let kmat_path = "materials/second.kmat";
        let mat = AssetUUID::new_v5(kmat_path);
        let project = Project::new(&[(tex_path, png(2, 2)), (kmat_path, kmat(0.5, Some(tex)))]);
        let mut world = World::new();
        let entity = world.spawn(MaterialRef::Asset(mat));
        project.tick(&mut world);
        project.project_material(&mut world, entity);

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
            standard_of(&world, entity).roughness,
            0.5,
            "the old material"
        );
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "re-projects with the new image"
        );
        project.project_material(&mut world, entity);

        assert_eq!(decode_only(&project, |uuid| *uuid == mat), 1);
        project.frame(&mut world);
        assert_eq!(standard_of(&world, entity).roughness, 0.9);
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(entity).is_none(),
            "re-projects with the new material"
        );
        assert!(reloading(&project).is_empty(), "nothing left reloading");
    }

    /// The entity holding a changed mesh is despawned while the new mesh
    /// decodes: the change is no longer looked at once nothing holds it.
    #[test]
    fn a_reload_whose_holder_is_despawned_is_no_longer_looked_at() {
        let obj_path = "meshes/despawned.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        assert!(reloading(&project).contains(&mesh));
        world.despawn(entity);
        project.frame(&mut world);
        assert!(
            reloading(&project).is_empty(),
            "dropped once nothing holds it"
        );
        assert!(!project.gpu_mesh_cached(&mesh));
        assert_eq!(project.decode(), 1);
        project.frame(&mut world);
        assert!(reloading(&project).is_empty());
    }

    /// The editor hands its project's service over while a reload of the
    /// engine's is decoding: the entity ends on the incoming project's mesh.
    #[test]
    fn a_service_handed_over_mid_reload_lands_the_incoming_mesh() {
        let obj_path = "meshes/handover.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        let five = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 2 0\nf 1 2 3\nf 1 3 4\nf 1 4 5\n";
        project.write(obj_path, five.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);

        let other = tempfile::tempdir().unwrap();
        write_file(&other.path().join("assets"), obj_path, QUAD_OBJ.as_bytes());
        let mut own = AssetService::open_loose_files(
            other.path(),
            &AssetIdRegistry::load(other.path()),
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
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("resolved")
                .handle
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
        let entity = world.spawn(MeshRef::Asset(mesh));
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
            world
                .get::<HandleComponent<Mesh>>(entity)
                .expect("resolved")
                .handle
                .positions
                .len(),
            6
        );
    }

    /// An entity naming a mesh while its reload decodes is served what its
    /// siblings show, the last good copy, until the new one lands; the
    /// change is decoded once for both.
    #[test]
    fn an_entity_naming_an_asset_mid_reload_is_served_the_last_good_copy() {
        let obj_path = "meshes/sibling.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let first = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        let second = world.spawn(MeshRef::Asset(mesh));
        project.frame(&mut world);
        assert_eq!(project.decode(), 1, "decoded once for both");
        let shown = |world: &World, entity| {
            world
                .get::<HandleComponent<Mesh>>(entity)
                .map(|resolved| resolved.handle.positions.len())
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
        let entity = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);
        project.project_mesh(&mut world, entity);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        project.remove_and_reindex(obj_path);
        project.decode();
        project.frame(&mut world);
        assert!(world.get::<HandleComponent<Mesh>>(entity).is_none());
        assert!(project.stored_mesh(&mesh).is_none());
        assert!(reloading(&project).is_empty());
    }

    /// The vertex count of the mesh `entity` holds, if it holds one.
    fn shown_vertices(world: &World, entity: EntityId) -> Option<usize> {
        world
            .get::<HandleComponent<Mesh>>(entity)
            .map(|resolved| resolved.handle.positions.len())
    }

    /// An entity served the last good copy mid-reload is re-tagged with the
    /// new value on the frame it lands, its stale projection dropped, like
    /// the entity that held the asset before the change.
    #[test]
    fn a_sibling_served_the_last_good_copy_is_retagged_when_the_new_value_lands() {
        let obj_path = "meshes/retagged.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let first = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        let second = world.spawn(MeshRef::Asset(mesh));
        project.frame(&mut world);
        assert_eq!(shown_vertices(&world, second), Some(3));
        project.project_mesh(&mut world, second);

        project.decode();
        project.frame(&mut world);
        assert_eq!(shown_vertices(&world, first), Some(6));
        assert_eq!(shown_vertices(&world, second), Some(6));
        assert!(world.get::<HandleComponent<GpuMesh>>(second).is_none());
    }

    /// The only holder of a changed mesh leaves while it decodes, and a new
    /// entity names it on the same frame: the new entity is not handed the
    /// copy no one holds any more (it would never be refreshed), and shows
    /// the new value once it lands.
    #[test]
    fn an_entity_naming_an_asset_its_last_holder_left_mid_reload_gets_the_new_value() {
        let obj_path = "meshes/handed_on.obj";
        let mesh = AssetUUID::new_v5(obj_path);
        let project = Project::new(&[(obj_path, TRIANGLE_OBJ.as_bytes().to_vec())]);
        let mut world = World::new();
        let first = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        world.despawn(first);
        let second = world.spawn(MeshRef::Asset(mesh));
        for _ in 0..8 {
            project.frame(&mut world);
            project.decode();
        }
        project.frame(&mut world);
        assert_eq!(shown_vertices(&world, second), Some(6));
    }

    /// A material resolved while a texture it names is decoded again is
    /// projected with the old image, and re-projected once the new one lands.
    #[test]
    fn a_material_resolved_mid_reload_of_its_texture_reprojects_when_the_image_lands() {
        let tex_path = "textures/mid.png";
        let tex = AssetUUID::new_v5(tex_path);
        let project = Project::new(&[(tex_path, png(2, 2))]);
        let mut world = World::new();
        let material = || {
            MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(tex),
                ..Default::default()
            }))
        };
        world.spawn(material());
        project.tick(&mut world);

        project.write(tex_path, &png(5, 5));
        project.invalidate(&tex);
        project.frame(&mut world);
        let later = world.spawn(material());
        project.frame(&mut world);
        project.project_material(&mut world, later);

        project.decode();
        project.frame(&mut world);
        let image = project.texture(&tex).expect("the image");
        assert_eq!((image.size.width, image.size.height), (5, 5));
        assert!(
            world.get::<HandleComponent<GpuMaterial>>(later).is_none(),
            "the material projected with the old image re-projects"
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
        let first = world.spawn(MeshRef::Asset(mesh));
        project.tick(&mut world);

        project.write(obj_path, QUAD_OBJ.as_bytes());
        project.invalidate(&mesh);
        project.frame(&mut world);
        let second = world.spawn(MeshRef::Asset(mesh));
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
                    shown_vertices(&world, entity),
                    Some(6),
                    "frame {frame}: the value in between"
                );
            }
            if shown_vertices(&world, first) == Some(9) {
                assert_eq!(shown_vertices(&world, second), Some(9));
                return;
            }
            project.decode();
        }
        panic!("the newest value never landed");
    }
}
